//! Per-conversation summary of the saved history: counts plus the newest pictures, files and links.
use super::MessageStore;
use crate::domain::{
    media::safe_url, validate_id, ConversationOverview, MediaDescriptor, SharedItem, SharedKind,
    MAX_MEDIA_DESCRIPTOR_BYTES, OVERVIEW_ITEM_LIMIT,
};
use anyhow::Result;
use rusqlite::params;

/// Text messages inspected for addresses, newest first. Bounds the work for very long histories.
const TEXT_SCAN_ROWS: usize = 400;
const MAX_TITLE_CHARS: usize = 120;

impl MessageStore {
    /// Counts and the newest items of the saved history. A thread that is hidden or whose privacy
    /// state is not verified yields an empty summary, exactly like [`MessageStore::page`].
    pub fn conversation_overview(
        &self,
        account: &str,
        thread: &str,
        limit: usize,
    ) -> Result<ConversationOverview> {
        validate_id(account)?;
        validate_id(thread)?;
        let limit = limit.clamp(1, OVERVIEW_ITEM_LIMIT);
        if !self.live_thread_visible(account, thread)? {
            return Ok(ConversationOverview::default());
        }
        let mut overview = ConversationOverview::default();
        let (total, first, last): (i64, Option<i64>, Option<i64>) = self.conn.query_row(
            "SELECT COUNT(*),MIN(sent_at_ms),MAX(sent_at_ms) FROM messages WHERE account_id=?1 AND thread_id=?2",
            params![account, thread],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        overview.total_messages = total.max(0) as u64;
        overview.first_at_ms = first;
        overview.last_at_ms = last;
        overview.sticker_count = self.conn.query_row(
            "SELECT COUNT(*) FROM message_content WHERE account_id=?1 AND thread_id=?2 AND kind='sticker'",
            params![account, thread],
            |row| row.get::<_, i64>(0),
        )? as u64;
        let mut card_links = 0u64;
        {
            let mut statement = self.conn.prepare_cached(
                "SELECT kind,COUNT(*) FROM message_media WHERE account_id=?1 AND thread_id=?2 GROUP BY kind",
            )?;
            let rows = statement.query_map(params![account, thread], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (kind, count) = row?;
                let count = count.max(0) as u64;
                match kind.as_str() {
                    "chat.photo" => overview.image_count = count,
                    "share.file" => overview.file_count = count,
                    "chat.recommended" | "chat.webcontent" => card_links += count,
                    _ => {}
                }
            }
        }
        let text_links: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id=?1 AND thread_id=?2 AND (body LIKE '%http://%' OR body LIKE '%https://%')",
            params![account, thread],
            |row| row.get(0),
        )?;
        overview.link_count = card_links + text_links.max(0) as u64;

        overview.images = self.media_items(account, thread, &["chat.photo"], limit)?;
        overview.files = self.media_items(account, thread, &["share.file"], limit)?;
        let mut links = self.media_items(
            account,
            thread,
            &["chat.recommended", "chat.webcontent"],
            limit,
        )?;
        links.extend(self.text_links(account, thread, limit)?);
        links.sort_by(|a, b| {
            b.sent_at_ms
                .cmp(&a.sent_at_ms)
                .then_with(|| b.message_id.cmp(&a.message_id))
        });
        links.dedup_by(|a, b| a.url.is_some() && a.url == b.url);
        links.truncate(limit);
        overview.links = links;
        Ok(overview)
    }

