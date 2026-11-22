//! Scoped recall tombstones dominate late history and outgoing echo replay.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS recall_tombstones(
           account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
           actor_id TEXT NOT NULL,source_client_id TEXT,occurred_at_ms INTEGER NOT NULL,
           observed_at_ms INTEGER NOT NULL,
           materialized INTEGER NOT NULL CHECK(materialized IN (0,1)),
           PRIMARY KEY(account_id,thread_id,message_id)
         );
         CREATE INDEX IF NOT EXISTS recall_pending
         ON recall_tombstones(account_id,observed_at_ms) WHERE materialized=0;
         CREATE INDEX IF NOT EXISTS message_reply_recall_target ON message_reply(
             account_id,thread_id,json_extract(quote_projection,'$.message_id'),
             json_extract(quote_projection,'$.owner_id')
         ) WHERE json_valid(quote_projection);
         CREATE INDEX IF NOT EXISTS outgoing_reply_recall_target ON outgoing_reply(
             account_id,json_extract(quote_projection,'$.message_id'),
             json_extract(quote_projection,'$.owner_id')
         ) WHERE json_valid(quote_projection);
         CREATE TABLE IF NOT EXISTS recall_outbox(
           account_id TEXT NOT NULL,client_id TEXT NOT NULL,thread_id TEXT NOT NULL,
           message_id TEXT NOT NULL,source_client_id TEXT NOT NULL,
           source_sent_at_ms INTEGER NOT NULL,created_at_ms INTEGER NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('sending','unknown','rejected','acknowledged','recalled')),
           PRIMARY KEY(account_id,client_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS recall_unresolved_target
         ON recall_outbox(account_id,thread_id,message_id)
         WHERE state IN ('sending','unknown','acknowledged');",
    )?;
    Ok(())
}

pub(super) fn materialized(conn: &Connection, message: &Message) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM recall_tombstones WHERE account_id=?1
           AND thread_id=?2 AND message_id=?3 AND actor_id=?4 AND materialized=1)",
        params![
            message.account_id,
            message.thread_id,
            message.id,
            message.sender
        ],
        |row| row.get(0),
    )?)
}

pub(super) fn redact(message: &mut Message) {
    crate::domain::RECALLED_TEXT.clone_into(&mut message.text);
    message.source_client_id = None;
    message.mentions.clear();
    message.reply_blocked = true;
    message.recalled = true;
    message.quote = None;
    message.sticker = None;
    message.media = None;
}

/// Late upload acknowledgments reuse the scrubbed accepted ledger and the
/// matching canonical message without reopening the original media payload.
pub(super) fn accepted_media_replay(
    conn: &Connection,
    account: &str,
    thread: &str,
    server: &str,
    client: u64,
) -> Result<(Message, Conversation)> {
    let sent_at_ms = conn.query_row(
        "SELECT m.sent_at_ms FROM messages m JOIN recall_tombstones t
         USING(account_id,thread_id,message_id)
         WHERE m.account_id=?1 AND m.thread_id=?2 AND m.message_id=?3
           AND m.sender=?1 AND t.actor_id=?1 AND t.source_client_id=?4 AND t.materialized=1",
        params![account, thread, server, client.to_string()],
        |row| row.get(0),
    )?;
    let message = Message {
        expires_at_ms: None,
        account_id: account.into(),
        thread_id: thread.into(),
        id: server.into(),
        sender: account.into(),
        text: crate::domain::RECALLED_TEXT.into(),
        sent_at_ms,
        source_client_id: None,
        mentions: Vec::new(),
        reply_blocked: true,
        recalled: true,
        quote: None,
        sticker: None,
        media: None,
    };
    let conversation = conn.query_row(
        CONVERSATION_SQL,
        params![account, thread, MAX_PREVIEW_CHARS as i64],
        conversation_row,
    )?;
    Ok((message, conversation))
}

pub(crate) struct RecallChange {
    #[cfg(feature = "messaging")]
    pub thread_id: String,
    #[cfg(feature = "messaging")]
    pub message_id: String,
    #[cfg(feature = "messaging")]
    pub actor_id: String,
    #[cfg(feature = "messaging")]
    pub message: Option<Message>,
    #[cfg(feature = "messaging")]
    pub conversation: Option<Conversation>,
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::domain::reply::valid_source_id;
    use crate::protocol::recall::{RecallNotice, RecallReference};
    use anyhow::ensure;

