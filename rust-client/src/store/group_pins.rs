//! Only unresolved mutations are persisted. The board snapshot is server-owned.
use super::*;

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS group_pin_intent (
            account_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            client_id TEXT NOT NULL,
            sender_id TEXT NOT NULL,
            pinned INTEGER NOT NULL CHECK(pinned IN (0,1)),
            board_version INTEGER NOT NULL CHECK(board_version BETWEEN 0 AND 9007199254740991),
            PRIMARY KEY(account_id,thread_id,message_id)
         ) WITHOUT ROWID;",
    )?;
    Ok(())
}

#[cfg(feature = "messaging")]
mod live {
    use super::*;
    use crate::{
        app::live::{GroupPinEntry, GroupPinSnapshot},
        domain::search::{MessageHit, SNIPPET_CHARS},
        protocol::group_pins::{PinReference, PinSnapshot},
    };
    use anyhow::ensure;

    const MAX_UNRESOLVED: usize = 100;

    pub(crate) struct PinSource {
        pub reference: PinReference,
        pub text: String,
        pub sender_name: String,
    }

    fn reference(message: &Message) -> Option<PinReference> {
        let source = message.reply_reference()?;
        if message.quote.is_some() || canonical_plain_text(&message.text).is_empty() {
            return None;
        }
        Some(PinReference {
            message_id: source.message_id,
            client_id: message.source_client_id.clone()?,
            sender_id: source.sender,
        })
    }

