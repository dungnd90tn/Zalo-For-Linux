//! A typed group send intent and scoped history annotations.
use super::*;
use crate::domain::mention::{self, Mention};
use anyhow::ensure;

const MAX_PROJECTION_BYTES: usize = 4 * 1024;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_mention(
            account_id TEXT NOT NULL,thread_id TEXT NOT NULL,message_id TEXT NOT NULL,
            annotation TEXT NOT NULL CHECK(length(CAST(annotation AS BLOB)) BETWEEN 3 AND 4096),
            PRIMARY KEY(account_id,thread_id,message_id),
            FOREIGN KEY(account_id,thread_id,message_id)
                REFERENCES messages(account_id,thread_id,message_id) ON DELETE CASCADE
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS outgoing_mention(
            account_id TEXT NOT NULL,client_id TEXT NOT NULL,
            annotation TEXT NOT NULL CHECK(length(CAST(annotation AS BLOB)) BETWEEN 3 AND 4096),
            PRIMARY KEY(account_id,client_id),
            FOREIGN KEY(account_id,client_id)
                REFERENCES outgoing(account_id,client_id) ON DELETE CASCADE
         ) WITHOUT ROWID;",
    )?;
    Ok(())
}

fn encode(mentions: &[Mention]) -> Result<String> {
    let annotation = serde_json::to_string(mentions)?;
    ensure!(
        !mentions.is_empty() && annotation.len() <= MAX_PROJECTION_BYTES,
        FailureReason::Validation
    );
    Ok(annotation)
}

fn decode(annotation: Option<String>, text: &str) -> Result<Vec<Mention>> {
    let Some(annotation) = annotation else {
        return Ok(Vec::new());
    };
    ensure!(
        annotation.len() <= MAX_PROJECTION_BYTES,
        FailureReason::ConflictingMetadata
    );
    let mentions: Vec<Mention> =
        serde_json::from_str(&annotation).map_err(|_| FailureReason::ConflictingMetadata)?;
    ensure!(
        !mentions.is_empty() && mention::validate(text, &mentions).is_ok(),
        FailureReason::ConflictingMetadata
    );
    Ok(mentions)
}

pub(super) fn reserve(
    conn: &Connection,
    account: &str,
    client_id: u64,
    mentions: &[Mention],
) -> Result<()> {
    conn.execute(
        "INSERT INTO outgoing_mention(account_id,client_id,annotation) VALUES(?1,?2,?3)",
        params![account, client_id.to_string(), encode(mentions)?],
    )?;
    Ok(())
}

pub(super) fn outgoing(
    conn: &Connection,
    account: &str,
    client_id: &str,
    text: &str,
) -> Result<Vec<Mention>> {
    let annotation = conn
        .query_row(
            "SELECT annotation FROM outgoing_mention WHERE account_id=?1 AND client_id=?2",
            params![account, client_id],
            |row| row.get(0),
        )
        .optional()?;
    decode(annotation, text)
}

fn annotation(conn: &Connection, message: &Message) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT annotation FROM message_mention WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
        params![message.account_id, message.thread_id, message.id],
        |row| row.get(0),
    ).optional()?)
}

pub(super) fn persist(conn: &Connection, message: &Message) -> Result<bool> {
    if message.mentions.is_empty() {
        return Ok(false);
    }
    let previous = decode(annotation(conn, message)?, &message.text)?;
    if !previous.is_empty() {
        ensure!(
            previous == message.mentions,
            FailureReason::ConflictingMetadata
        );
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO message_mention(account_id,thread_id,message_id,annotation) VALUES(?1,?2,?3,?4)",
        params![message.account_id, message.thread_id, message.id, encode(&message.mentions)?],
    )?;
    Ok(true)
}

pub(super) fn check_existing_ack(conn: &Connection, message: &Message) -> Result<()> {
    if message.thread_id.starts_with("g:") {
        let existing = decode(annotation(conn, message)?, &message.text)?;
        ensure!(
            existing.is_empty() || existing == message.mentions,
            FailureReason::ConflictingEcho
        );
    }
    Ok(())
}

pub(super) fn hydrate(conn: &Connection, message: &mut Message) -> Result<()> {
    if message.reply_blocked && !message.recalled && message.thread_id.starts_with("g:") {
        message.mentions = decode(annotation(conn, message)?, &message.text)?;
    }
    Ok(())
}

