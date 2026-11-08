//! UI-side state of a search in the selected conversation. Results belong to one thread and one
//! request number; a reply for another thread, or an older request, is ignored. Nothing here
//! touches storage: requests go through the bounded worker queue like every other read.
use super::{Model, SubmitError};
use crate::domain::{search::search_query, Cursor, MessageHit, MessageSearch};
use std::time::{Duration, Instant};

/// How long a message reached from a search stays highlighted.
pub const FOCUS_HIGHLIGHT: Duration = Duration::from_millis(2400);

#[derive(Default)]
pub(crate) struct SearchState {
    thread: Option<String>,
    request: u64,
    pending: bool,
    failed: bool,
    results: Option<MessageSearch>,
    focus: Option<Focus>,
}

/// A message the timeline should scroll to and highlight once it is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub thread: String,
    pub message_id: String,
    /// Set when the timeline first shows the message; the highlight fades from then.
    pub shown_at: Option<Instant>,
}

impl Model {
    /// Results for `thread`, if the latest request for it has completed.
    pub fn search_results(&self, thread: &str) -> Option<&MessageSearch> {
        (self.search.thread.as_deref() == Some(thread))
            .then_some(self.search.results.as_ref())
            .flatten()
    }
    pub fn search_pending(&self, thread: &str) -> bool {
        self.search.pending && self.search.thread.as_deref() == Some(thread)
    }
    pub fn search_failed(&self, thread: &str) -> bool {
        self.search.failed && self.search.thread.as_deref() == Some(thread)
    }
    /// Ask for `query` in `thread`. `enqueue` receives the request number that the reply must
    /// carry; state changes only when the worker accepted the request.
    pub fn request_search(
        &mut self,
        thread: &str,
        query: &str,
        enqueue: impl FnOnce(u64, String) -> std::result::Result<(), SubmitError>,
    ) -> std::result::Result<(), SubmitError> {
        let query = search_query(query).ok_or(SubmitError::Invalid)?.to_owned();
        let request = self.search.request.wrapping_add(1);
        enqueue(request, query)?;
        if self.search.thread.as_deref() != Some(thread) {
            self.search.results = None;
        }
        self.search.thread = Some(thread.to_owned());
        self.search.request = request;
        self.search.pending = true;
        self.search.failed = false;
        Ok(())
    }
    pub fn apply_search(&mut self, request: u64, thread: &str, search: MessageSearch) -> bool {
        if request != self.search.request || self.search.thread.as_deref() != Some(thread) {
            return false;
        }
        self.search.results = Some(search);
        self.search.pending = false;
        self.search.failed = false;
        true
    }
    pub fn apply_search_failed(&mut self, request: u64, thread: &str) -> bool {
        if request != self.search.request || self.search.thread.as_deref() != Some(thread) {
            return false;
        }
        self.search.pending = false;
        self.search.failed = true;
        true
    }
    /// Forget results and any pending reply (a later reply no longer matches).
    pub fn clear_search(&mut self) {
        self.search.request = self.search.request.wrapping_add(1);
        self.search.thread = None;
        self.search.results = None;
        self.search.pending = false;
        self.search.failed = false;
    }
    /// Whether `hit` is in the loaded page, so no reload is needed to show it.
    pub fn hit_loaded(&self, hit: &MessageHit) -> bool {
        self.messages.iter().any(|m| m.id == hit.message_id)
    }
    /// The page to load to show `hit` (`None` = the newest page) and remember it as the focus.
    pub fn focus_hit(&mut self, thread: &str, hit: &MessageHit) -> Option<Cursor> {
        self.search.focus = Some(Focus {
            thread: thread.to_owned(),
            message_id: hit.message_id.clone(),
            shown_at: None,
        });
        hit.jump.clone()
    }
    /// The focused message of the selected thread, if any.
    pub fn focus(&self) -> Option<&Focus> {
        self.search
            .focus
            .as_ref()
            .filter(|focus| self.selected.as_deref() == Some(focus.thread.as_str()))
    }
    /// Called when the timeline draws the focused message. Returns `true` the first time, when
    /// the timeline should scroll to it; the focus is dropped once the highlight has faded.
    pub fn focus_shown(&mut self, now: Instant) -> bool {
        let Some(focus) = self.search.focus.as_mut() else {
            return false;
        };
        match focus.shown_at {
            None => {
                focus.shown_at = Some(now);
                true
            }
            Some(at) => {
                if now.duration_since(at) >= FOCUS_HIGHLIGHT {
                    self.search.focus = None;
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, jump: Option<Cursor>) -> MessageHit {
        MessageHit {
            message_id: id.into(),
            sender: "22".into(),
            sent_at_ms: 1,
            snippet: "x".into(),
            highlight: (0, 1),
            jump,
        }
    }
    fn results(query: &str) -> MessageSearch {
        MessageSearch {
            query: query.into(),
            hits: vec![hit("1", None)],
            truncated: false,
        }
    }

    #[test]
    fn replies_for_older_requests_or_other_threads_are_ignored() {
        let mut model = Model::default();
        let mut first = 0;
        model
            .request_search("a", " phở ", |request, query| {
                assert_eq!(query, "phở");
                first = request;
                Ok(())
            })
            .unwrap();
        assert!(model.search_pending("a"));
        let mut second = 0;
        model
            .request_search("a", "bún", |request, _| {
                second = request;
                Ok(())
            })
            .unwrap();
        assert!(!model.apply_search(first, "a", results("phở")));
        assert!(!model.apply_search(second, "b", results("bún")));
        assert!(model.search_results("a").is_none());
        assert!(model.apply_search(second, "a", results("bún")));
        assert_eq!(model.search_results("a").unwrap().query, "bún");
        assert!(model.search_results("b").is_none());
        assert!(!model.search_pending("a"));
    }

    #[test]
    fn refused_requests_change_nothing_and_clearing_drops_late_replies() {
        let mut model = Model::default();
        assert_eq!(
            model.request_search("a", "   ", |_, _| unreachable!()),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            model.request_search("a", "x", |_, _| Err(SubmitError::Busy)),
            Err(SubmitError::Busy)
        );
        assert!(!model.search_pending("a"));
        let mut request = 0;
        model
            .request_search("a", "x", |r, _| {
                request = r;
                Ok(())
            })
            .unwrap();
        model.clear_search();
        assert!(!model.apply_search(request, "a", results("x")));
        assert!(!model.apply_search_failed(request, "a"));
    }

    #[test]
    fn focus_scrolls_once_then_expires_and_belongs_to_its_thread() {
        let mut model = Model {
            selected: Some("a".into()),
            ..Model::default()
        };
        let cursor = Cursor {
            sent_at_ms: 5,
            id: "9".into(),
        };
        assert_eq!(
            model.focus_hit("a", &hit("1", Some(cursor.clone()))),
            Some(cursor)
        );
        assert_eq!(model.focus().unwrap().message_id, "1");
        let start = Instant::now();
        assert!(model.focus_shown(start));
        assert!(!model.focus_shown(start + Duration::from_millis(10)));
        assert!(model.focus().is_some());
        model.selected = Some("b".into());
        assert!(model.focus().is_none(), "another thread never shows it");
        model.selected = Some("a".into());
        assert!(!model.focus_shown(start + FOCUS_HIGHLIGHT));
        assert!(model.focus().is_none());
    }
}