    fn media_items(
        &self,
        account: &str,
        thread: &str,
        kinds: &[&str],
        limit: usize,
    ) -> Result<Vec<SharedItem>> {
        // The kinds are fixed literals chosen by this module, never user input.
        let list = kinds
            .iter()
            .map(|kind| format!("'{kind}'"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT m.message_id,m.sender,m.sent_at_ms,
                CASE WHEN length(CAST(mm.projection AS BLOB))<=?3 THEN mm.projection END
             FROM message_media mm JOIN messages m
               ON m.account_id=mm.account_id AND m.thread_id=mm.thread_id AND m.message_id=mm.message_id
             WHERE mm.account_id=?1 AND mm.thread_id=?2 AND mm.kind IN ({list})
             ORDER BY m.sent_at_ms DESC,m.message_id DESC LIMIT ?4"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(
            params![
                account,
                thread,
                MAX_MEDIA_DESCRIPTOR_BYTES as i64,
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )?;
        let mut items = Vec::new();
        for row in rows {
            let (message_id, sender, sent_at_ms, projection) = row?;
            // A row that cannot be read back is skipped, not guessed.
            let Some(descriptor) = projection
                .and_then(|json| serde_json::from_str::<MediaDescriptor>(&json).ok())
                .filter(|descriptor| descriptor.validate().is_ok())
            else {
                continue;
            };
            let base = SharedItem {
                message_id,
                sender,
                sent_at_ms,
                kind: SharedKind::Link,
                title: String::new(),
                extension: String::new(),
                size_bytes: None,
                host: String::new(),
                url: None,
                thumbnail_url: None,
            };
            match descriptor {
                MediaDescriptor::Photo(photo) => items.push(SharedItem {
                    kind: SharedKind::Image,
                    title: short(if photo.caption.is_empty() {
                        &photo.title
                    } else {
                        &photo.caption
                    }),
                    url: photo
                        .original_url
                        .as_deref()
                        .or(photo.thumbnail_url.as_deref())
                        .and_then(safe_url),
                    thumbnail_url: photo.thumbnail_url.as_deref().and_then(safe_url),
                    ..base
                }),
                MediaDescriptor::File(file) => items.push(SharedItem {
                    kind: SharedKind::File,
                    title: short(&file.title),
                    extension: file.extension.clone(),
                    size_bytes: file.size_bytes,
                    url: file
                        .url
                        .as_deref()
                        .filter(|_| !file.encrypted)
                        .and_then(safe_url),
                    ..base
                }),
                MediaDescriptor::Recommended(card) | MediaDescriptor::WebContent(card) => {
                    let url = card
                        .url
                        .as_deref()
                        .or_else(|| card.buttons.iter().find_map(|b| b.url.as_deref()))
                        .and_then(safe_url);
                    items.push(SharedItem {
                        kind: SharedKind::Link,
                        title: short(&card.title),
                        host: url.as_deref().map(host_of).unwrap_or_default(),
                        thumbnail_url: card.image_url.as_deref().and_then(safe_url),
                        url,
                        ..base
                    });
                }
                MediaDescriptor::Poll(_) => {}
            }
        }
        Ok(items)
    }

    fn text_links(&self, account: &str, thread: &str, limit: usize) -> Result<Vec<SharedItem>> {
        let mut statement = self.conn.prepare_cached(
            "SELECT message_id,sender,sent_at_ms,body FROM messages
             WHERE account_id=?1 AND thread_id=?2 AND (body LIKE '%http://%' OR body LIKE '%https://%')
             ORDER BY sent_at_ms DESC,message_id DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(params![account, thread, TEXT_SCAN_ROWS as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut items: Vec<SharedItem> = Vec::new();
        'rows: for row in rows {
            let (message_id, sender, sent_at_ms, body) = row?;
            for url in addresses(&body) {
                if items
                    .iter()
                    .any(|item| item.url.as_deref() == Some(url.as_str()))
                {
                    continue;
                }
                items.push(SharedItem {
                    message_id: message_id.clone(),
                    sender: sender.clone(),
                    sent_at_ms,
                    kind: SharedKind::Link,
                    title: String::new(),
                    extension: String::new(),
                    size_bytes: None,
                    host: host_of(&url),
                    url: Some(url),
                    thumbnail_url: None,
                });
                if items.len() >= limit {
                    break 'rows;
                }
            }
        }
        Ok(items)
    }
}

/// Addresses written in plain text: tokens that start with `http://` or `https://`, stripped of
/// trailing sentence punctuation and checked by the same validator as every opened link.
pub(crate) fn addresses(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'')) {
        let lower = token.to_ascii_lowercase();
        let Some(start) = lower.find("http://").or_else(|| lower.find("https://")) else {
            continue;
        };
        let candidate = token[start..].trim_end_matches(|c: char| {
            matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}')
        });
        if let Some(url) = safe_url(candidate) {
            found.push(url);
        }
    }
    found
}
fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_default()
}
fn short(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= MAX_TITLE_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_TITLE_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MediaKind, MediaMessageContent, Message};
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
    fn put_text(store: &mut MessageStore, id: &str, text: &str, at: i64) {
        store
            .receive_batch_with_media("11", &[(message(id, text, at), None, None, None)], None)
            .unwrap();
    }
    fn put_media(
        store: &mut MessageStore,
        id: &str,
        kind: MediaKind,
        content: serde_json::Value,
        at: i64,
    ) {
        let content = MediaMessageContent::new(kind, &content).unwrap();
        let mut m = message(
            id,
            &format!("[Tin nhắn {} chưa được hỗ trợ]", kind.as_str()),
            at,
        );
        m.media = Some(content.descriptor().clone());
        store
            .receive_batch_with_media("11", &[(m, None, None, Some(content))], None)
            .unwrap();
    }
    fn history() -> MessageStore {
        let mut store = MessageStore::in_memory().unwrap();
        put_text(&mut store, "t1", "Xem https://example.com/doc, nhé.", 100);
        put_text(&mut store, "t2", "không có địa chỉ nào ở đây", 101);
        put_media(
            &mut store,
            "p1",
            MediaKind::Photo,
            json!({
            "title":"Ảnh A","description":"Chú thích A",
            "href":"https://photo-talk.zadn.vn/a.jpg","thumb":"https://photo-talk.zadn.vn/a_t.jpg"}),
            102,
        );
        put_media(
            &mut store,
            "p2",
            MediaKind::Photo,
            json!({
            "title":"Ảnh B","href":"https://photo-talk.zadn.vn/b.jpg"}),
            103,
        );
        put_media(
            &mut store,
            "f1",
            MediaKind::File,
            json!({
            "title":"bao-cao","params":"{\"fileExt\":\"pdf\",\"fileSize\":\"2048\"}",
            "href":"https://files.example.com/bao-cao.pdf"}),
            104,
        );
        put_media(
            &mut store,
            "r1",
            MediaKind::Recommended,
            json!({
            "action":"recommened.link","title":"Bài viết hay","href":"https://news.example.com/a"}),
            105,
        );
        put_text(&mut store, "t3", "lại https://example.com/doc lần nữa", 106);
        put_text(
            &mut store,
            "t4",
            "ftp://bad.example.com và http://user:pw@evil.example.com/x",
            107,
        );
        store
    }

    #[test]
    fn counts_and_newest_first_items_cover_pictures_files_and_links() {
        let store = history();
        let overview = store.conversation_overview("11", "d:22", 50).unwrap();
        assert_eq!(overview.total_messages, 8);
        assert_eq!(
            (overview.first_at_ms, overview.last_at_ms),
            (Some(100), Some(107))
        );
        assert_eq!((overview.image_count, overview.file_count), (2, 1));
        // Messages that mention an address (t1, t3, t4) plus one link card.
        assert_eq!(overview.link_count, 4);

        let images: Vec<_> = overview
            .images
            .iter()
            .map(|i| i.message_id.as_str())
            .collect();
        assert_eq!(images, ["p2", "p1"], "newest first");
        assert_eq!(
            overview.images[1].title, "Chú thích A",
            "caption wins over the title"
        );
        assert_eq!(overview.images[1].kind, SharedKind::Image);
        assert_eq!(
            overview.images[1].thumbnail_url.as_deref(),
            Some("https://photo-talk.zadn.vn/a_t.jpg")
        );

        assert_eq!(overview.files.len(), 1);
        let file = &overview.files[0];
        assert_eq!(
            (file.extension.as_str(), file.size_bytes),
            ("pdf", Some(2048))
        );
        assert_eq!(
            file.url.as_deref(),
            Some("https://files.example.com/bao-cao.pdf")
        );
        assert!(file.title.starts_with("bao-cao"));

        // The card and one deduplicated text address, newest first. The repeated address is
        // reported once, at its newest message; unsafe schemes and credentials are dropped.
        let links: Vec<_> = overview
            .links
            .iter()
            .map(|l| {
                (
                    l.message_id.as_str(),
                    l.url.as_deref().unwrap(),
                    l.host.as_str(),
                )
            })
            .collect();
        assert_eq!(
            links,
            [
                ("t3", "https://example.com/doc", "example.com"),
                ("r1", "https://news.example.com/a", "news.example.com"),
            ]
        );
        assert_eq!(overview.links[1].title, "Bài viết hay");
    }

    #[test]
    fn item_lists_respect_the_limit_and_other_threads_do_not_leak_in() {
        let mut store = history();
        for n in 0..70 {
            put_media(
                &mut store,
                &format!("x{n}"),
                MediaKind::Photo,
                json!({
                "title":"x","href":"https://photo-talk.zadn.vn/x.jpg"}),
                200 + n,
            );
        }
        let overview = store.conversation_overview("11", "d:22", 500).unwrap();
        assert_eq!(overview.image_count, 72);
        assert_eq!(overview.images.len(), OVERVIEW_ITEM_LIMIT);
        assert_eq!(overview.images[0].message_id, "x69");
        let other = store.conversation_overview("11", "d:99", 50).unwrap();
        assert_eq!(other, ConversationOverview::default());
        let wrong_account = store.conversation_overview("12", "d:22", 50).unwrap();
        assert_eq!(wrong_account, ConversationOverview::default());
    }

    #[test]
    fn hidden_or_unverified_conversations_reveal_nothing() {
        let mut store = history();
        store.begin_live_privacy("11").unwrap();
        assert_eq!(
            store.conversation_overview("11", "d:22", 50).unwrap(),
            ConversationOverview::default(),
            "an unverified privacy state must not leak counts or items"
        );
        store
            .replace_live_privacy("11", &["d:22".to_owned()], None)
            .unwrap();
        assert_eq!(
            store.conversation_overview("11", "d:22", 50).unwrap(),
            ConversationOverview::default()
        );
        store
            .replace_live_privacy("11", &["d:22".to_owned()], Some("d:22"))
            .unwrap();
        assert_eq!(
            store
                .conversation_overview("11", "d:22", 50)
                .unwrap()
                .total_messages,
            8
        );
    }

    #[test]
    fn plain_text_addresses_are_trimmed_validated_and_bounded() {
        assert_eq!(
            addresses("a (https://example.com/x). b, HTTP://Example.org/Y; c https://e.com/z?q=1!"),
            [
                "https://example.com/x",
                "http://example.org/Y",
                "https://e.com/z?q=1"
            ]
        );
        assert!(addresses("javascript:alert(1) file:///etc/passwd httpx://no").is_empty());
        assert!(addresses("https://user:pw@example.com/").is_empty());
        assert!(addresses(&format!("https://example.com/{}", "a".repeat(9000))).is_empty());
    }
}
