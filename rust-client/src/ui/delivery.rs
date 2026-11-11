//! Bounded UI review state for interrupted sends. The durable outbox is authoritative.
use sha2::{Digest, Sha256};
use std::collections::HashMap;

struct Pending {
    request: u64,
    digest: [u8; 32],
}
pub(super) struct Review {
    request: u64,
    digest: Option<[u8; 32]>,
    pub accepted: bool,
    pub same_body: bool,
}
#[derive(Default)]
pub(super) struct DeliveryBook {
    pending: HashMap<String, Pending>,
    reviews: HashMap<String, Review>,
    overflow: bool,
}
impl DeliveryBook {
    pub fn submitted(&mut self, thread: &str, request: u64, text: &str) {
        if self.pending.len() < crate::domain::MAX_PAGE_SIZE || self.pending.contains_key(thread) {
            self.pending.insert(
                thread.into(),
                Pending {
                    request,
                    digest: digest(text),
                },
            );
        } else {
            self.overflow = true;
        }
    }
    pub fn failed(&mut self, thread: &str, request: u64, accepted: bool) -> bool {
        if let Some(pending) = self.pending.get(thread) {
            if pending.request != request {
                return false;
            }
        } else {
            // Replayed failures cannot replace the digest or reopen a dismissed review.
            if let Some(review) = self
                .reviews
                .get_mut(thread)
                .filter(|review| review.request == request)
            {
                if accepted && !review.accepted {
                    review.accepted = true;
                    return true;
                }
            }
            return false;
        }
        let original = self
            .pending
            .get(thread)
            .filter(|item| item.request == request)
            .map(|item| item.digest);
        if original.is_some() {
            self.pending.remove(thread);
        }
        if self.reviews.len() < crate::domain::MAX_PAGE_SIZE || self.reviews.contains_key(thread) {
            self.reviews.insert(
                thread.into(),
                Review {
                    request,
                    digest: original,
                    accepted,
                    same_body: true,
                },
            );
        } else {
            self.overflow = true;
        }
        true
    }
    /// Worker loss cannot prove whether outstanding HTTP sends reached Zalo.
    pub fn interrupted(&mut self) {
        let pending: Vec<_> = self
            .pending
            .iter()
            .map(|(thread, item)| (thread.clone(), item.request))
            .collect();
        for (thread, request) in pending {
            self.failed(&thread, request, false);
        }
    }
    pub fn sent(&mut self, thread: &str, request: u64) {
        if self
            .pending
            .get(thread)
            .is_some_and(|item| item.request == request)
        {
            self.pending.remove(thread);
        }
        if self
            .reviews
            .get(thread)
            .is_some_and(|item| item.request == request)
        {
            self.reviews.remove(thread);
        }
    }
    pub fn edited(&mut self, thread: &str, text: &str) {
        if let Some(review) = self.reviews.get_mut(thread) {
            review.same_body = review
                .digest
                .is_none_or(|original| original == digest(text));
        }
    }
    pub fn can_send(&self, thread: &str) -> bool {
        !self.overflow
            && self
                .reviews
                .get(thread)
                .is_none_or(|review| !review.same_body)
    }
    pub fn review(&self, thread: &str) -> Option<&Review> {
        self.reviews.get(thread)
    }
}
fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_failures_cannot_replace_newer_pending_or_review_state() {
        let mut book = DeliveryBook::default();
        book.submitted("d:22", 1, "old");
        assert!(book.failed("d:22", 1, false));
        book.edited("d:22", "new");
        book.submitted("d:22", 2, "new");
        assert!(!book.failed("d:22", 1, true));
        assert!(!book.review("d:22").unwrap().accepted);
        assert!(book.failed("d:22", 2, true));
        assert!(!book.failed("d:22", 1, false));
        assert!(book.review("d:22").unwrap().accepted);
        assert!(
            !book.failed("d:22", 2, false),
            "Acceptance is never downgraded by replay"
        );
        assert!(book.review("d:22").unwrap().accepted);
    }

    #[test]
    fn stopped_worker_preserves_unacknowledged_send_guard_across_reconnect() {
        let mut book = DeliveryBook::default();
        book.submitted("d:22", 7, "Giữ nguyên bản nháp");
        book.interrupted();
        assert!(!book.can_send("d:22"));
        assert!(!book.review("d:22").unwrap().accepted);
        book.sent("d:22", 8);
        assert!(
            !book.can_send("d:22"),
            "unrelated ACK cannot release original send"
        );
        book.sent("d:22", 7);
        assert!(book.can_send("d:22"));
    }
    #[test]
    fn unknown_send_preserves_new_drafts_but_blocks_original_body_even_after_undo() {
        let mut book = DeliveryBook::default();
        book.submitted("d:123", 7, "xin chào");
        book.failed("d:123", 7, false);
        assert!(!book.can_send("d:123"));
        book.edited("d:123", "tin nhắn khác");
        assert!(book.can_send("d:123"));
        book.edited("d:123", "xin chào");
        assert!(!book.can_send("d:123"));
        assert!(book.can_send("d:456"));
        book.sent("d:123", 999);
        assert!(!book.can_send("d:123"));
        book.sent("d:123", 7);
        assert!(book.can_send("d:123"));
    }
    #[test]
    fn server_acceptance_remains_distinct_from_unknown_transport_outcome() {
        let mut book = DeliveryBook::default();
        book.submitted("g:123", 1, "accepted");
        book.failed("g:123", 1, true);
        assert!(book.review("g:123").unwrap().accepted);
        assert!(!book.can_send("g:123"));
        book.edited("g:123", "");
        assert!(book.can_send("g:123"));
    }
}
