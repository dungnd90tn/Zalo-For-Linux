//! Durable one-recipient forwarding. Ordinary sends never own this ledger.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS forward_outbox (
            account_id TEXT NOT NULL, client_id TEXT NOT NULL, thread_id TEXT NOT NULL,
            source_thread_id TEXT NOT NULL, source_message_id TEXT NOT NULL,
            body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=65536),
            reference TEXT NOT NULL CHECK(length(CAST(reference AS BLOB))<=4096),
            created_at_ms INTEGER NOT NULL, server_id TEXT, server_ts INTEGER,
            echo_received INTEGER NOT NULL DEFAULT 0 CHECK(echo_received IN (0,1)),
            state TEXT NOT NULL CHECK(state IN ('sending','unknown','acknowledged','accepted','rejected')),
            PRIMARY KEY(account_id,client_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS forward_server_id
            ON forward_outbox(account_id,thread_id,server_id) WHERE server_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS forward_unresolved
            ON forward_outbox(account_id,source_thread_id,source_message_id,thread_id)
            WHERE state IN ('sending','unknown','acknowledged');
         CREATE TABLE IF NOT EXISTS message_forward (
            account_id TEXT NOT NULL, thread_id TEXT NOT NULL, message_id TEXT NOT NULL,
            reference TEXT CHECK(reference IS NULL OR length(CAST(reference AS BLOB))<=4096),
            kind TEXT NOT NULL CHECK(kind IN ('ordinary','forwarded')),
            CHECK(kind='forwarded' OR reference IS NULL),
            PRIMARY KEY(account_id,thread_id,message_id),
            FOREIGN KEY(account_id,thread_id,message_id)
                REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE
         );",
    )?;
    Ok(())
}

