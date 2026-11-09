//! Literal search in one conversation's saved history.
use super::MessageStore;
use crate::domain::{
    search::{Matcher, SEARCH_SCAN_ROWS},
    validate_id, Cursor, MediaDescriptor, MessageHit, MessageSearch, MAX_MEDIA_DESCRIPTOR_BYTES,
    SEARCH_RESULT_LIMIT,
};
use anyhow::{bail, Result};
use rusqlite::params;

/// Newer messages kept below a hit when jumping to it, so it is seen with what followed.
const JUMP_CONTEXT: i64 = 10;

/// What a saved row is searched by: the readable text of a picture, file, card or poll, and
/// the body of everything else. Stickers have no text. The placeholder body that media rows
/// keep for older binaries is never matched.
fn searchable(body: String, sticker: bool, projection: Option<String>) -> Option<String> {
    if let Some(json) = projection {
        let descriptor = serde_json::from_str::<MediaDescriptor>(&json)
            .ok()
            .filter(|descriptor| descriptor.validate().is_ok())?;
        let text = match descriptor {
            MediaDescriptor::Photo(photo) => photo.caption,
            MediaDescriptor::File(file) => file.title,
            MediaDescriptor::Recommended(card) | MediaDescriptor::WebContent(card) => {
                format!("{}\n{}", card.title, card.description)
            }
            MediaDescriptor::Poll(poll) => format!("{}\n{}", poll.title, poll.question),
        };
        return (!text.trim().is_empty()).then_some(text);
    }
    (!sticker).then_some(body)
}

impl MessageStore {
    /// Saved messages of `thread` containing `query`, newest first. A hidden thread, or one
    /// whose privacy state is not verified yet, yields no hits, exactly like
    /// [`MessageStore::page`]. At most [`SEARCH_SCAN_ROWS`] rows are inspected.
    pub fn search_messages(
        &self,
        account: &str,
        thread: &str,
        query: &str,
        limit: usize,
    ) -> Result<MessageSearch> {
        self.search_messages_before(account, thread, query, limit, None)
    }

    /// Continue with an exclusive keyset boundary. The cursor follows inspected rows,
    /// so a page without matches can still advance past the scan budget.
    pub fn search_messages_before(
        &self,
        account: &str,
        thread: &str,
        query: &str,
        limit: usize,
        before: Option<&Cursor>,
    ) -> Result<MessageSearch> {
        validate_id(account)?;
        validate_id(thread)?;
        if !crate::domain::search::valid_search_cursor(before) {
            bail!("Invalid search cursor");
        }
        let Some(matcher) = Matcher::new(query) else {
            bail!("Invalid search query");
        };
        let limit = limit.clamp(1, SEARCH_RESULT_LIMIT);
        let mut search = MessageSearch {
            query: query.trim().to_owned(),
            ..MessageSearch::default()
        };
        if !self.live_thread_visible(account, thread)? {
            return Ok(search);
        }
        // Separate SQL shapes allow SQLite to seek directly to the continuation boundary.
        let boundary = if before.is_some() {
            "AND (m.sent_at_ms,m.message_id)<(?5,?6)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT m.message_id,m.sender,m.sent_at_ms,m.body,
                EXISTS(SELECT 1 FROM message_content c WHERE c.account_id=m.account_id AND c.thread_id=m.thread_id
                    AND c.message_id=m.message_id AND c.kind='sticker'),
                CASE WHEN mm.message_id IS NULL THEN NULL
                     WHEN length(CAST(mm.projection AS BLOB))<=?3 THEN mm.projection ELSE '' END
             FROM messages m LEFT JOIN message_media mm
               ON mm.account_id=m.account_id AND mm.thread_id=m.thread_id AND mm.message_id=m.message_id
             WHERE m.account_id=?1 AND m.thread_id=?2
             {boundary}
             ORDER BY m.sent_at_ms DESC,m.message_id DESC LIMIT ?4"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let mut rows = if let Some(cursor) = before {
            statement.query(params![
                account,
                thread,
                MAX_MEDIA_DESCRIPTOR_BYTES as i64,
                SEARCH_SCAN_ROWS as i64 + 1,
                cursor.sent_at_ms,
                cursor.id
            ])?
        } else {
            statement.query(params![
                account,
                thread,
                MAX_MEDIA_DESCRIPTOR_BYTES as i64,
                SEARCH_SCAN_ROWS as i64 + 1
            ])?
        };
        let mut scanned = 0usize;
        let mut last_scanned = None;
        while let Some(row) = rows.next()? {
            // A single lookahead proves there are older rows. Do not consume it:
            // the next request must include it regardless of whether it matches.
            if scanned == SEARCH_SCAN_ROWS || search.hits.len() == limit {
                search.next = last_scanned;
                search.truncated = true;
                break;
            }
            scanned += 1;
            last_scanned = Some(Cursor {
                sent_at_ms: row.get(2)?,
                id: row.get(0)?,
            });
            let Some(text) = searchable(row.get(3)?, row.get(4)?, row.get(5)?) else {
                continue;
            };
            let Some((snippet, highlight)) = matcher.snippet(&text) else {
                continue;
            };
            search.hits.push(MessageHit {
                message_id: row.get(0)?,
                sender: row.get(1)?,
                sent_at_ms: row.get(2)?,
                snippet,
                highlight,
                jump: None,
            });
        }
        drop(rows);
        for hit in &mut search.hits {
            hit.jump = self.jump_cursor(account, thread, hit.sent_at_ms, &hit.message_id)?;
        }
        Ok(search)
    }

