//! Search within one conversation's saved history. Matching is literal and Vietnamese
//! accent/case-insensitive (see [`fold_search`](super::fold_search)); it reads stored rows only
//! and never asks the network, so it finds what this device has saved and nothing more.
use super::Cursor;

/// Results carried per request.
pub const SEARCH_RESULT_LIMIT: usize = 50;
/// Longest accepted query, in characters after trimming.
pub const MAX_SEARCH_QUERY_CHARS: usize = 120;
/// Saved messages inspected per request, newest first. Bounds the work on very long histories;
/// a search that stops here says so instead of claiming there is nothing older.
pub const SEARCH_SCAN_ROWS: usize = 20_000;
/// Characters of the message shown around the match.
pub const SNIPPET_CHARS: usize = 120;
const SNIPPET_LEAD_CHARS: usize = 36;

/// One saved message that contains the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHit {
    pub message_id: String,
    pub sender: String,
    pub sent_at_ms: i64,
    /// Part of the message around the first match, single-line, with `…` where it was cut.
    pub snippet: String,
    /// Byte range of the match inside `snippet`.
    pub highlight: (usize, usize),
    /// Page cursor that loads a page containing this message with some newer context after
    /// it; `None` means the newest page already contains it.
    pub jump: Option<Cursor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageSearch {
    /// The query exactly as the user typed it (trimmed).
    pub query: String,
    /// Newest first, at most [`SEARCH_RESULT_LIMIT`].
    pub hits: Vec<MessageHit>,
    /// More matches may exist: the result limit or the scan bound was reached.
    pub truncated: bool,
}

/// The trimmed query, or `None` when it is empty, too long or has nothing searchable.
pub fn search_query(query: &str) -> Option<&str> {
    let query = query.trim();
    (!query.is_empty()
        && query.chars().count() <= MAX_SEARCH_QUERY_CHARS
        && !query.chars().any(char::is_control)
        && !super::fold_search(query).is_empty())
    .then_some(query)
}

/// Folds `text` like [`fold_search`](super::fold_search) but keeps, for every folded char, the
/// index of the original char it came from, so a match can be mapped back to the original.
fn fold_indexed(text: &str) -> (Vec<char>, Vec<usize>) {
    let mut folded = Vec::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        if ch.is_whitespace() {
            folded.push(' ');
            origin.push(index);
            continue;
        }
        let mut buffer = [0u8; 4];
        for out in super::fold_search(ch.encode_utf8(&mut buffer)).chars() {
            folded.push(out);
            origin.push(index);
        }
    }
    (folded, origin)
}

fn fold_query(query: &str) -> Vec<char> {
    fold_indexed(query.trim()).0
}

/// Original char range of the first match of the folded `needle` in `text`.
fn find(text: &str, needle: &[char]) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let (folded, origin) = fold_indexed(text);
    let start = folded.windows(needle.len()).position(|w| w == needle)?;
    let mut end = origin[start + needle.len() - 1] + 1;
    // Keep combining marks with the letter they belong to.
    end += text
        .chars()
        .skip(end)
        .take_while(|ch| ('\u{0300}'..='\u{036f}').contains(ch))
        .count();
    Some((origin[start], end))
}

/// Matches messages against one query; built once per request.
pub struct Matcher {
    needle: Vec<char>,
}

impl Matcher {
    pub fn new(query: &str) -> Option<Self> {
        let needle = fold_query(search_query(query)?);
        (!needle.is_empty()).then_some(Self { needle })
    }

    /// The snippet and highlight for `text`, or `None` when it does not contain the query.
    pub fn snippet(&self, text: &str) -> Option<(String, (usize, usize))> {
        let (start, end) = find(text, &self.needle)?;
        let chars: Vec<char> = text
            .chars()
            .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
            .collect();
        let lead = SNIPPET_LEAD_CHARS.min(SNIPPET_CHARS.saturating_sub(end - start));
        let from = start.saturating_sub(lead);
        let to = (from + SNIPPET_CHARS).min(chars.len());
        let mut snippet = String::new();
        if from > 0 {
            snippet.push('…');
        }
        let mut highlight = (0, 0);
        for (index, ch) in chars.iter().enumerate().take(to).skip(from) {
            if index == start {
                highlight.0 = snippet.len();
            }
            snippet.push(*ch);
            if index + 1 == end.min(to) {
                highlight.1 = snippet.len();
            }
        }
        if highlight.1 < highlight.0 {
            highlight.1 = snippet.len();
        }
        if to < chars.len() {
            snippet.push('…');
        }
        Some((snippet, highlight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_ignores_accents_case_and_whitespace_kind() {
        let matcher = Matcher::new("  DUONG pho ").unwrap();
        let (snippet, (a, b)) = matcher.snippet("Hẹn ở đường\tphố Huế nhé").unwrap();
        assert_eq!(snippet, "Hẹn ở đường phố Huế nhé");
        assert_eq!(&snippet[a..b], "đường phố");
        assert!(Matcher::new("đường").unwrap().snippet("DUONG").is_some());
        assert!(Matcher::new("pho").unwrap().snippet("Hẹn mai").is_none());
    }

    #[test]
    fn combining_marks_in_stored_text_still_match_and_highlight_whole_letters() {
        // "Việt" written with combining marks (NFD) as some clients send it.
        let text = "Tie\u{0302}\u{0301}ng Vie\u{0323}\u{0302}t";
        let matcher = Matcher::new("tiếng việt").unwrap();
        let (snippet, (a, b)) = matcher.snippet(text).unwrap();
        assert_eq!(&snippet[a..b], text);
        let (snippet, (a, b)) = Matcher::new("vie").unwrap().snippet(text).unwrap();
        assert_eq!(&snippet[a..b], "Vie\u{0323}\u{0302}");
    }

    #[test]
    fn long_messages_are_cut_around_the_match_on_char_boundaries() {
        let text = format!("{}điểm hẹn{}", "á".repeat(300), "ờ".repeat(300));
        let (snippet, (a, b)) = Matcher::new("diem hen").unwrap().snippet(&text).unwrap();
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert_eq!(&snippet[a..b], "điểm hẹn");
        assert!(snippet.chars().count() <= SNIPPET_CHARS + 2);
    }

    #[test]
    fn empty_overlong_or_control_queries_are_refused() {
        for query in ["", "   ", "\u{0301}", "a\u{7}b"] {
            assert!(search_query(query).is_none(), "{query:?}");
        }
        assert!(search_query(&"a".repeat(MAX_SEARCH_QUERY_CHARS)).is_some());
        assert!(search_query(&"a".repeat(MAX_SEARCH_QUERY_CHARS + 1)).is_none());
    }
}