pub(super) fn server_reserved(
    conn: &Connection,
    account: &str,
    thread: &str,
    id: &str,
) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM forward_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
         OR EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded')",
        params![account,thread,id],
        |row| row.get(0),
    )?)
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::domain::reply::valid_source_id;
    use crate::protocol::forward::{ForwardMarker, ForwardReference};
    use anyhow::ensure;

    const MAX_CLIENT_ID: i64 = 9_007_199_254_740_991;
    const MAX_UNRESOLVED: i64 = 100;
    const MAX_REFERENCE_BYTES: usize = 4096;
    pub(crate) const MAX_TARGETS: usize = 25;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ForwardFailure {
        Source,
        Destination,
        Unresolved,
    }

    impl std::fmt::Display for ForwardFailure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Self::Source => "Forward source is not available",
                Self::Destination => "Forward destination is not available",
                Self::Unresolved => "A forward is still unresolved",
            })
        }
    }

    impl std::error::Error for ForwardFailure {}

    pub(crate) struct PreparedForward {
        pub client_id: u64,
        pub text: String,
        pub reference: ForwardReference,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ForwardTarget {
        pub thread_id: String,
        pub title: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ForwardOutcome {
        Pending,
        Unknown,
        Rejected,
        Acknowledged,
        Accepted,
    }

    fn direct_peer<'a>(account: &str, thread: &'a str) -> Option<&'a str> {
        thread
            .strip_prefix("d:")
            .filter(|peer| valid_source_id(account) && valid_source_id(peer) && *peer != account)
    }

    fn normal_thread(conn: &Connection, account: &str, thread: &str) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM temp.live_privacy WHERE account_id=?1 AND ready=1)
             AND NOT EXISTS(SELECT 1 FROM temp.live_hidden WHERE account_id=?1 AND thread_id=?2)
             AND NOT EXISTS(SELECT 1 FROM e2ee_threads WHERE account_id=?1 AND thread_id=?2)",
            params![account, thread],
            |row| row.get(0),
        )?)
    }

    fn source(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<(String, ForwardReference)> {
        ensure!(
            direct_peer(account, thread).is_some()
                && valid_source_id(id)
                && normal_thread(conn, account, thread)?,
            ForwardFailure::Source
        );
        let mut source = conn.query_row(
            "SELECT m.sender,m.body,m.sent_at_ms FROM messages m
             WHERE m.account_id=?1 AND m.thread_id=?2 AND m.message_id=?3
             AND NOT EXISTS(SELECT 1 FROM message_content WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_info WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_media WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)
             AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded')
             AND (EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='ordinary')
                  OR (m.sender=?1 AND EXISTS(SELECT 1 FROM outgoing o WHERE o.account_id=?1 AND o.thread_id=?2 AND o.server_id=?3 AND o.body=m.body AND o.state='accepted')))
             AND NOT EXISTS(SELECT 1 FROM list_silent_messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3)",
            params![account, thread, id],
            |row| Ok(Message {
                account_id: account.into(), thread_id: thread.into(), id: id.into(),
                sender: row.get(0)?, text: row.get(1)?, sent_at_ms: row.get(2)?,
                source_client_id: None, mentions: Vec::new(), reply_blocked: false, recalled: false, quote: None, sticker: None, media: None,
            }),
        ).optional()?.ok_or(ForwardFailure::Source)?;
        message_reply::hydrate(conn, &mut source)?;
        source.validate().map_err(|_| ForwardFailure::Source)?;
        ensure!(
            !canonical_plain_text(&source.text).is_empty()
                && !source.text.chars().any(|c| matches!(c, '\u{0}'..='\u{7}')),
            ForwardFailure::Source
        );
        let reference =
            ForwardReference::from_source(&source).map_err(|_| ForwardFailure::Source)?;
        Ok((source.text, reference))
    }

    fn destination(conn: &Connection, account: &str, thread: &str) -> Result<()> {
        let peer = direct_peer(account, thread).ok_or(ForwardFailure::Destination)?;
        ensure!(
            normal_thread(conn, account, thread)?,
            ForwardFailure::Destination
        );
        let eligible: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM directory_entries
             WHERE account_id=?1 AND thread_id=?2 AND remote_id=?3 AND is_group=0
             AND is_friend=1 AND COALESCE(is_blocked,0)=0)",
            params![account, thread, peer],
            |row| row.get(0),
        )?;
        ensure!(eligible, ForwardFailure::Destination);
        Ok(())
    }

    fn encode(reference: &ForwardReference) -> Result<String> {
        let encoded = serde_json::to_string(reference)?;
        ensure!(
            encoded.len() <= MAX_REFERENCE_BYTES,
            FailureReason::Validation
        );
        Ok(encoded)
    }

    fn decode(encoded: &str) -> Result<ForwardReference> {
        ensure!(
            encoded.len() <= MAX_REFERENCE_BYTES,
            FailureReason::ConflictingMetadata
        );
        serde_json::from_str(encoded).map_err(|_| FailureReason::ConflictingMetadata.into())
    }

    fn allocate(conn: &Connection, account: &str, now_ms: i64) -> Result<i64> {
        ensure!(
            (1..=MAX_CLIENT_ID).contains(&now_ms),
            FailureReason::Validation
        );
        let previous: Option<i64> = conn
            .query_row(
                "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )
            .optional()?;
        let next = previous
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(FailureReason::Validation)?
            .max(now_ms);
        ensure!(next <= MAX_CLIENT_ID, FailureReason::Validation);
        conn.execute(
            "INSERT INTO outgoing_counters(account_id,last_id) VALUES(?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",
            params![account, next],
        )?;
        Ok(next)
    }

    struct ForwardRow {
        client: String,
        thread: String,
        body: String,
        reference: String,
        state: String,
        server: Option<String>,
        server_ts: Option<i64>,
        created: i64,
        echoed: bool,
    }

    impl ForwardRow {
        fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
            Ok(Self {
                client: row.get(0)?,
                thread: row.get(1)?,
                body: row.get(2)?,
                reference: row.get(3)?,
                state: row.get(4)?,
                server: row.get(5)?,
                server_ts: row.get(6)?,
                created: row.get(7)?,
                echoed: row.get(8)?,
            })
        }
    }

    fn client_row(conn: &Connection, account: &str, client: &str) -> Result<Option<ForwardRow>> {
        Ok(conn
            .query_row(
                "SELECT client_id,thread_id,body,reference,state,server_id,server_ts,created_at_ms,echo_received
             FROM forward_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client],
                ForwardRow::from_row,
            )
            .optional()?)
    }

    fn server_row(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<ForwardRow>> {
        Ok(conn
            .query_row(
                "SELECT client_id,thread_id,body,reference,state,server_id,server_ts,created_at_ms,echo_received
             FROM forward_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3",
                params![account, thread, id],
                ForwardRow::from_row,
            )
            .optional()?)
    }

    pub(in super::super) struct ForwardMatch {
        pub retimed: bool,
        pub reference: ForwardReference,
    }

    /// Called only after the forward marker has been removed from the ordinary
    /// echo path. Exact wire reference is required to settle an unbound intent.
    pub(in super::super) fn reconcile_received(
        conn: &Connection,
        message: &mut Message,
        hint: Option<&str>,
        marker: Option<&ForwardMarker>,
    ) -> Result<Option<ForwardMatch>> {
        let reference = match marker {
            Some(ForwardMarker::Parsed(reference)) => Some(reference),
            Some(ForwardMarker::Unavailable | ForwardMarker::StructuredWebchat) => return Ok(None),
            Some(ForwardMarker::Ordinary) | None => None,
        };
        if message.sender != message.account_id
            || message.quote.is_some()
            || message.sticker.is_some()
            || message.media.is_some()
        {
            return Ok(None);
        }
        let explicit = hint
            .map(|id| client_row(conn, &message.account_id, id))
            .transpose()?
            .flatten();
        let bound = server_row(conn, &message.account_id, &message.thread_id, &message.id)?;
        if explicit
            .as_ref()
            .zip(bound.as_ref())
            .is_some_and(|(a, b)| a.client != b.client)
        {
            return Ok(None);
        }
        let Some(row) = explicit.or(bound) else {
            return Ok(None);
        };
        if row.thread != message.thread_id
            || row.body != message.text
            || !matches!(
                row.state.as_str(),
                "sending" | "unknown" | "acknowledged" | "accepted"
            )
            || row.server.as_deref().is_some_and(|id| id != message.id)
        {
            return Ok(None);
        }
        let expected = decode(&row.reference)?;
        if reference.map_or(row.server.as_deref() != Some(&message.id), |actual| {
            &expected != actual
        }) {
            return Ok(None);
        }
        let previous: Option<(String,String,i64)> = conn.query_row(
            "SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![message.account_id,message.thread_id,message.id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).optional()?;
        if let Some((sender, body, _)) = &previous {
            ensure!(
                sender == &message.account_id && body == &message.text,
                FailureReason::ConflictingEcho
            );
            verify_prior(
                conn,
                &message.account_id,
                &message.thread_id,
                &message.id,
                &expected,
                reference.is_some(),
            )?;
        }
        ensure!(
            no_other_ack(conn, &message.account_id, &message.thread_id, &message.id)?,
            FailureReason::ConflictingEcho
        );
        let retimed =
            !row.echoed && previous.is_some_and(|(_, _, sent)| sent != message.sent_at_ms);
        if retimed {
            conn.execute(
                "UPDATE messages SET sent_at_ms=?4 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![message.account_id, message.thread_id, message.id, message.sent_at_ms],
            )?;
        }
        ensure!(conn.execute(
            "UPDATE forward_outbox SET state='accepted',server_id=?3,server_ts=?4,echo_received=1 WHERE account_id=?1 AND client_id=?2",
            params![message.account_id,row.client,message.id,message.sent_at_ms],
        )? == 1, FailureReason::ConflictingEcho);
        message.source_client_id = Some(row.client);
        message.reply_blocked = true;
        Ok(Some(ForwardMatch {
            retimed,
            reference: expected,
        }))
    }

    fn verify_prior(
        conn: &Connection,
        account: &str,
        thread: &str,
        id: &str,
        expected: &ForwardReference,
        has_wire_proof: bool,
    ) -> Result<()> {
        let previous: Option<Option<String>> = conn.query_row(
            "SELECT reference FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded'",
            params![account,thread,id],
            |row| row.get(0),
        ).optional()?;
        let matches = match previous {
            Some(Some(raw)) => decode(&raw)? == *expected,
            Some(None) => has_wire_proof,
            None => false,
        };
        ensure!(matches, FailureReason::ConflictingEcho);
        Ok(())
    }

    fn no_other_ack(conn: &Connection, account: &str, thread: &str, server: &str) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)",
            params![account,thread,server],
            |row| row.get(0),
        )?)
    }

    pub(in super::super) fn persist(
        conn: &Connection,
        message: &Message,
        marker: &ForwardMarker,
    ) -> Result<bool> {
        let (kind, reference) = match marker {
            ForwardMarker::Ordinary => {
                if message.quote.is_some() || message.reply_reference().is_none() {
                    return Ok(false);
                }
                ("ordinary", None)
            }
            ForwardMarker::Unavailable => ("forwarded", None),
            ForwardMarker::Parsed(reference) => ("forwarded", Some(encode(reference)?)),
            ForwardMarker::StructuredWebchat => return Ok(false),
        };
        let old: Option<String> = conn.query_row(
            "SELECT reference FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
            params![message.account_id, message.thread_id, message.id],
            |row| row.get::<_, Option<String>>(0),
        ).optional()?.flatten();
        if old
            .as_ref()
            .zip(reference.as_ref())
            .is_some_and(|(a, b)| a != b)
        {
            bail!(FailureReason::ConflictingMetadata);
        }
        Ok(conn.execute(
            "INSERT INTO message_forward VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(account_id,thread_id,message_id) DO UPDATE SET reference=excluded.reference,kind=excluded.kind
             WHERE excluded.kind='forwarded'
                 AND (kind='ordinary' OR (reference IS NULL AND excluded.reference IS NOT NULL))",
            params![message.account_id,message.thread_id,message.id,reference,kind],
        )? > 0)
    }

    impl MessageStore {
        pub(crate) fn forward_targets(
            &self,
            account: &str,
            source_thread: &str,
            source_id: &str,
            query: &str,
        ) -> Result<Vec<ForwardTarget>> {
            source(&self.conn, account, source_thread, source_id)?;
            ensure!(
                query.chars().count() <= 128 && !query.chars().any(char::is_control),
                FailureReason::Validation
            );
            let folded = crate::domain::fold_search(query);
            let pattern = format!(
                "%{}%",
                folded
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let mut statement = self.conn.prepare_cached(
                "SELECT d.thread_id,d.display_name FROM directory_entries d
                 WHERE d.account_id=?1 AND d.is_group=0 AND d.remote_id<>?1
                 AND d.is_friend=1 AND COALESCE(d.is_blocked,0)=0
                 AND d.search_name LIKE ?2 ESCAPE '\\'
                 AND NOT EXISTS(SELECT 1 FROM temp.live_hidden h WHERE h.account_id=d.account_id AND h.thread_id=d.thread_id)
                 AND NOT EXISTS(SELECT 1 FROM e2ee_threads e WHERE e.account_id=d.account_id AND e.thread_id=d.thread_id)
                 ORDER BY d.display_name,d.thread_id LIMIT ?3",
            )?;
            let rows =
                statement.query_map(params![account, pattern, MAX_TARGETS as i64], |row| {
                    Ok(ForwardTarget {
                        thread_id: row.get(0)?,
                        title: row.get(1)?,
                    })
                })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        }

        pub(crate) fn prepare_forward(
            &mut self,
            account: &str,
            source_thread: &str,
            source_id: &str,
            thread: &str,
            now_ms: i64,
        ) -> Result<PreparedForward> {
            let tx = self.conn.transaction()?;
            let (text, reference) = source(&tx, account, source_thread, source_id)?;
            destination(&tx, account, thread)?;
            let (unresolved, duplicate): (i64, bool) = tx.query_row(
                "SELECT count(*),COALESCE(MAX(source_thread_id=?2 AND source_message_id=?3 AND thread_id=?4),0)
                 FROM forward_outbox WHERE account_id=?1 AND state IN ('sending','unknown','acknowledged')",
                params![account, source_thread, source_id, thread],
                |row| Ok((row.get(0)?,row.get(1)?)),
            )?;
            ensure!(
                unresolved < MAX_UNRESOLVED && !duplicate,
                ForwardFailure::Unresolved
            );
            let client = allocate(&tx, account, now_ms)?;
            tx.execute(
                "INSERT INTO forward_outbox(account_id,client_id,thread_id,source_thread_id,source_message_id,body,reference,created_at_ms,state)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'sending')",
                params![account,client.to_string(),thread,source_thread,source_id,text,encode(&reference)?,now_ms],
            )?;
            tx.commit()?;
            Ok(PreparedForward {
                client_id: client as u64,
                text,
                reference,
            })
        }

        pub(crate) fn fail_forward(&self, account: &str, client: u64, unknown: bool) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE forward_outbox SET state=?3 WHERE account_id=?1 AND client_id=?2 AND state='sending'",
                params![account,client.to_string(),if unknown {"unknown"} else {"rejected"}],
            )?;
            Ok(())
        }

        pub(crate) fn forward_outcome(&self, account: &str, client: u64) -> Result<ForwardOutcome> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let state: String = self.conn.query_row(
                "SELECT state FROM forward_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client.to_string()],
                |row| row.get(0),
            )?;
            Ok(match state.as_str() {
                "sending" => ForwardOutcome::Pending,
                "unknown" => ForwardOutcome::Unknown,
                "rejected" => ForwardOutcome::Rejected,
                "acknowledged" => ForwardOutcome::Acknowledged,
                "accepted" => ForwardOutcome::Accepted,
                _ => bail!(FailureReason::ConflictingEcho),
            })
        }

        pub(crate) fn recover_forwards(&mut self, account: &str) -> Result<()> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            self.conn.execute(
                "UPDATE forward_outbox SET state='unknown' WHERE account_id=?1 AND state='sending'",
                [account],
            )?;
            let clients: Vec<String> = {
                let mut query = self.conn.prepare_cached(
                    "SELECT client_id FROM forward_outbox WHERE account_id=?1 AND state='acknowledged' LIMIT ?2",
                )?;
                let rows = query.query_map(params![account, MAX_UNRESOLVED], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for client in clients {
                self.accept_forward_ack(account, client.parse()?)?;
            }
            Ok(())
        }

        pub(crate) fn record_forward_ack(
            &self,
            account: &str,
            client: u64,
            server: &str,
            timestamp: Option<i64>,
        ) -> Result<()> {
            ensure!(
                valid_source_id(account)
                    && valid_source_id(server)
                    && timestamp.is_none_or(|ts| ts >= 0),
                FailureReason::Validation
            );
            ensure!(self.conn.execute(
                "UPDATE forward_outbox SET state=CASE WHEN state='accepted' THEN state ELSE 'acknowledged' END,
                    server_id=?3, server_ts=COALESCE(server_ts,?4)
                 WHERE account_id=?1 AND client_id=?2 AND state IN ('sending','unknown','acknowledged','accepted')
                 AND (server_id IS NULL OR server_id=?3)
                 AND NOT EXISTS(SELECT 1 FROM outgoing WHERE account_id=?1 AND thread_id=forward_outbox.thread_id AND server_id=?3)
                 AND NOT EXISTS(SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND thread_id=forward_outbox.thread_id AND server_id=?3)
                 AND NOT EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND thread_id=forward_outbox.thread_id AND server_id=?3)",
                params![account,client.to_string(),server,timestamp],
            )?==1, FailureReason::ConflictingEcho);
            Ok(())
        }

        pub(crate) fn accept_forward_ack(
            &mut self,
            account: &str,
            client: u64,
        ) -> Result<(Message, Conversation)> {
            ensure!(valid_source_id(account), FailureReason::Validation);
            let tx = self.conn.transaction()?;
            let row = client_row(&tx, account, &client.to_string())?
                .ok_or(FailureReason::ConflictingEcho)?;
            let server = row.server.ok_or(FailureReason::ConflictingEcho)?;
            ensure!(
                matches!(row.state.as_str(), "acknowledged" | "accepted"),
                FailureReason::ConflictingEcho
            );
            ensure!(
                no_other_ack(&tx, account, &row.thread, &server)?,
                FailureReason::ConflictingEcho
            );
            let expected = decode(&row.reference)?;
            let previous: Option<(String,String,i64)> = tx.query_row(
                "SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![account,row.thread,server],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
            ).optional()?;
            if let Some((sender, body, _)) = &previous {
                ensure!(
                    sender == account && body == &row.body,
                    FailureReason::ConflictingEcho
                );
                verify_prior(&tx, account, &row.thread, &server, &expected, false)?;
            }
            let message = Message {
                account_id: account.into(),
                thread_id: row.thread,
                id: server,
                sender: account.into(),
                text: row.body,
                source_client_id: Some(row.client),
                mentions: Vec::new(),
                reply_blocked: true,
                recalled: false,
                quote: None,
                sticker: None,
                media: None,
                sent_at_ms: previous
                    .map(|(_, _, ts)| ts)
                    .or(row.server_ts)
                    .unwrap_or(row.created),
            };
            message.validate()?;
            insert_validated(&tx, std::slice::from_ref(&message))?;
            persist(&tx, &message, &ForwardMarker::Parsed(expected))?;
            tx.execute(
                "UPDATE forward_outbox SET state='accepted' WHERE account_id=?1 AND client_id=?2",
                params![account, client.to_string()],
            )?;
            let conversation = tx.query_row(
                CONVERSATION_SQL,
                params![account, message.thread_id, MAX_PREVIEW_CHARS as i64],
                conversation_row,
            )?;
            tx.commit()?;
            Ok((message, conversation))
        }
    }
}