/// A fully typed own echo can settle an uncertain send. A metadata-free echo
/// can restore known annotations only after an HTTP ACK binds the server ID.
pub(super) fn correlate(
    conn: &Connection,
    message: &mut Message,
    client_id: &str,
    body: &str,
    server_bound: bool,
) -> Result<bool> {
    if !message.thread_id.starts_with("g:") {
        return Ok(true);
    }
    let expected = outgoing(conn, &message.account_id, client_id, body)?;
    if expected != message.mentions {
        if expected.is_empty()
            || !message.mentions.is_empty()
            || !server_bound
            || message.reply_blocked
            || message.quote.is_some()
            || body != message.text
        {
            return Ok(false);
        }
        message.mentions = expected;
        message.reply_blocked = true;
    }
    Ok(message.mentions.is_empty() || (message.quote.is_none() && body == message.text))
}

#[cfg(all(test, feature = "messaging"))]
mod tests {
    use super::*;

    const TEXT: &str = "😀 @Ánh";

    fn alice() -> Mention {
        Mention {
            uid: "22".into(),
            pos: 3,
            len: 4,
        }
    }

    fn group_message(id: &str, client: u64, mentions: Vec<Mention>) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "g:33".into(),
            id: id.into(),
            sender: "11".into(),
            text: TEXT.into(),
            source_client_id: Some(client.to_string()),
            reply_blocked: !mentions.is_empty(),
            mentions,
            recalled: false,
            quote: None,
            sticker: None,
            media: None,
            sent_at_ms: 5_000,
        }
    }

    fn state(store: &MessageStore, client: u64) -> String {
        store
            .conn
            .query_row(
                "SELECT state FROM outgoing WHERE account_id='11' AND client_id=?1",
                [client.to_string()],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn unknown_mentions_require_a_typed_echo_and_ack_bindings_survive_restart() {
        let mut store = MessageStore::in_memory().unwrap();
        let client = store
            .prepare_outgoing_mention("11", "g:33", TEXT, &[alice()], 5_000)
            .unwrap();
        store.fail_outgoing("11", client, true).unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        let untyped = group_message("910", client, vec![]);
        reopened
            .receive_batch(
                "11",
                &[(untyped, Some(client.to_string()))],
                Some(("511_0", "80")),
            )
            .unwrap();
        assert_eq!(state(&reopened, client), "unknown");
        assert!(reopened
            .prepare_outgoing("11", "g:33", TEXT, 5_001)
            .is_err());
        let typed = group_message("910", client, vec![alice()]);
        reopened
            .receive_batch(
                "11",
                &[(typed.clone(), Some(client.to_string()))],
                Some(("511_0", "81")),
            )
            .unwrap();
        assert_eq!(state(&reopened, client), "accepted");
        assert_eq!(reopened.page("11", "g:33", None, 10).unwrap(), [typed]);
        assert_eq!(reopened.receive_cursor("11", "511_0").unwrap(), "81");
    }

    #[test]
    fn ordinary_echo_cannot_settle_another_group_member_intent() {
        let mut store = MessageStore::in_memory().unwrap();
        let client = store.prepare_outgoing("11", "g:33", TEXT, 5_000).unwrap();
        let typed = group_message("910", client, vec![alice()]);
        store
            .receive_batch("11", &[(typed, Some(client.to_string()))], None)
            .unwrap();
        assert_eq!(state(&store, client), "sending");
        assert!(store.accept_outgoing("11", client, "910", None).is_err());
    }

    #[test]
    fn version_eighteen_upgrade_preserves_unresolved_and_persists_new_annotation_ack() {
        let mut original = MessageStore::in_memory().unwrap();
        let pending = original
            .prepare_outgoing("11", "g:33", "Bản nháp gốc", 4_000)
            .unwrap();
        original.fail_outgoing("11", pending, true).unwrap();
        original
            .receive_batch("11", &[], Some(("511_0", "74")))
            .unwrap();
        original
            .conn
            .execute_batch(
                "DROP TABLE message_mention; DROP TABLE outgoing_mention; PRAGMA user_version=18;",
            )
            .unwrap();
        let mut store = MessageStore::initialize(original.conn).unwrap();
        assert_eq!(
            store
                .conn
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            19
        );
        assert_eq!(state(&store, pending), "unknown");
        assert_eq!(store.receive_cursor("11", "511_0").unwrap(), "74");
        let client = store
            .prepare_outgoing_mention("11", "g:33", TEXT, &[alice()], 5_000)
            .unwrap();
        assert!(client > pending);
        store.record_outgoing_ack("11", client, "910").unwrap();
        let mut reopened = MessageStore::initialize(store.conn).unwrap();
        reopened.reconcile_outgoing("11").unwrap();
        let messages = reopened.page("11", "g:33", None, 10).unwrap();
        assert_eq!(messages, [group_message("910", client, vec![alice()])]);
        assert!(messages[0].reply_reference().is_none());
        assert!(reopened.page("12", "g:33", None, 10).unwrap().is_empty());
    }
}
