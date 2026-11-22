//! A typed ordinary sticker reservation, independent of text and attachment outboxes.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    let old_layout: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('sticker_outbox') WHERE name='sticker_id')
          AND NOT EXISTS(SELECT 1 FROM pragma_table_info('sticker_outbox') WHERE name='category_id')",
        [],
        |row| row.get(0),
    )?;
    if old_layout {
        conn.execute_batch("ALTER TABLE sticker_outbox RENAME TO sticker_outbox_v17;")?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sticker_outbox(
            account_id TEXT NOT NULL, client_id TEXT NOT NULL, thread_id TEXT NOT NULL,
            sticker_id INTEGER NOT NULL, category_id INTEGER NOT NULL, sticker_type INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL, server_id TEXT,
            echoed INTEGER NOT NULL DEFAULT 0 CHECK(echoed IN (0,1)),
            CHECK((sticker_id BETWEEN 1 AND 30 AND category_id=0 AND sticker_type=7)
                OR (sticker_id BETWEEN 1 AND 9007199254740991
                    AND category_id BETWEEN 1 AND 9007199254740991 AND sticker_type=3)),
            PRIMARY KEY(account_id,client_id));",
    )?;
    if old_layout {
        conn.execute_batch(
            "INSERT INTO sticker_outbox(account_id,client_id,thread_id,sticker_id,category_id,sticker_type,created_at_ms,server_id,echoed)
             SELECT account_id,client_id,thread_id,sticker_id,0,7,created_at_ms,server_id,echoed FROM sticker_outbox_v17;
             DROP TABLE sticker_outbox_v17;",
        )?;
    }
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS sticker_unconfirmed ON
            sticker_outbox(account_id,thread_id,sticker_id,category_id,sticker_type) WHERE server_id IS NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS sticker_server_id ON
            sticker_outbox(account_id,thread_id,server_id) WHERE server_id IS NOT NULL;",
    )?;
    Ok(())
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::{
        domain::{StickerDescriptor, LEGACY_STICKER_PLACEHOLDER, MAX_STICKER_ID},
        protocol::messaging::{numeric_id, SendAck, Target, ThreadKind},
    };
    use anyhow::ensure;

    pub(crate) enum Reservation {
        Ready(u64),
        Unconfirmed,
    }

    impl MessageStore {
        pub(crate) fn prepare_sticker(
            &mut self,
            account: &str,
            thread: &str,
            sticker: StickerDescriptor,
            now_ms: i64,
        ) -> Result<Reservation> {
            let target = Target::from_thread(thread)?;
            numeric_id(account)?;
            ensure!(
                sticker.can_send()
                    && (1..=MAX_STICKER_ID as i64).contains(&now_ms)
                    && !(target.kind == ThreadKind::Direct && target.remote_id == account),
                FailureReason::Validation
            );
            super::super::attachments::plain_visible(self, account, thread)?;
            let tx = self.conn.transaction()?;
            let unconfirmed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sticker_outbox WHERE account_id=?1 AND thread_id=?2
                   AND sticker_id=?3 AND category_id=?4 AND sticker_type=?5 AND server_id IS NULL)
                 OR (SELECT COUNT(*) FROM sticker_outbox WHERE account_id=?1 AND server_id IS NULL)>=100",
                params![account, thread, sticker.sticker_id, sticker.category_id, sticker.sticker_type],
                |row| row.get(0),
            )?;
            if unconfirmed {
                return Ok(Reservation::Unconfirmed);
            }
            let previous: Option<i64> = tx
                .query_row(
                    "SELECT last_id FROM outgoing_counters WHERE account_id=?1",
                    [account],
                    |row| row.get(0),
                )
                .optional()?;
            let next = now_ms.max(previous.unwrap_or_default().saturating_add(1));
            ensure!(next <= MAX_STICKER_ID as i64, FailureReason::Validation);
            tx.execute(
                "INSERT INTO outgoing_counters VALUES(?1,?2)
                 ON CONFLICT(account_id) DO UPDATE SET last_id=excluded.last_id",
                params![account, next],
            )?;
            tx.execute(
                "INSERT INTO sticker_outbox(account_id,client_id,thread_id,sticker_id,category_id,sticker_type,created_at_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![account, next.to_string(), thread, sticker.sticker_id, sticker.category_id, sticker.sticker_type, now_ms],
            )?;
            tx.commit()?;
            Ok(Reservation::Ready(next as u64))
        }

        /// Called only before dispatch or following an explicit server rejection.
        pub(crate) fn release_sticker(&self, account: &str, client: u64) -> Result<()> {
            let changed = self.conn.execute(
                "DELETE FROM sticker_outbox WHERE account_id=?1 AND client_id=?2 AND server_id IS NULL",
                params![account, client.to_string()],
            )?;
            ensure!(changed == 1, FailureReason::ConflictingEcho);
            Ok(())
        }

        pub(crate) fn accept_sticker(
            &mut self,
            account: &str,
            client: u64,
            ack: &SendAck,
        ) -> Result<(Message, Conversation)> {
            numeric_id(&ack.server_id)?;
            let tx = self.conn.transaction()?;
            let client = client.to_string();
            let row = by_client(&tx, account, &client)?.ok_or(FailureReason::ConflictingEcho)?;
            ensure!(
                row.server_id.as_ref().is_none_or(|id| id == &ack.server_id),
                FailureReason::ConflictingEcho
            );
            let mut message = Message {
                expires_at_ms: None,
                account_id: account.into(),
                thread_id: row.thread,
                id: ack.server_id.clone(),
                sender: account.into(),
                text: LEGACY_STICKER_PLACEHOLDER.into(),
                sent_at_ms: ack
                    .timestamp_ms
                    .filter(|&stamp| stamp > 0)
                    .unwrap_or(row.created_at_ms),
                sticker: Some(row.sticker),
                source_client_id: None,
                mentions: Vec::new(),
                reply_blocked: true,
                recalled: false,
                media: None,
                quote: None,
            };
            verify_binding(&tx, &message, &client)?;
            let previous = existing_stamp(&tx, &message)?;
            if let Some(stamp) = previous {
                message.sent_at_ms = stamp;
            }
            message.validate()?;
            insert_validated(&tx, std::slice::from_ref(&message))?;
            let conversation = tx.query_row(
                CONVERSATION_SQL,
                params![account, message.thread_id, MAX_PREVIEW_CHARS as i64],
                conversation_row,
            )?;
            tx.execute(
                "UPDATE sticker_outbox SET server_id=?3 WHERE account_id=?1 AND client_id=?2",
                params![account, client, ack.server_id],
            )?;
            tx.commit()?;
            Ok((message, conversation))
        }
    }

    struct Binding {
        client: String,
        thread: String,
        sticker: StickerDescriptor,
        created_at_ms: i64,
        server_id: Option<String>,
        echoed: bool,
    }

    fn binding(row: &rusqlite::Row<'_>) -> rusqlite::Result<Binding> {
        Ok(Binding {
            client: row.get(0)?,
            thread: row.get(1)?,
            sticker: StickerDescriptor {
                sticker_id: row.get(2)?,
                category_id: row.get(3)?,
                sticker_type: row.get(4)?,
            },
            created_at_ms: row.get(5)?,
            server_id: row.get(6)?,
            echoed: row.get(7)?,
        })
    }

    fn by_client(conn: &Connection, account: &str, client: &str) -> Result<Option<Binding>> {
        Ok(conn
            .query_row(
                "SELECT client_id,thread_id,sticker_id,category_id,sticker_type,created_at_ms,server_id,echoed
                 FROM sticker_outbox WHERE account_id=?1 AND client_id=?2",
                params![account, client],
                binding,
            )
            .optional()?)
    }

    fn verify_binding(conn: &Connection, message: &Message, client: &str) -> Result<()> {
        let available: bool = conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM outgoing WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM forward_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM attachment_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM photo_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3)
               AND NOT EXISTS(SELECT 1 FROM sticker_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3 AND client_id<>?4)
               AND NOT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2 AND message_id=?3 AND kind='forwarded')",
            params![message.account_id, message.thread_id, message.id, client],
            |row| row.get(0),
        )?;
        ensure!(available, FailureReason::ConflictingEcho);
        Ok(())
    }

    fn existing_stamp(conn: &Connection, message: &Message) -> Result<Option<i64>> {
        let previous: Option<(String, String, i64)> = conn
            .query_row(
                "SELECT sender,body,sent_at_ms FROM messages WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![message.account_id, message.thread_id, message.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((sender, text, stamp)) = previous {
            ensure!(
                sender == message.sender
                    && text == message.text
                    && message_content::load(conn, message)?
                        .is_none_or(|sticker| Some(sticker) == message.sticker),
                FailureReason::ConflictingEcho
            );
            Ok(Some(stamp))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn reconcile(
        conn: &Connection,
        message: &Message,
        client: Option<&str>,
    ) -> Result<bool> {
        let Some(sticker) = message.sticker.filter(|sticker| sticker.can_send()) else {
            return Ok(false);
        };
        if message.sender != message.account_id {
            return Ok(false);
        }
        let explicit = client
            .map(|client| by_client(conn, &message.account_id, client))
            .transpose()?
            .flatten();
        let bound = conn
            .query_row(
                "SELECT client_id,thread_id,sticker_id,category_id,sticker_type,created_at_ms,server_id,echoed
                 FROM sticker_outbox WHERE account_id=?1 AND thread_id=?2 AND server_id=?3",
                params![message.account_id, message.thread_id, message.id],
                binding,
            )
            .optional()?;
        if explicit
            .as_ref()
            .zip(bound.as_ref())
            .is_some_and(|(a, b)| a.client != b.client)
        {
            return Ok(false);
        }
        let Some(row) = explicit.or(bound) else {
            return Ok(false);
        };
        if row.thread != message.thread_id
            || row.sticker != sticker
            || row
                .server_id
                .as_ref()
                .is_some_and(|server| server != &message.id)
        {
            return Ok(false);
        }
        verify_binding(conn, message, &row.client)?;
        let existing = existing_stamp(conn, message)?;
        let retimed = row.server_id.is_some()
            && !row.echoed
            && existing.is_some_and(|stamp| stamp != message.sent_at_ms);
        if retimed {
            conn.execute(
                "UPDATE messages SET sent_at_ms=?4 WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                params![message.account_id, message.thread_id, message.id, message.sent_at_ms],
            )?;
        }
        conn.execute(
            "UPDATE sticker_outbox SET server_id=?3,echoed=1 WHERE account_id=?1 AND client_id=?2",
            params![message.account_id, row.client, message.id],
        )?;
        Ok(retimed)
    }
}

#[cfg(feature = "messaging")]
pub(super) use live::reconcile;
#[cfg(feature = "messaging")]
pub(crate) use live::Reservation;

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;
    use crate::{
        domain::{StickerDescriptor, LEGACY_STICKER_PLACEHOLDER, STICKER_PREVIEW},
        protocol::messaging::SendAck,
    };

    fn reserve(store: &mut MessageStore, sticker: StickerDescriptor) -> u64 {
        let Reservation::Ready(id) = store.prepare_sticker("11", "d:22", sticker, 5000).unwrap()
        else {
            panic!("expected a fresh sticker reservation")
        };
        id
    }

    fn echo(thread: &str, id: &str, sticker: StickerDescriptor, stamp: i64) -> Message {
        Message {
            expires_at_ms: None,
            account_id: "11".into(),
            thread_id: thread.into(),
            id: id.into(),
            sender: "11".into(),
            text: LEGACY_STICKER_PLACEHOLDER.into(),
            sticker: Some(sticker),
            sent_at_ms: stamp,
            source_client_id: None,
            mentions: Vec::new(),
            reply_blocked: true,
            recalled: false,
            media: None,
            quote: None,
        }
    }

    #[test]
    fn only_a_matching_typed_own_echo_can_release_an_uncertain_sticker() {
        let (mut store, sticker) = (
            MessageStore::in_memory().unwrap(),
            StickerDescriptor::builtin(8),
        );
        let old_text_id = store.prepare_outgoing("11", "d:22", "draft", 5000).unwrap();
        let id = reserve(&mut store, sticker);
        assert!(id > old_text_id);
        for other in [
            echo("g:22", "900", sticker, 5050),
            echo("d:22", "901", StickerDescriptor::builtin(9), 5051),
            Message {
                expires_at_ms: None,
                sender: "22".into(),
                ..echo("d:22", "902", sticker, 5052)
            },
        ] {
            store
                .receive_batch("11", &[(other, Some(id.to_string()))], None)
                .unwrap();
            assert!(matches!(
                store.prepare_sticker("11", "d:22", sticker, 5055).unwrap(),
                Reservation::Unconfirmed
            ));
        }
        let actual = echo("d:22", "903", sticker, 5060);
        store
            .receive_batch("11", &[(actual.clone(), Some(id.to_string()))], None)
            .unwrap();
        let ack = SendAck {
            server_id: actual.id.clone(),
            timestamp_ms: Some(5059),
        };
        let (accepted, conversation) = store.accept_sticker("11", id, &ack).unwrap();
        assert_eq!(
            accepted, actual,
            "a late HTTP ack must keep the authoritative echo stamp"
        );
        assert_eq!(conversation.last_text, STICKER_PREVIEW);
        let next = reserve(&mut store, sticker);
        assert!(next > id);
        let state: String = store
            .conn
            .query_row(
                "SELECT state FROM outgoing WHERE account_id=?1 AND client_id=?2",
                params!["11", old_text_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "sending");
    }

    #[test]
    fn authenticated_ack_before_echo_retimes_once_and_is_replay_safe() {
        let mut store = MessageStore::in_memory().unwrap();
        let sticker = StickerDescriptor::builtin(3);
        let id = reserve(&mut store, sticker);
        let ack = SendAck {
            server_id: "900".into(),
            timestamp_ms: None,
        };
        assert_eq!(
            store.accept_sticker("11", id, &ack).unwrap().0.sent_at_ms,
            5000
        );
        let actual = echo("d:22", "900", sticker, 5060);
        let payload = [(actual.clone(), None)];
        assert_eq!(
            store.receive_batch("11", &payload, None).unwrap()[0].0,
            actual
        );
        assert!(store
            .receive_batch("11", &payload, None)
            .unwrap()
            .is_empty());
        assert_eq!(store.page("11", "d:22", None, 2).unwrap(), [actual]);
    }

    #[test]
    fn schema_sixteen_upgrade_preserves_history_and_pending_sends() {
        let mut original = MessageStore::in_memory().unwrap();
        let sticker = StickerDescriptor::builtin(3);
        let historical = echo("d:22", "900", sticker, 5000);
        original
            .receive_batch("11", &[(historical.clone(), None)], Some(("510_0", "123")))
            .unwrap();
        let draft = original
            .prepare_outgoing("11", "d:22", "synthetic draft", 5001)
            .unwrap();
        original.fail_outgoing("11", draft, true).unwrap();
        original
            .conn
            .execute_batch(
                "DROP TABLE sticker_outbox; DROP TABLE reaction_events;
                 DROP TABLE pending_reaction_events; PRAGMA user_version=16;",
            )
            .unwrap();

        let mut upgraded = MessageStore::initialize(original.conn).unwrap();
        assert_eq!(
            upgraded
                .conn
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            crate::store::SCHEMA_VERSION
        );
        assert_eq!(upgraded.receive_cursor("11", "510_0").unwrap(), "123");
        assert_eq!(upgraded.page("11", "d:22", None, 2).unwrap(), [historical]);
        assert!(upgraded
            .prepare_outgoing("11", "d:22", "synthetic draft", 5002)
            .is_err());
        assert!(reserve(&mut upgraded, sticker) > draft);
        let new_tables: u32 = upgraded.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('sticker_outbox','reaction_events')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(new_tables, 2);
    }

    #[test]
    fn schema_seventeen_migration_preserves_the_existing_builtin_attempt_bindings() {
        let original = MessageStore::in_memory().unwrap();
        original.conn.execute_batch(
            "DROP TABLE sticker_outbox;
             CREATE TABLE sticker_outbox(
               account_id TEXT NOT NULL, client_id TEXT NOT NULL, thread_id TEXT NOT NULL,
               sticker_id INTEGER NOT NULL CHECK(sticker_id BETWEEN 1 AND 30),
               created_at_ms INTEGER NOT NULL, server_id TEXT,
               echoed INTEGER NOT NULL DEFAULT 0 CHECK(echoed IN (0,1)),
               PRIMARY KEY(account_id,client_id));
             CREATE UNIQUE INDEX sticker_unconfirmed ON sticker_outbox(account_id,thread_id,sticker_id)
               WHERE server_id IS NULL;
             CREATE UNIQUE INDEX sticker_server_id ON sticker_outbox(account_id,thread_id,server_id)
               WHERE server_id IS NOT NULL;
             INSERT INTO sticker_outbox VALUES('11','5001','d:22',3,5001,NULL,0);
             INSERT INTO outgoing_counters VALUES('11',5001);
             PRAGMA user_version=17;",
        ).unwrap();
        let mut upgraded = MessageStore::initialize(original.conn).unwrap();
        let builtin = StickerDescriptor::builtin(3);
        assert!(matches!(
            upgraded
                .prepare_sticker("11", "d:22", builtin, 5002)
                .unwrap(),
            Reservation::Unconfirmed
        ));
        let catalog = StickerDescriptor {
            sticker_id: 3,
            category_id: 10047,
            sticker_type: 3,
        };
        assert!(reserve(&mut upgraded, catalog) > 5001);
        let ack = SendAck {
            server_id: "903".into(),
            timestamp_ms: None,
        };
        let (message, _) = upgraded.accept_sticker("11", 5001, &ack).unwrap();
        assert_eq!(message.sticker, Some(builtin));
        assert_eq!(message.sent_at_ms, 5001);
        let version: i64 = upgraded
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, crate::store::SCHEMA_VERSION);
    }
}