    const MAX_CLIENT_ID: i64 = 9_007_199_254_740_991;
    const MAX_UNRESOLVED: i64 = 100;
    const MAX_PENDING_TOMBSTONES: i64 = 4096;
    const PENDING_TOMBSTONE_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
    use crate::domain::RECALLED_TEXT as PLACEHOLDER;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum RecallOutcome {
        Pending,
        Unknown,
        Rejected,
        Acknowledged,
        Recalled,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum RecallFailure {
        Unavailable,
        Expired,
        AlreadyRecalled,
        Unresolved,
    }

    impl std::fmt::Display for RecallFailure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Self::Unavailable => "Recall target is not available",
                Self::Expired => "Recall window has expired",
                Self::AlreadyRecalled => "Message is already recalled",
                Self::Unresolved => "A recall is still unresolved",
            })
        }
    }

    impl std::error::Error for RecallFailure {}

    pub(crate) struct PreparedRecall {
        pub client_id: u64,
        pub reference: RecallReference,
    }

    #[derive(Clone, Copy)]
    enum Scope<'a> {
        Direct(&'a str),
        Group,
    }

    fn scope<'a>(account: &str, thread: &'a str) -> Option<Scope<'a>> {
        let (kind, remote) = thread.split_once(':')?;
        if !valid_source_id(account) || !valid_source_id(remote) {
            return None;
        }
        match kind {
            "d" if account != remote => Some(Scope::Direct(remote)),
            "g" => Some(Scope::Group),
            _ => None,
        }
    }

    fn private_or_encrypted(conn: &Connection, account: &str, thread: &str) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             OR EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             OR EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
            params![account, thread],
            |row| row.get(0),
        )?)
    }

    fn original(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<Message>> {
        let mut message = conn.query_row(
            "SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![account,thread,id],
            |row| Ok(Message {
                expires_at_ms: None,
                account_id:account.into(),thread_id:thread.into(),id:id.into(),sender:row.get(0)?,
                text:row.get(1)?,sent_at_ms:row.get(2)?,source_client_id:None,mentions: Vec::new(), reply_blocked:false,recalled:false,
                quote:None,sticker:None,media:None,
            }),
        ).optional()?;
        if let Some(message) = &mut message {
            message_reply::hydrate(conn, message)?;
        }
        Ok(message)
    }

    fn ordinary(conn: &Connection, message: &Message) -> Result<bool> {
        if message.reply_reference().is_none() || message.quote.is_some() {
            return Ok(false);
        }
        let allowed = conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded')
             AND NOT EXISTS(SELECT 1 FROM list_silent_messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",
            params![message.account_id,message.thread_id,message.id],
            |row| row.get(0),
        )?;
        Ok(allowed)
    }

    fn owned_media(conn: &Connection, message: &Message) -> Result<Option<MediaKind>> {
        let Some(client) = message
            .source_client_id
            .as_deref()
            .filter(|id| valid_source_id(id))
        else {
            return Ok(None);
        };
        if message.sender != message.account_id
            || message.quote.is_some()
            || !message.mentions.is_empty()
        {
            return Ok(None);
        }
        let (kind, ledger) = match message.text.as_str() {
            text if text == MediaKind::Photo.legacy_placeholder() => {
                (MediaKind::Photo, "photo_outbox")
            }
            text if text == MediaKind::File.legacy_placeholder() => {
                (MediaKind::File, "attachment_outbox")
            }
            _ => return Ok(None),
        };
        if !message_media::projection(conn, message)?.is_some_and(|media| media.kind() == kind) {
            return Ok(None);
        }
        let accepted = conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM {ledger} WHERE account_id=?1 AND thread_id=?2
              AND server_id=?3 AND client_id=?4 AND state='accepted' AND media_metadata IS NOT NULL)
              AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2
                  AND message_id=?3 AND kind='forwarded')"
            ),
            params![message.account_id, message.thread_id, message.id, client],
            |row| row.get::<_, bool>(0),
        )?;
        Ok(accepted.then_some(kind))
    }

    fn scrub_owned_media(conn: &Connection, message: &Message, kind: MediaKind) -> Result<()> {
        let scrubbed = match kind {
            MediaKind::Photo => photos::scrub_recalled(conn, message)?,
            MediaKind::File => conn.execute(
                "UPDATE attachment_outbox SET file_name='',file_id=NULL,file_url=NULL,
                    parameters=NULL,parameters_hash=NULL,media_metadata=NULL,checksum='00000000000000000000000000000000'
                 WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND client_id=?4 AND state='accepted'",
                params![message.account_id, message.thread_id, message.id, message.source_client_id],
            )?,
            _ => bail!(FailureReason::ConflictingEcho),
        };
        ensure!(scrubbed == 1, FailureReason::ConflictingEcho);
        conn.execute(
            "DELETE FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![message.account_id, message.thread_id, message.id],
        )?;
        Ok(())
    }

    pub(in super::super) fn materialize_pending_media(
        conn: &Connection,
        message: &Message,
    ) -> Result<Option<RecallChange>> {
        let Some(client) = message.source_client_id.as_deref() else {
            return Ok(None);
        };
        let Some(prior) = tombstone(conn, &message.account_id, &message.thread_id, &message.id)?
        else {
            return Ok(None);
        };
        if prior.materialized
            || prior.actor != message.sender
            || prior.source_client.as_deref() != Some(client)
            || owned_media(conn, message)?.is_none()
        {
            return Ok(None);
        }
        write(
            conn,
            &message.account_id,
            &message.thread_id,
            &message.id,
            &prior.actor,
            Some(client),
            prior.occurred_at_ms,
        )
    }

    struct Tombstone {
        actor: String,
        source_client: Option<String>,
        materialized: bool,
        occurred_at_ms: i64,
    }

    fn tombstone(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<Tombstone>> {
        Ok(conn
            .query_row(
                "SELECT actor_id,source_client_id,materialized,occurred_at_ms FROM recall_tombstones
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account, thread, id],
                |row| {
                    Ok(Tombstone {
                        actor: row.get(0)?,
                        source_client: row.get(1)?,
                        materialized: row.get(2)?,
                        occurred_at_ms: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    fn same_cli(prior: Option<&str>, incoming: Option<&str>) -> bool {
        prior.zip(incoming).is_none_or(|(a, b)| a == b)
    }

    /// Called before projecting any replay of a potentially recalled row.
    /// The caller supplies ordinary source eligibility from the wire mapper.
    pub(in super::super) fn redacted_copy(
        conn: &Connection,
        message: &Message,
        ordinary_text: bool,
    ) -> Result<Option<Message>> {
        redact_bound_copy(conn, message, ordinary_text, true)
    }

    pub(in super::super) fn history_redacted_copy(
        conn: &Connection,
        message: &Message,
        ordinary_text: bool,
    ) -> Result<Option<Message>> {
        redact_bound_copy(conn, message, ordinary_text, false)
    }

    fn redact_bound_copy(
        conn: &Connection,
        message: &Message,
        ordinary_text: bool,
        settle_intent: bool,
    ) -> Result<Option<Message>> {
        let Some(scope) = scope(&message.account_id, &message.thread_id) else {
            return Ok(None);
        };
        let Some(prior) = tombstone(conn, &message.account_id, &message.thread_id, &message.id)?
        else {
            return Ok(None);
        };
        let group = matches!(scope, Scope::Group);
        let client = message.source_client_id.as_deref();
        let matches_client = if group {
            prior
                .source_client
                .as_deref()
                .is_some_and(|id| client == Some(id))
        } else {
            same_cli(prior.source_client.as_deref(), client)
        };
        if message.sender != prior.actor
            || (!prior.materialized && (!ordinary_text || !matches_client))
        {
            return Ok(None);
        }
        if group && !prior.materialized {
            if message.sender == message.account_id
                && own_mention(conn, &message.account_id, &message.thread_id, client)?
            {
                return Ok(None);
            }
            // An early group notice has now matched the original ordinary row.
            redact_copies(
                conn,
                &message.account_id,
                &message.thread_id,
                &message.id,
                &prior.actor,
            )?;
            if settle_intent && message.sender == message.account_id {
                settle_own_recall(
                    conn,
                    &message.account_id,
                    &message.thread_id,
                    &message.id,
                    client,
                )?;
            }
        }
        conn.execute(
            "UPDATE recall_tombstones SET materialized=1,
                source_client_id=COALESCE(source_client_id,?4)
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND materialized=0",
            params![
                message.account_id,
                message.thread_id,
                message.id,
                message.source_client_id
            ],
        )?;
        let mut sanitized = message.clone();
        sanitized.sent_at_ms = conn.query_row(
            "SELECT sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?4",
            params![message.account_id,message.thread_id,message.id,message.sender],
            |row|row.get(0),
        ).optional()?.unwrap_or(message.sent_at_ms);
        redact(&mut sanitized);
        Ok(Some(sanitized))
    }

    pub(in super::super) fn redact_replay(
        conn: &Connection,
        message: &mut Message,
        ordinary_text: bool,
    ) -> Result<bool> {
        if let Some(redacted) = redacted_copy(conn, message, ordinary_text)? {
            *message = redacted;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// A recalled own echo can prove the old text intent landed, but only
    /// when its original, authenticated ordinary metadata matches the ledger.
    pub(in super::super) fn settle_recalled_own_echo(
        conn: &Connection,
        message: &Message,
    ) -> Result<()> {
        if message.sender != message.account_id {
            return Ok(());
        }
        let Some(client) = &message.source_client_id else {
            return Ok(());
        };
        conn.execute(
            "UPDATE outgoing SET body=?5,server_id=?3,state='accepted'
             WHERE account_id=?1 AND thread_id=?2 AND client_id=?4
               AND (server_id IS NULL OR server_id=?3) AND body=?6
               AND state IN ('sending','unknown','acknowledged','accepted')
               AND NOT EXISTS(SELECT 1 FROM outgoing_reply r WHERE r.account_id=?1 AND r.client_id=?4)
               AND NOT EXISTS(SELECT 1 FROM outgoing_mention n WHERE n.account_id=?1 AND n.client_id=?4)
               AND NOT EXISTS(SELECT 1 FROM outgoing other WHERE other.account_id=?1 AND other.thread_id=?2 AND other.server_id=?3 AND other.client_id<>?4)",
            params![message.account_id,message.thread_id,message.id,client,PLACEHOLDER,message.text],
        )?;
        Ok(())
    }

    pub(in super::super) fn redact_quote(
        conn: &Connection,
        account: &str,
        thread: &str,
        quote: &mut MessageQuote,
    ) -> Result<bool> {
        let Some(prior) = tombstone(conn, account, thread, &quote.message_id)? else {
            return Ok(false);
        };
        if quote.owner_id != prior.actor
            || (!prior.materialized
                && (thread.starts_with("g:")
                    || !same_cli(prior.source_client.as_deref(), quote.client_id.as_deref())))
        {
            return Ok(false);
        }
        PLACEHOLDER.clone_into(&mut quote.text);
        quote.message_type = 0;
        Ok(true)
    }

    fn redact_copies(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        actor: &str,
    ) -> Result<()> {
        message_reply::redact_copies(conn, account, thread, id, actor, PLACEHOLDER)
    }

    fn bound_pending(conn: &Connection, account: &str) -> Result<()> {
        conn.execute(
            "DELETE FROM recall_tombstones WHERE account_id=?1 AND materialized=0
             AND observed_at_ms < unixepoch('now')*1000-?2",
            params![account, PENDING_TOMBSTONE_AGE_MS],
        )?;
        conn.execute(
            "DELETE FROM recall_tombstones WHERE rowid IN (
                SELECT rowid FROM recall_tombstones WHERE account_id=?1 AND materialized=0
                ORDER BY observed_at_ms DESC,rowid DESC LIMIT -1 OFFSET ?2
            )",
            params![account, MAX_PENDING_TOMBSTONES],
        )?;
        Ok(())
    }

    fn own_mention(
        conn: &Connection,
        account: &str,
        thread: &str,
        client: Option<&str>,
    ) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM outgoing_mention n JOIN outgoing o
                ON o.account_id=n.account_id AND o.client_id=n.client_id
                WHERE n.account_id=?1 AND o.thread_id=?2 AND n.client_id=?3)",
            params![account, thread, client],
            |row| row.get(0),
        )?)
    }

    fn settle_own_recall(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        client: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE recall_outbox SET state='recalled'
             WHERE account_id=?1 AND thread_id=?2 AND message_id=?3
             AND state IN ('sending','unknown','acknowledged') AND (?4 IS NULL OR source_client_id=?4)",
            params![account, thread, id, client],
        )?;
        Ok(())
    }

    fn write(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        actor: &str,
        client: Option<&str>,
        time: i64,
    ) -> Result<Option<RecallChange>> {
        let Some(scope) = scope(account, thread) else {
            return Ok(None);
        };
        let group = matches!(scope, Scope::Group);
        if !valid_source_id(id)
            || !valid_source_id(actor)
            || time <= 0
            || matches!(scope, Scope::Direct(peer) if actor != account && actor != peer)
            || client.is_some_and(|id| !valid_source_id(id))
        {
            return Ok(None);
        }
        let mut previous = tombstone(conn, account, thread, id)?;
        let mut original = original(conn, account, thread, id)?;
        let mut media = None;
        if group
            && original.is_none()
            && (client.is_none()
                || (actor == account && own_mention(conn, account, thread, client)?))
        {
            return Ok(None);
        }
        if let Some(message) = &original {
            if message.sender != actor || !same_cli(message.source_client_id.as_deref(), client) {
                return Ok(None);
            }
            media = owned_media(conn, message)?;
            if media.is_none()
                && previous.as_ref().is_none_or(|prior| !prior.materialized)
                && message_media::exists(conn, message)?
            {
                return Ok(None);
            }
            if group
                && previous.as_ref().is_none_or(|prior| !prior.materialized)
                && media.is_none()
                && !ordinary(conn, message)?
            {
                return Ok(None);
            }
        }
        // A group notice with no client ID can use a verified original. Compare
        // that ID against any pending notice before materializing its tombstone.
        let client = if group {
            client.or_else(|| original.as_ref()?.source_client_id.as_deref())
        } else {
            client
        };
        if previous.as_ref().is_some_and(|prior| {
            prior.actor != actor || !same_cli(prior.source_client.as_deref(), client)
        }) {
            // A notice can precede history. If that pending notice disagreed
            // with the eventual row, let a new notice bound to the actual row
            // replace it. Never change an already materialized tombstone.
            let bound_original = match &original {
                Some(_) if group => previous.as_ref().is_some_and(|prior| !prior.materialized),
                Some(message) => media.is_some() || ordinary(conn, message)?,
                None => false,
            };
            if !bound_original {
                return Ok(None);
            }
            conn.execute(
                "DELETE FROM recall_tombstones WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account, thread, id],
            )?;
            previous = None;
        }
        if let Some(message) = &original {
            if !group && previous.is_none() && media.is_none() && !ordinary(conn, message)? {
                return Ok(None);
            }
        }
        let client = client.or_else(|| {
            original
                .as_ref()
                .and_then(|m| m.source_client_id.as_deref())
        });
        conn.execute(
            "INSERT INTO recall_tombstones VALUES(?1,?2,?3,?4,?5,?6,unixepoch('now')*1000,?7)
             ON CONFLICT(account_id,thread_id,message_id) DO UPDATE SET
              source_client_id=COALESCE(source_client_id,excluded.source_client_id),
              materialized=MAX(materialized,excluded.materialized)",
            params![account, thread, id, actor, client, time, original.is_some()],
        )?;
        if original.is_none() {
            bound_pending(conn, account)?;
        }
        if actor == account && (!group || original.is_some()) {
            settle_own_recall(conn, account, thread, id, client)?;
        }
        if !group || original.is_some() {
            redact_copies(conn, account, thread, id, actor)?;
        }
        if let Some(message) = &mut original {
            if let Some(kind) = media {
                scrub_owned_media(conn, message, kind)?;
            }
            conn.execute(
                "UPDATE messages SET body=?5 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND sender=?4",
                params![account,thread,id,actor,PLACEHOLDER],
            )?;
            if actor == account {
                conn.execute(
                    "UPDATE outgoing SET body=?4,state='accepted' WHERE account_id=?1 AND thread_id=?2 AND server_id=?3
                     AND state IN ('acknowledged','accepted')",
                    params![account,thread,id,PLACEHOLDER],
                )?;
            }
            for table in [
                "receipt_references",
                "message_unread",
                "message_local_reads",
                "delivery_receipts",
                "message_forward",
                "message_reply",
            ] {
                conn.execute(
                    &format!("DELETE FROM {table} WHERE account_id=?1 AND thread_id=?2 AND message_id=?3"),
                    params![account,thread,id],
                )?;
            }
            redact(message);
            message_reply::persist(conn, message, None)?;
        }
        let conversation = original
            .as_ref()
            .map(|_| {
                conn.query_row(
                    CONVERSATION_SQL,
                    params![account, thread, MAX_PREVIEW_CHARS as i64],
                    conversation_row,
                )
            })
            .transpose()?;
        Ok(Some(RecallChange {
            thread_id: thread.into(),
            message_id: id.into(),
            actor_id: actor.into(),
            message: original,
            conversation,
        }))
    }

    pub(in super::super) fn apply_notice(
        conn: &Connection,
        account: &str,
        notice: &RecallNotice,
    ) -> Result<Option<RecallChange>> {
        if !valid_source_id(&notice.event_id) || notice.event_id == notice.message_id {
            return Ok(None);
        }
        if delete_for_me::state(conn, account, &notice.thread_id, &notice.message_id)?.as_deref()
            == Some("deleted")
        {
            return Ok(None);
        }
        if notice.thread_id.starts_with("g:")
            && conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
                params![account, notice.thread_id],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Ok(None);
        }
        let change = write(
            conn,
            account,
            &notice.thread_id,
            &notice.message_id,
            &notice.actor_id,
            notice.client_id.as_deref(),
            notice.occurred_at_ms,
        )?;
        // An early group notice remains internal until an ordinary source is
        // matched. There is no UI change to apply to menus or quotes yet.
        Ok(change.filter(|change| !notice.thread_id.starts_with("g:") || change.message.is_some()))
    }

    fn allocate(conn: &Connection, account: &str, now: i64) -> Result<i64> {
        let last: Option<i64> = conn
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )
            .optional()?;
        let next = last
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(FailureReason::Validation)?
            .max(now);
        ensure!(
            (1..=MAX_CLIENT_ID).contains(&next),
            FailureReason::Validation
        );
        conn.execute(
            "INSERT INTO outgoing_counters(account_id,last_id) VALUES(?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",
            params![account, next],
        )?;
        Ok(next)
    }

    impl MessageStore {
        pub(crate) fn prepare_recall(
            &mut self,
            account: &str,
            thread: &str,
            id: &str,
            now: i64,
            max_age: i64,
        ) -> Result<PreparedRecall> {
            ensure!(
                scope(account, thread).is_some()
                    && valid_source_id(id)
                    && (1..=MAX_CLIENT_ID).contains(&now)
                    && max_age > 0,
                RecallFailure::Unavailable
            );
            let tx = self.conn.transaction()?;
            ensure!(
                !private_or_encrypted(&tx, account, thread)?
                    && !crate::store::group_actions::write_blocked(&tx, account, thread)?,
                RecallFailure::Unavailable
            );
            ensure!(
                tombstone(&tx, account, thread, id)?.is_none(),
                RecallFailure::AlreadyRecalled
            );
            ensure!(
                delete_for_me::state(&tx, account, thread, id)?.is_none(),
                RecallFailure::Unresolved
            );
            let original = original(&tx, account, thread, id)?.ok_or(RecallFailure::Unavailable)?;
            ensure!(original.sender == account, RecallFailure::Unavailable);
            let reference = if owned_media(&tx, &original)?.is_some() {
                ensure!(original.sent_at_ms > 0, RecallFailure::Unavailable);
                RecallReference {
                    message_id: original.id,
                    client_id: original
                        .source_client_id
                        .ok_or(RecallFailure::Unavailable)?,
                    sent_at_ms: original.sent_at_ms,
                }
            } else {
                ensure!(ordinary(&tx, &original)?, RecallFailure::Unavailable);
                RecallReference::from_source(account, &original)
                    .map_err(|_| RecallFailure::Unavailable)?
            };
            let age = now
                .checked_sub(reference.sent_at_ms)
                .ok_or(RecallFailure::Unavailable)?;
            ensure!((0..max_age).contains(&age), RecallFailure::Expired);
            let (count, duplicate): (i64, bool) = tx.query_row(
                "SELECT count(*),COALESCE(MAX(thread_id=?2 AND message_id=?3),0) FROM recall_outbox
                 WHERE account_id=?1 AND state IN ('sending','unknown','acknowledged')",
                params![account, thread, id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            ensure!(
                count < MAX_UNRESOLVED && !duplicate,
                RecallFailure::Unresolved
            );
            let client = allocate(&tx, account, now)?;
            tx.execute(
                "INSERT INTO recall_outbox VALUES(?1,?2,?3,?4,?5,?6,?7,'sending')",
                params![
                    account,
                    client.to_string(),
                    thread,
                    id,
                    reference.client_id,
                    reference.sent_at_ms,
                    now
                ],
            )?;
            tx.commit()?;
            Ok(PreparedRecall {
                client_id: client as u64,
                reference,
            })
        }

        pub(crate) fn record_recall_ack(&self, account: &str, client: u64) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            ensure!(self.conn.execute(
                "UPDATE recall_outbox SET state=CASE WHEN state='recalled' THEN state ELSE 'acknowledged' END
                 WHERE account_id=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','recalled')",
                params![account,client.to_string()],
            )?==1, FailureReason::ConflictingEcho);
            Ok(())
        }

        pub(crate) fn recall_dispatch_pending(&self, account: &str, client: u64) -> Result<bool> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let thread: Option<String> = self.conn.query_row(
                "SELECT thread_id FROM recall_outbox WHERE account_id=?1 AND client_id=?2 AND state='sending'",
                params![account, client.to_string()],
                |row| row.get(0),
            ).optional()?;
            let Some(thread) = thread else {
                return Ok(false);
            };
            if !thread.starts_with("g:") {
                return Ok(true);
            }
            Ok(!private_or_encrypted(&self.conn, account, &thread)?
                && !crate::store::group_actions::write_blocked(&self.conn, account, &thread)?)
        }

        pub(crate) fn accept_recall_ack(
            &mut self,
            account: &str,
            client: u64,
        ) -> Result<RecallChange> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let tx = self.conn.transaction()?;
            let (thread, id, source, created): (String, String, String, i64) = tx.query_row(
                "SELECT thread_id,message_id,source_client_id,created_at_ms FROM recall_outbox
                 WHERE account_id=?1 AND client_id=?2 AND state IN ('acknowledged','recalled')",
                params![account, client.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            let change = write(&tx, account, &thread, &id, account, Some(&source), created)?
                .ok_or(FailureReason::ConflictingEcho)?;
            if thread.starts_with("g:") && change.message.is_none() {
                // A matched HTTP ACK confirms the exact reserved operation
                // even if the source row was cleared while awaiting it.
                settle_own_recall(&tx, account, &thread, &id, Some(&source))?;
            }
            tx.commit()?;
            Ok(change)
        }

        pub(crate) fn fail_recall(&self, account: &str, client: u64, unknown: bool) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE recall_outbox SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",
                params![account,client.to_string(),if unknown {"unknown"} else {"rejected"}],
            )?;
            Ok(())
        }

        pub(crate) fn recall_outcome(&self, account: &str, client: u64) -> Result<RecallOutcome> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let state: String = self.conn.query_row(
                "SELECT state FROM recall_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client.to_string()],
                |row| row.get(0),
            )?;
            Ok(match state.as_str() {
                "sending" => RecallOutcome::Pending,
                "unknown" => RecallOutcome::Unknown,
                "rejected" => RecallOutcome::Rejected,
                "acknowledged" => RecallOutcome::Acknowledged,
                "recalled" => RecallOutcome::Recalled,
                _ => bail!(FailureReason::ConflictingEcho),
            })
        }

        pub(crate) fn recover_recalls(&mut self, account: &str) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE recall_outbox SET state='unknown' WHERE account_id=?1 AND state='sending'",
                [account],
            )?;
            let clients: Vec<String> = {
                let mut query=self.conn.prepare_cached("SELECT client_id FROM recall_outbox WHERE account_id=?1 AND state='acknowledged' LIMIT ?2")?;
                let rows = query.query_map(params![account, MAX_UNRESOLVED], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for client in clients {
                self.accept_recall_ack(account, client.parse()?)?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "messaging")]
pub(super) use live::{
    apply_notice, history_redacted_copy, materialize_pending_media, redact_quote, redact_replay,
    redacted_copy, settle_recalled_own_echo,
};
#[cfg(feature = "messaging")]
pub(crate) use live::{RecallFailure, RecallOutcome};

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::{
        domain::{read_state::ReceiveContext, RECALLED_TEXT},
        protocol::{forward::ForwardMarker, recall::RecallNotice},
    };
    use serde_json::json;

    fn message(thread: &str, id: &str, sender: &str, body: &str) -> Message {
        Message {
            expires_at_ms: None,
            account_id: "11".into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: sender.into(),
            text: body.into(),
            sent_at_ms: 100,
            source_client_id: Some("101".into()),
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
            quote: None,
            media: None,
            sticker: None,
        }
    }

    fn notice(id: &str, actor: &str) -> RecallNotice {
        RecallNotice {
            thread_id: "d:22".into(),
            message_id: id.into(),
            actor_id: actor.into(),
            client_id: None,
            event_id: "800".into(),
            occurred_at_ms: 201,
        }
    }

    fn receive(
        store: &mut MessageStore,
        messages: &[(ReceivedReply, Option<ForwardMarker>)],
        notices: &[RecallNotice],
        cursor: Option<(&str, &str)>,
    ) -> Result<ReceivedWithRecalls> {
        store.receive_batch_with_recalls(
            "11",
            messages,
            cursor,
            &[],
            ReceiveContext::OfflineSync,
            notices,
        )
    }

    #[test]
    fn explicit_marker_does_not_treat_a_literal_body_as_recalled() {
        let mut store = MessageStore::in_memory().unwrap();
        let ordinary = message("d:22", "7", "22", RECALLED_TEXT);
        store.insert_batch(&[ordinary]).unwrap();
        let loaded = store.refresh_messages("11", "d:22", &["7".into()]).unwrap();
        assert!(!loaded[0].is_recalled());
        receive(&mut store, &[], &[notice("7", "22")], None).unwrap();
        let loaded = store.refresh_messages("11", "d:22", &["7".into()]).unwrap();
        assert!(loaded[0].is_recalled());
        assert!(loaded[0].source_client_id.is_none());
    }

    #[test]
    fn group_notice_awaits_the_exact_ordinary_source_and_leaves_other_threads_intact() {
        let mut store = MessageStore::in_memory().unwrap();
        let original = message("g:77", "7", "22", "Tin trong nhóm");
        let mut quote = message("g:77", "8", "33", "Trích dẫn");
        quote.quote = Some(MessageQuote::from_message(&original).unwrap());
        let other_group = message("g:78", "7", "22", "Tin ở nhóm khác");
        store.insert_batch(&[quote, other_group.clone()]).unwrap();
        let mut group_notice = RecallNotice {
            thread_id: "g:77".into(),
            ..notice("7", "22")
        };
        receive(&mut store, &[], &[group_notice.clone()], None).unwrap();
        let mut wrong_client = original.clone();
        wrong_client.source_client_id = Some("201".into());
        assert!(redacted_copy(&store.conn, &wrong_client, true)
            .unwrap()
            .is_none());
        assert!(redacted_copy(&store.conn, &original, true)
            .unwrap()
            .is_none());

        group_notice.client_id = Some("101".into());
        receive(&mut store, &[], &[group_notice], None).unwrap();
        assert!(redacted_copy(&store.conn, &wrong_client, true)
            .unwrap()
            .is_none());
        let loaded = store.refresh_messages("11", "g:77", &["8".into()]).unwrap();
        assert_eq!(loaded[0].quote.as_ref().unwrap().text, "Tin trong nhóm");
        receive(
            &mut store,
            &[((original.clone(), None, None, None, None), None)],
            &[],
            None,
        )
        .unwrap();
        let loaded = store
            .refresh_messages("11", "g:77", &["7".into(), "8".into()])
            .unwrap();
        assert!(loaded
            .iter()
            .find(|message| message.id == "7")
            .unwrap()
            .is_recalled());
        assert_eq!(
            loaded
                .iter()
                .find(|message| message.id == "8")
                .unwrap()
                .quote
                .as_ref()
                .unwrap()
                .text,
            RECALLED_TEXT
        );
        assert_eq!(
            store.refresh_messages("11", "g:78", &["7".into()]).unwrap(),
            [other_group]
        );

        let existing = message("g:77", "9", "22", "Tin đã lưu");
        store.insert_batch(std::slice::from_ref(&existing)).unwrap();
        let global_only = RecallNotice {
            thread_id: "g:77".into(),
            ..notice("9", "22")
        };
        receive(&mut store, &[], &[global_only], None).unwrap();
        assert!(store.refresh_messages("11", "g:77", &["9".into()]).unwrap()[0].is_recalled());
    }

    #[test]
    fn group_recall_cannot_settle_a_typed_mention_from_a_plain_echo() {
        use crate::domain::mention::Mention;
        let mut store = MessageStore::in_memory().unwrap();
        let text = "Chào @An";
        let client = store
            .prepare_outgoing_mention(
                "11",
                "g:77",
                text,
                &[Mention {
                    uid: "22".into(),
                    pos: 5,
                    len: 3,
                }],
                101,
            )
            .unwrap();
        store.fail_outgoing("11", client, true).unwrap();
        let group_notice = RecallNotice {
            thread_id: "g:77".into(),
            client_id: Some(client.to_string()),
            ..notice("7", "11")
        };
        receive(&mut store, &[], &[group_notice], None).unwrap();
        let mut echo = message("g:77", "7", "11", text);
        echo.source_client_id = Some(client.to_string());
        assert!(redacted_copy(&store.conn, &echo, true).unwrap().is_none());
        settle_recalled_own_echo(&store.conn, &echo).unwrap();
        let state: (String, String) = store
            .conn
            .query_row(
                "SELECT state,body FROM outgoing WHERE account_id='11' AND client_id=?1",
                [client.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, ("unknown".into(), text.into()));
    }

    #[test]
    fn duplicate_group_notice_never_redacts_a_later_peer_mention() {
        let mut store = MessageStore::in_memory().unwrap();
        let incoming = RecallNotice {
            thread_id: "g:77".into(),
            client_id: Some("101".into()),
            ..notice("7", "22")
        };
        let (_, _, changes) =
            receive(&mut store, &[], std::slice::from_ref(&incoming), None).unwrap();
        assert!(changes.is_empty());

        let mut original = message("g:77", "7", "22", "@An");
        original.reply_blocked = true;
        original.mentions.push(crate::domain::mention::Mention {
            uid: "11".into(),
            pos: 0,
            len: 3,
        });
        receive(
            &mut store,
            &[((original.clone(), None, None, None, None), None)],
            std::slice::from_ref(&incoming),
            None,
        )
        .unwrap();
        let (_, _, changes) = receive(&mut store, &[], &[incoming], None).unwrap();
        assert!(changes.is_empty());
        assert_eq!(
            store.refresh_messages("11", "g:77", &["7".into()]).unwrap(),
            [original]
        );
    }

    #[test]
    fn group_notice_with_committed_source_replaces_a_mismatched_pending_client() {
        let mut store = MessageStore::in_memory().unwrap();
        let mut incoming = RecallNotice {
            thread_id: "g:77".into(),
            client_id: Some("201".into()),
            ..notice("7", "22")
        };
        receive(&mut store, &[], std::slice::from_ref(&incoming), None).unwrap();
        let original = message("g:77", "7", "22", "Tin đã lưu");
        receive(
            &mut store,
            &[((original.clone(), None, None, None, None), None)],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(
            store.refresh_messages("11", "g:77", &["7".into()]).unwrap(),
            [original]
        );
        incoming.client_id = None;
        let (_, _, changes) = receive(&mut store, &[], &[incoming], None).unwrap();
        assert!(changes[0].message.as_ref().unwrap().is_recalled());
        let stored: (String, bool) = store
            .conn
            .query_row(
                "SELECT source_client_id,materialized FROM recall_tombstones
             WHERE account_id='11' AND thread_id='g:77' AND message_id='7'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, ("101".into(), true));
    }

    #[test]
    fn early_group_own_notice_waits_for_source_while_exact_ack_settles_intent() {
        let mut store = MessageStore::in_memory().unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        let original = message("g:77", "7", "11", "Tin của tôi");
        store.insert_batch(std::slice::from_ref(&original)).unwrap();
        let intent = store.prepare_recall("11", "g:77", "7", 120, 100).unwrap();
        store.conn.execute(
            "DELETE FROM messages WHERE account_id='11' AND thread_id='g:77' AND message_id='7'",
            [],
        ).unwrap();
        let incoming = RecallNotice {
            thread_id: "g:77".into(),
            client_id: Some(intent.reference.client_id),
            ..notice("7", "11")
        };
        let (_, _, changes) = receive(&mut store, &[], &[incoming], None).unwrap();
        assert!(changes.is_empty());
        assert_eq!(
            store.recall_outcome("11", intent.client_id).unwrap(),
            RecallOutcome::Pending
        );

        store.record_recall_ack("11", intent.client_id).unwrap();
        assert_eq!(
            store
                .accept_recall_ack("11", intent.client_id)
                .unwrap()
                .message_id,
            "7"
        );
        assert_eq!(
            store.recall_outcome("11", intent.client_id).unwrap(),
            RecallOutcome::Recalled
        );
        let (messages, _, changes) = receive(
            &mut store,
            &[((original, None, None, None, None), None)],
            &[],
            None,
        )
        .unwrap();
        assert!(changes.is_empty());
        assert!(messages[0].0.is_recalled());
    }

    #[test]
    fn own_recall_sanitizes_body_outbox_quotes_search_and_late_wire() {
        let mut store = MessageStore::in_memory().unwrap();
        store.replace_live_privacy("11", &[], None).unwrap();
        let client = store
            .prepare_outgoing("11", "d:22", "Bí mật tổng hợp", 100)
            .unwrap();
        let (source, _) = store.accept_outgoing("11", client, "7", Some(100)).unwrap();
        let mut reply = message("d:22", "9", "22", "Trả lời");
        reply.quote = Some(MessageQuote::from_message(&source).unwrap());
        let raw = QuoteContent::new(
            "11",
            &json!({"ownerId":"11","globalMsgId":"7","cliMsgId":client.to_string(),
            "cliMsgType":1,"ts":100,"msg":source.text,"sensitive_extension":source.text}),
        )
        .unwrap();
        receive(
            &mut store,
            &[((reply.clone(), None, None, None, Some(raw.clone())), None)],
            &[],
            None,
        )
        .unwrap();
        let previous_draft = store
            .prepare_outgoing_reply("11", "d:22", "Lời nháp cũ", "7", 160)
            .unwrap();

        let recall = store.prepare_recall("11", "d:22", "7", 171, 100).unwrap();
        assert!(recall.client_id > previous_draft);
        store.record_recall_ack("11", recall.client_id).unwrap();
        let changed = store.accept_recall_ack("11", recall.client_id).unwrap();
        assert!(changed.message.unwrap().is_recalled());
        assert_eq!(
            store.recall_outcome("11", recall.client_id).unwrap(),
            RecallOutcome::Recalled
        );
        assert!(store
            .search_messages("11", "d:22", "Bí mật tổng hợp", 15)
            .unwrap()
            .hits
            .is_empty());
        assert!(store
            .message_delivery("11", "d:22", &["7".into()])
            .unwrap()
            .is_empty());
        let stored_body: String = store
            .conn
            .query_row(
                "SELECT body FROM outgoing WHERE account_id='11' AND client_id=?1",
                [client.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored_body, RECALLED_TEXT);
        let quote = message_reply::outgoing(&store.conn, "11", previous_draft)
            .unwrap()
            .unwrap();
        assert_eq!(quote.text, RECALLED_TEXT);
        assert!(store
            .message_quote_content("11", "d:22", "9")
            .unwrap()
            .is_none());
        assert!(store
            .prepare_outgoing_reply("11", "d:22", "Không trích lại", "7", 172)
            .is_err());

        // Late original and reply replay must not restore either copy.
        let mut stale = source;
        stale.sent_at_ms = 101;
        receive(
            &mut store,
            &[
                (
                    (stale, Some(client.to_string()), None, None, None),
                    Some(ForwardMarker::Unavailable),
                ),
                ((reply, None, None, None, Some(raw)), None),
            ],
            &[],
            None,
        )
        .unwrap();
        let loaded = store
            .refresh_messages("11", "d:22", &["7".into(), "9".into()])
            .unwrap();
        assert_eq!(loaded[0].text, RECALLED_TEXT);
        assert_eq!(loaded[0].sent_at_ms, 100);
        assert_eq!(loaded[1].quote.as_ref().unwrap().text, RECALLED_TEXT);
        let wire = store.message_quote_content("11", "d:22", "9").unwrap();
        assert!(wire.is_none_or(|raw| !raw.as_json().contains("Bí mật tổng hợp")));
    }

    #[test]
    fn matched_notice_before_history_is_atomic_with_cursor_and_only_scoped() {
        let mut store = MessageStore::in_memory().unwrap();
        let remote = message("d:22", "7", "22", "Bí mật tổng hợp");
        let other = message("d:33", "7", "33", "Hội thoại khác");
        store.insert_batch(std::slice::from_ref(&other)).unwrap();
        let mut scoped = notice("7", "22");
        scoped.client_id = Some("101".into());
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_recall_cursor BEFORE INSERT ON receive_cursors
             BEGIN SELECT RAISE(ABORT,'synthetic rollback'); END;",
            )
            .unwrap();
        let same_frame = [(
            (remote.clone(), None, None, None, None),
            Some(ForwardMarker::Ordinary),
        )];
        assert!(receive(
            &mut store,
            &same_frame,
            std::slice::from_ref(&scoped),
            Some(("510_0", "99"))
        )
        .is_err());
        assert_eq!(store.receive_cursor("11", "510_0").unwrap(), "1");
        assert!(!materialized(&store.conn, &remote).unwrap());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_recall_cursor")
            .unwrap();

        receive(&mut store, &[], &[scoped], Some(("510_0", "99"))).unwrap();
        assert!(!materialized(&store.conn, &remote).unwrap());
        receive(&mut store, &same_frame, &[], None).unwrap();
        assert!(materialized(&store.conn, &remote).unwrap());
        let row = store.refresh_messages("11", "d:33", &["7".into()]).unwrap();
        assert_eq!(row, [other]);
    }

    #[test]
    fn acknowledgement_survives_local_failure_and_is_repaired_on_restart() {
        let mut store = MessageStore::in_memory().unwrap();
        let row = message("d:22", "7", "11", "Bí mật tổng hợp");
        store.replace_live_privacy("11", &[], None).unwrap();
        store.insert_batch(&[row]).unwrap();
        assert!(store.prepare_recall("11", "d:22", "7", 200, 100).is_err());
        let recall = store.prepare_recall("11", "d:22", "7", 199, 100).unwrap();
        store.record_recall_ack("11", recall.client_id).unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_recall_projection BEFORE UPDATE OF body ON messages
             BEGIN SELECT RAISE(ABORT,'synthetic failed projection'); END;",
            )
            .unwrap();
        assert!(store.accept_recall_ack("11", recall.client_id).is_err());
        assert_eq!(
            store.recall_outcome("11", recall.client_id).unwrap(),
            RecallOutcome::Acknowledged
        );
        store
            .conn
            .execute_batch("DROP TRIGGER reject_recall_projection")
            .unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.recover_recalls("11").unwrap();
        assert_eq!(
            reopened.recall_outcome("11", recall.client_id).unwrap(),
            RecallOutcome::Recalled
        );
        assert!(reopened
            .refresh_messages("11", "d:22", &["7".into()])
            .unwrap()[0]
            .is_recalled());
    }
}