    impl MessageStore {
        pub(crate) fn group_pin_source(
            &self,
            account: &str,
            thread: &str,
            id: &str,
        ) -> Result<PinSource> {
            self.validate_group_scope(account, thread)?;
            ensure!(
                !self.group_write_blocked(account, thread)?,
                FailureReason::Validation
            );
            let message = self
                .refresh_messages(account, thread, &[id.to_owned()])?
                .into_iter()
                .next()
                .ok_or(FailureReason::Validation)?;
            let reference = reference(&message).ok_or(FailureReason::Validation)?;
            let forwarded: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM message_forward WHERE account_id=?1 AND thread_id=?2
                   AND message_id=?3 AND kind='forwarded')",
                params![account, thread, id],
                |row| row.get(0),
            )?;
            ensure!(!forwarded, FailureReason::Validation);
            let sender_name = self
                .sender_names(account, std::slice::from_ref(&message.sender))?
                .into_iter()
                .next()
                .map_or_else(String::new, |name| name.display_name);
            Ok(PinSource {
                reference,
                text: message.text,
                sender_name,
            })
        }

        pub(crate) fn reserve_group_pin(
            &self,
            account: &str,
            thread: &str,
            source: &PinSource,
            pinned: bool,
            board_version: u64,
        ) -> Result<bool> {
            let actual = self.group_pin_source(account, thread, &source.reference.message_id)?;
            ensure!(
                source.reference == actual.reference && source.text == actual.text,
                FailureReason::Validation
            );
            let pending: usize = self.conn.query_row(
                "SELECT COUNT(*) FROM group_pin_intent WHERE account_id=?1",
                [account],
                |row| row.get(0),
            )?;
            if pending >= MAX_UNRESOLVED {
                return Ok(false);
            }
            let reference = &source.reference;
            Ok(self.conn.execute(
                "INSERT INTO group_pin_intent(account_id,thread_id,message_id,client_id,sender_id,pinned,board_version)
                 VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT DO NOTHING",
                params![account, thread, reference.message_id, reference.client_id, reference.sender_id, pinned, board_version],
            )? == 1)
        }

        pub(crate) fn release_group_pin(
            &self,
            account: &str,
            thread: &str,
            reference: &PinReference,
            pinned: bool,
        ) -> Result<()> {
            self.conn.execute(
                "DELETE FROM group_pin_intent WHERE account_id=?1 AND thread_id=?2
                  AND message_id=?3 AND client_id=?4 AND sender_id=?5 AND pinned=?6",
                params![
                    account,
                    thread,
                    reference.message_id,
                    reference.client_id,
                    reference.sender_id,
                    pinned
                ],
            )?;
            Ok(())
        }

        /// The server proves an unresolved operation only by showing the
        /// requested final state, with the exact typed message identity.
        pub(crate) fn project_group_pins(
            &mut self,
            account: &str,
            thread: &str,
            snapshot: &PinSnapshot,
            can_mutate: bool,
        ) -> Result<GroupPinSnapshot> {
            self.validate_group_scope(account, thread)?;
            let mut entries = Vec::with_capacity(snapshot.items.len());
            let ids = snapshot
                .items
                .iter()
                .map(|entry| entry.reference.message_id.clone())
                .collect::<Vec<_>>();
            let mut by_id = std::collections::HashMap::new();
            for batch in ids.chunks(MAX_REFRESH_MESSAGES) {
                by_id.extend(
                    self.refresh_messages(account, thread, batch)?
                        .into_iter()
                        .map(|message| (message.id.clone(), message)),
                );
            }
            for item in &snapshot.items {
                let id = &item.reference.message_id;
                let hit = by_id
                    .get(id)
                    .filter(|message| reference(message).as_ref() == Some(&item.reference))
                    .map(|message| self.group_pin_hit(message))
                    .transpose()?;
                entries.push(GroupPinEntry {
                    message_id: id.clone(),
                    hit,
                });
            }

            let tx = self.conn.transaction()?;
            let pending = {
                let mut statement = tx.prepare_cached(
                    "SELECT message_id,client_id,sender_id,pinned,board_version FROM group_pin_intent
                     WHERE account_id=?1 AND thread_id=?2 ORDER BY message_id LIMIT ?3",
                )?;
                let rows =
                    statement.query_map(params![account, thread, MAX_UNRESOLVED], |row| {
                        Ok((
                            PinReference {
                                message_id: row.get(0)?,
                                client_id: row.get(1)?,
                                sender_id: row.get(2)?,
                            },
                            row.get::<_, bool>(3)?,
                            row.get::<_, u64>(4)?,
                        ))
                    })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            let mut unresolved = Vec::with_capacity(pending.len());
            for (reference, pinned, baseline) in pending {
                let actual = snapshot
                    .items
                    .iter()
                    .find(|item| item.reference.message_id == reference.message_id);
                let resolved = snapshot.version > baseline
                    && match actual {
                        Some(item) => pinned && item.reference == reference,
                        None => !pinned,
                    };
                if resolved {
                    tx.execute(
                        "DELETE FROM group_pin_intent WHERE account_id=?1 AND thread_id=?2 AND message_id=?3",
                        params![account, thread, reference.message_id],
                    )?;
                } else {
                    unresolved.push(reference.message_id);
                }
            }
            tx.commit()?;
            Ok(GroupPinSnapshot {
                can_mutate: can_mutate && !self.group_write_blocked(account, thread)?,
                entries,
                unresolved,
            })
        }

        fn group_pin_hit(&self, message: &Message) -> Result<MessageHit> {
            let mut chars = message
                .text
                .chars()
                .map(|char| if char.is_whitespace() { ' ' } else { char });
            let mut snippet: String = chars.by_ref().take(SNIPPET_CHARS).collect();
            if chars.next().is_some() {
                snippet.push('…');
            }
            Ok(MessageHit {
                message_id: message.id.clone(),
                sender: message.sender.clone(),
                sent_at_ms: message.sent_at_ms,
                snippet,
                highlight: (0, 0),
                jump: self.jump_cursor(
                    &message.account_id,
                    &message.thread_id,
                    message.sent_at_ms,
                    &message.id,
                )?,
            })
        }
    }
}

#[cfg(feature = "messaging")]
pub(crate) use live::PinSource;