#[cfg(feature = "messaging")]
pub use live::ForwardTarget;
#[cfg(feature = "messaging")]
pub(super) use live::{persist, reconcile_received};
#[cfg(feature = "messaging")]
pub(crate) use live::{ForwardFailure, ForwardOutcome};

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::{
        domain::read_state::{DeliveryState, ReceiveContext},
        protocol::forward::ForwardMarker,
    };

    fn message(thread: &str, id: &str, sender: &str, client: &str) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: sender.into(),
            text: "Xin chào đầu tiên".into(),
            sent_at_ms: 100,
            source_client_id: Some(client.into()),
            quote: None,
            media: None,
            sticker: None,
            mentions: Vec::new(),
            reply_blocked: false,
            recalled: false,
        }
    }

    fn receive(
        store: &mut MessageStore,
        message: Message,
        marker: Option<ForwardMarker>,
        cursor: Option<(&str, &str)>,
    ) -> Result<ReceivedWithUnread> {
        let client = message.source_client_id.clone();
        store.receive_batch_with_forwards(
            "11",
            &[((message, client, None, None, None), marker)],
            cursor,
            &[],
            ReceiveContext::OfflineSync,
        )
    }

    fn fixture() -> (MessageStore, Message) {
        let store = MessageStore::in_memory().unwrap();
        // A visible friend's name deliberately includes accents for picker search.
        store.conn.execute_batch(
            "INSERT INTO directory_entries(account_id,thread_id,remote_id,is_group,display_name,search_name,is_friend,is_blocked,observed_at_ms)
             VALUES('11','d:33','33',0,'Bảo Trân','bao tran',1,0,1),
                   ('11','d:55','55',0,'Bảo Võ','bao vo',1,1,1),
                   ('11','d:66','66',0,'Bảo Anh','bao anh',NULL,0,1),
                   ('77','d:44','44',0,'Bảo Nam','bao nam',1,0,1);",
        ).unwrap();
        (store, message("d:22", "100", "22", "200"))
    }

    #[test]
    fn picker_and_reservation_require_current_normal_source_and_friend_destination() {
        let (mut store, source) = fixture();
        store.insert_batch(std::slice::from_ref(&source)).unwrap();
        assert!(store.forward_targets("11", "d:22", "100", "Bảo").is_err());
        store.replace_live_privacy("11", &[], None).unwrap();
        assert!(
            store.forward_targets("11", "d:22", "100", "Bảo").is_err(),
            "schema thirteen cached text cannot establish forward provenance"
        );
        receive(
            &mut store,
            source.clone(),
            Some(ForwardMarker::Ordinary),
            None,
        )
        .unwrap();
        let targets = store.forward_targets("11", "d:22", "100", "Bảo").unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].thread_id, "d:33");
        assert!(store
            .prepare_forward("11", "d:22", "100", "d:55", 1000)
            .is_err());
        assert!(store
            .prepare_forward("11", "d:22", "100", "d:66", 1000)
            .is_err());
        assert!(store
            .prepare_forward("11", "d:22", "100", "d:44", 1000)
            .is_err());
        store
            .replace_live_privacy("11", &["d:33".into()], Some("d:33"))
            .unwrap();
        assert!(store
            .forward_targets("11", "d:22", "100", "")
            .unwrap()
            .is_empty());
        assert!(store
            .prepare_forward("11", "d:22", "100", "d:33", 1000)
            .is_err());
        store
            .replace_live_privacy("11", &["d:22".into()], Some("d:22"))
            .unwrap();
        assert!(store.forward_targets("11", "d:22", "100", "").is_err());
        store.replace_live_privacy("11", &[], None).unwrap();
        receive(&mut store, source, Some(ForwardMarker::Unavailable), None).unwrap();
        assert!(
            store
                .prepare_forward("11", "d:22", "100", "d:33", 1000)
                .is_err(),
            "a later non-ordinary forward marker blocks use as a clean source"
        );
    }

    #[test]
    fn typed_echo_commits_with_cursor_and_delayed_ack_never_downgrades() {
        let (mut store, source) = fixture();
        store.replace_live_privacy("11", &[], None).unwrap();
        receive(&mut store, source, Some(ForwardMarker::Ordinary), None).unwrap();
        let send = store
            .prepare_forward("11", "d:22", "100", "d:33", 1000)
            .unwrap();
        let ordinary = store
            .prepare_outgoing("11", "d:33", &send.text, 1000)
            .unwrap();
        assert_eq!(ordinary, send.client_id + 1);
        let echo = message("d:33", "300", "11", &send.client_id.to_string());
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_forward_cursor BEFORE INSERT ON receive_cursors
             BEGIN SELECT RAISE(ABORT,'synthetic forward cursor failure'); END;",
            )
            .unwrap();
        assert!(receive(
            &mut store,
            echo.clone(),
            Some(ForwardMarker::Parsed(send.reference.clone())),
            Some(("510_0", "99")),
        )
        .is_err());
        assert_eq!(
            store.forward_outcome("11", send.client_id).unwrap(),
            ForwardOutcome::Pending
        );
        assert!(!server_reserved(&store.conn, "11", "d:33", "300").unwrap());
        store
            .conn
            .execute_batch("DROP TRIGGER reject_forward_cursor")
            .unwrap();
        receive(
            &mut store,
            echo,
            Some(ForwardMarker::Parsed(send.reference)),
            Some(("510_0", "99")),
        )
        .unwrap();
        store.fail_forward("11", send.client_id, true).unwrap();
        assert_eq!(
            store.forward_outcome("11", send.client_id).unwrap(),
            ForwardOutcome::Accepted
        );
        assert!(store.record_outgoing_ack("11", ordinary, "300").is_err());
        assert!(store
            .record_forward_ack("11", send.client_id, "301", None)
            .is_err());
        store
            .record_forward_ack("11", send.client_id, "300", Some(111))
            .unwrap();
        let (stored, _) = store.accept_forward_ack("11", send.client_id).unwrap();
        assert_eq!(stored.sent_at_ms, 100);
        assert!(stored.reply_blocked);
        assert_eq!(store.receive_cursor("11", "510_0").unwrap(), "99");
        assert_eq!(
            store
                .message_delivery("11", "d:33", &["300".into()])
                .unwrap()[0]
                .state,
            DeliveryState::Accepted
        );
    }

    #[test]
    fn unresolved_forward_requires_reference_or_durable_binding_and_survives_restart() {
        let (mut store, source) = fixture();
        store.replace_live_privacy("11", &[], None).unwrap();
        receive(&mut store, source, Some(ForwardMarker::Ordinary), None).unwrap();
        let send = store
            .prepare_forward("11", "d:22", "100", "d:33", 1000)
            .unwrap();
        store.recover_forwards("11").unwrap();
        assert_eq!(
            store.forward_outcome("11", send.client_id).unwrap(),
            ForwardOutcome::Unknown
        );
        assert!(store
            .prepare_forward("11", "d:22", "100", "d:33", 1000)
            .is_err());
        let ordinary = store
            .prepare_outgoing("11", "d:33", &send.text, 1000)
            .unwrap();
        let echo = message("d:33", "300", "11", &ordinary.to_string());
        receive(&mut store, echo, Some(ForwardMarker::Unavailable), None).unwrap();
        assert!(
            store.record_outgoing_ack("11", ordinary, "300").is_err(),
            "a malformed typed echo must never settle plain text"
        );
        assert_eq!(
            store.forward_outcome("11", send.client_id).unwrap(),
            ForwardOutcome::Unknown
        );
        store
            .record_forward_ack("11", send.client_id, "301", Some(99))
            .unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_forward_projection BEFORE INSERT ON message_forward
             BEGIN SELECT RAISE(ABORT,'synthetic projection failure'); END;",
            )
            .unwrap();
        assert!(store.accept_forward_ack("11", send.client_id).is_err());
        assert_eq!(
            store.forward_outcome("11", send.client_id).unwrap(),
            ForwardOutcome::Acknowledged
        );
        store
            .conn
            .execute_batch("DROP TRIGGER reject_forward_projection")
            .unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.recover_forwards("11").unwrap();
        assert_eq!(
            reopened.forward_outcome("11", send.client_id).unwrap(),
            ForwardOutcome::Accepted
        );
        let mut after_ack = message("d:33", "301", "11", "999");
        after_ack.sent_at_ms = 102;
        let (stored, _) = receive(&mut reopened, after_ack, None, None).unwrap();
        let canonical = send.client_id.to_string();
        assert_eq!(
            stored[0].0.source_client_id.as_deref(),
            Some(canonical.as_str())
        );
        assert_eq!(stored[0].0.sent_at_ms, 102);
    }

    #[test]
    fn schema_thirteen_upgrade_keeps_messages_and_never_invents_forward_source_proof() {
        let (mut store, source) = fixture();
        store
            .receive_batch(
                &source.account_id,
                &[(source.clone(), None)],
                Some(("510_0", "99")),
            )
            .unwrap();
        store
            .conn
            .execute_batch(
                "DROP TABLE message_forward; DROP TABLE forward_outbox; PRAGMA user_version=13;",
            )
            .unwrap();
        let mut migrated = MessageStore::initialize(store.conn).unwrap();
        assert_eq!(
            migrated
                .conn
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            crate::store::SCHEMA_VERSION
        );
        assert_eq!(migrated.receive_cursor("11", "510_0").unwrap(), "99");
        migrated.replace_live_privacy("11", &[], None).unwrap();
        assert!(migrated
            .prepare_forward("11", "d:22", "100", "d:33", 1000)
            .is_err());
        receive(&mut migrated, source, Some(ForwardMarker::Ordinary), None).unwrap();
        assert!(migrated
            .prepare_forward("11", "d:22", "100", "d:33", 1000)
            .is_ok());
    }
}