    /// Cursor whose page shows the message with [`JUMP_CONTEXT`] newer ones after it, or `None`
    /// when there are not that many newer ones and the newest page already contains it.
    fn jump_cursor(
        &self,
        account: &str,
        thread: &str,
        sent_at_ms: i64,
        id: &str,
    ) -> Result<Option<Cursor>> {
        let mut statement = self.conn.prepare_cached(
            "SELECT sent_at_ms,message_id FROM messages
             WHERE account_id=?1 AND thread_id=?2 AND (sent_at_ms,message_id)>(?3,?4)
             ORDER BY sent_at_ms,message_id LIMIT 1 OFFSET ?5",
        )?;
        let mut rows = statement.query(params![account, thread, sent_at_ms, id, JUMP_CONTEXT])?;
        Ok(match rows.next()? {
            Some(row) => Some(Cursor {
                sent_at_ms: row.get(0)?,
                id: row.get(1)?,
            }),
            None => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MediaKind, MediaMessageContent, Message, StickerDescriptor};
    use serde_json::json;

    fn message(id: &str, text: &str, at: i64) -> Message {
        Message {
            account_id: "11".into(),
            thread_id: "d:22".into(),
            id: id.into(),
            sender: "22".into(),
            text: text.into(),
            sticker: None,
            media: None,
            sent_at_ms: at,
        }
    }
    fn put(store: &mut MessageStore, message: Message) {
        store
            .receive_batch_with_media("11", &[(message, None, None, None)], None)
            .unwrap();
    }
    fn put_media(store: &mut MessageStore, id: &str, kind: MediaKind, content: serde_json::Value) {
        let content = MediaMessageContent::new(kind, &content).unwrap();
        let mut m = message(
            id,
            &format!("[Tin nhắn {} chưa được hỗ trợ]", kind.as_str()),
            50,
        );
        m.media = Some(content.descriptor().clone());
        store
            .receive_batch_with_media("11", &[(m, None, None, Some(content))], None)
            .unwrap();
    }
    fn ids(search: &MessageSearch) -> Vec<&str> {
        search.hits.iter().map(|h| h.message_id.as_str()).collect()
    }

    #[test]
    fn finds_newest_first_without_accents_and_scoped_to_one_thread() {
        let mut store = MessageStore::in_memory().unwrap();
        put(&mut store, message("1", "Hẹn ở quán phở", 10));
        put(&mut store, message("2", "không liên quan", 11));
        put(&mut store, message("3", "PHO bò nhé", 12));
        let mut other = message("4", "phở thread khác", 13);
        other.thread_id = "d:33".into();
        put(&mut store, other);
        let search = store.search_messages("11", "d:22", " pho ", 50).unwrap();
        assert_eq!(search.query, "pho");
        assert_eq!(ids(&search), ["3", "1"]);
        assert!(!search.truncated);
        let first = &search.hits[0];
        assert_eq!(&first.snippet[first.highlight.0..first.highlight.1], "PHO");
        assert!(store
            .search_messages("12", "d:22", "pho", 50)
            .unwrap()
            .hits
            .is_empty());
        assert!(store.search_messages("11", "d:22", "  ", 50).is_err());
    }

    #[test]
    fn media_is_searched_by_its_readable_text_and_placeholders_or_stickers_never_match() {
        let mut store = MessageStore::in_memory().unwrap();
        put_media(
            &mut store,
            "f1",
            MediaKind::File,
            json!({"title":"Báo cáo quý","params":"{\"fileExt\":\"pdf\",\"fileSize\":\"2048\"}",
                "href":"https://files.example.com/a.pdf"}),
        );
        put_media(
            &mut store,
            "p1",
            MediaKind::Photo,
            json!({"title":"Ảnh","description":"Ảnh đi biển","href":"https://photo-talk.zadn.vn/a.jpg"}),
        );
        let mut sticker = message("s1", crate::domain::LEGACY_STICKER_PLACEHOLDER, 60);
        sticker.sticker = Some(StickerDescriptor {
            sticker_id: 1,
            category_id: 2,
            sticker_type: 7,
        });
        put(&mut store, sticker);
        assert_eq!(
            ids(&store.search_messages("11", "d:22", "bao cao", 50).unwrap()),
            ["f1"]
        );
        assert_eq!(
            ids(&store.search_messages("11", "d:22", "di bien", 50).unwrap()),
            ["p1"]
        );
        for placeholder in ["tin nhắn", "nhãn dán"] {
            assert!(store
                .search_messages("11", "d:22", placeholder, 50)
                .unwrap()
                .hits
                .is_empty());
        }
    }

    #[test]
    fn limit_marks_truncation_and_jump_cursors_show_the_hit_with_context() {
        let mut store = MessageStore::in_memory().unwrap();
        for i in 0..80 {
            let text = if i % 2 == 0 { "cuộc họp" } else { "khác" };
            put(&mut store, message(&format!("m{i:03}"), text, 1000 + i));
        }
        let search = store.search_messages("11", "d:22", "hop", 5).unwrap();
        assert_eq!(search.hits.len(), 5);
        assert!(search.truncated);
        // The newest hit (m078) has one newer message: the newest page already shows it.
        assert_eq!(search.hits[0].message_id, "m078");
        assert_eq!(search.hits[0].jump, None);
        let all = store.search_messages("11", "d:22", "hop", 50).unwrap();
        let oldest = all.hits.last().unwrap();
        assert_eq!(oldest.message_id, "m000");
        let cursor = oldest.jump.clone().unwrap();
        let page = store.page("11", "d:22", Some(&cursor), 50).unwrap();
        let position = page.iter().position(|m| m.id == "m000").unwrap();
        assert_eq!(page.len() - position, 11, "hit plus ten newer messages");
    }

    #[test]
    fn pagination_has_no_gaps_with_timestamp_ties_and_new_arrivals() {
        let mut store = MessageStore::in_memory().unwrap();
        let messages: Vec<_> = (0..125)
            .map(|i| {
                message(
                    &format!("m{i:03}"),
                    if i % 3 == 0 { "khác" } else { "hẹn" },
                    10,
                )
            })
            .collect();
        for chunk in messages.chunks(crate::domain::MAX_PAGE_SIZE) {
            store.insert_batch(chunk).unwrap();
        }
        let expected: Vec<_> = messages
            .iter()
            .rev()
            .filter(|m| m.text == "hẹn")
            .map(|m| m.id.clone())
            .collect();
        let mut page = store.search_messages("11", "d:22", "hen", 50).unwrap();
        assert_eq!(page.hits.len(), 50);
        let first = page.next.clone().unwrap();
        let mut found: Vec<_> = ids(&page).into_iter().map(str::to_owned).collect();
        put(&mut store, message("new", "hẹn mới", 11));
        page = store
            .search_messages_before("11", "d:22", "hen", 50, Some(&first))
            .unwrap();
        found.extend(ids(&page).into_iter().map(str::to_owned));
        assert_eq!(found, expected);
        assert!(!page.truncated);
        assert!(page.next.is_none());
        // Replaying the same boundary is stable, even after a newer message arrives.
        assert_eq!(
            page,
            store
                .search_messages_before("11", "d:22", "hen", 50, Some(&first))
                .unwrap()
        );
        assert_eq!(
            ids(&store.search_messages("11", "d:22", "hen", 1).unwrap()),
            ["new"]
        );
    }

    #[test]
    fn scan_budget_can_continue_past_an_empty_page_and_exact_end_is_not_truncated() {
        let mut store = MessageStore::in_memory().unwrap();
        let messages: Vec<_> = (0..=SEARCH_SCAN_ROWS)
            .map(|i| {
                message(
                    &format!("m{i:05}"),
                    if i == 0 { "đích" } else { "khác" },
                    i as i64,
                )
            })
            .collect();
        for chunk in messages.chunks(crate::domain::MAX_PAGE_SIZE) {
            store.insert_batch(chunk).unwrap();
        }
        let page = store.search_messages("11", "d:22", "dich", 50).unwrap();
        assert!(page.hits.is_empty());
        assert!(page.truncated);
        assert_eq!(page.next.as_ref().unwrap().id, "m00001");
        let last = store
            .search_messages_before("11", "d:22", "dich", 50, page.next.as_ref())
            .unwrap();
        assert_eq!(ids(&last), ["m00000"]);
        assert!(last.next.is_none());
        let exact = store
            .search_messages_before(
                "11",
                "d:22",
                "absent",
                50,
                Some(&Cursor {
                    sent_at_ms: SEARCH_SCAN_ROWS as i64,
                    id: format!("m{SEARCH_SCAN_ROWS:05}"),
                }),
            )
            .unwrap();
        assert!(exact.hits.is_empty());
        assert!(
            !exact.truncated,
            "exactly the scan budget with no older rows is exhausted"
        );
        assert!(exact.next.is_none());
    }

    #[test]
    fn continuation_rechecks_privacy_and_validates_boundaries() {
        let mut store = MessageStore::in_memory().unwrap();
        for i in 0..3 {
            put(&mut store, message(&i.to_string(), "bí mật", i));
        }
        let page = store.search_messages("11", "d:22", "bi mat", 1).unwrap();
        let boundary = page.next.as_ref().unwrap();
        store.begin_live_privacy("11").unwrap();
        for unlocked in [false, true, false] {
            store
                .replace_live_privacy("11", &["d:22".into()], unlocked.then_some("d:22"))
                .unwrap();
            let page = store
                .search_messages_before("11", "d:22", "bi mat", 1, Some(boundary))
                .unwrap();
            assert_eq!(page.hits.len(), usize::from(unlocked));
            if !unlocked {
                assert!(page.next.is_none());
            }
        }
        for cursor in [
            Cursor {
                sent_at_ms: -1,
                id: "ok".into(),
            },
            Cursor {
                sent_at_ms: 1,
                id: "".into(),
            },
        ] {
            assert!(store
                .search_messages_before("11", "d:22", "bi mat", 1, Some(&cursor))
                .is_err());
        }
    }

    #[test]
    fn hidden_or_unverified_conversations_reveal_nothing() {
        let mut store = MessageStore::in_memory().unwrap();
        put(&mut store, message("1", "bí mật", 10));
        store.begin_live_privacy("11").unwrap();
        assert!(store
            .search_messages("11", "d:22", "bi mat", 50)
            .unwrap()
            .hits
            .is_empty());
        store
            .replace_live_privacy("11", &["d:22".to_owned()], None)
            .unwrap();
        assert!(store
            .search_messages("11", "d:22", "bi mat", 50)
            .unwrap()
            .hits
            .is_empty());
        store
            .replace_live_privacy("11", &["d:22".to_owned()], Some("d:22"))
            .unwrap();
        assert_eq!(
            store
                .search_messages("11", "d:22", "bi mat", 50)
                .unwrap()
                .hits
                .len(),
            1
        );
    }
}
