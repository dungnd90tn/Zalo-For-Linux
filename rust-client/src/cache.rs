//! O(1) LRU media lookups, bounded by cache-owned payload bytes and entry count.
//! Consumers holding cloned Arc values may retain memory after eviction.
use lru::LruCache;
use std::{num::NonZeroUsize, sync::Arc};

pub const MAX_ENTRIES: usize = 1024;
pub const MAX_KEY_BYTES: usize = 512;
pub struct MediaCache {
    entries: LruCache<String, Arc<[u8]>>,
    max_bytes: usize,
    bytes: usize,
}
impl MediaCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(MAX_ENTRIES).unwrap()),
            max_bytes,
            bytes: 0,
        }
    }
    pub fn get(&mut self, key: &str) -> Option<Arc<[u8]>> {
        self.entries.get(key).cloned()
    }
    /// Rejected entries do not remove an existing value or evict useful data.
    pub fn insert(&mut self, key: String, data: Arc<[u8]>) -> bool {
        if key.is_empty()
            || key.len() > MAX_KEY_BYTES
            || data.is_empty()
            || data.len() > self.max_bytes
        {
            return false;
        }
        if let Some(old) = self.entries.pop(&key) {
            self.bytes -= old.len();
        }
        while self.bytes > self.max_bytes - data.len() || self.entries.len() >= MAX_ENTRIES {
            if let Some((_, old)) = self.entries.pop_lru() {
                self.bytes -= old.len();
            } else {
                break;
            }
        }
        self.bytes += data.len();
        self.entries.put(key, data);
        true
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_budget_evicts_cold_entries_and_replacement_accounts_exactly() {
        let mut c = MediaCache::new(8);
        c.insert("a".into(), Arc::from([1u8; 3]));
        c.insert("b".into(), Arc::from([2u8; 3]));
        assert!(c.get("a").is_some());
        c.insert("c".into(), Arc::from([3u8; 4]));
        assert!(c.get("b").is_none());
        assert_eq!(c.bytes(), 7);
        for _ in 0..100 {
            assert!(c.insert("a".into(), Arc::from([4u8; 2])));
            assert_eq!(c.bytes(), 6);
        }
        assert!(!c.insert("oversize".into(), Arc::from([0u8; 9])));
        assert_eq!(c.bytes(), 6);
    }
    #[test]
    fn tiny_objects_cannot_grow_metadata_without_bound() {
        let mut c = MediaCache::new(usize::MAX);
        for i in 0..MAX_ENTRIES + 100 {
            c.insert(i.to_string(), Arc::from([1u8]));
        }
        assert_eq!(c.len(), MAX_ENTRIES);
        assert_eq!(c.bytes(), MAX_ENTRIES);
        assert!(c.get("0").is_none());
        assert!(!c.insert("x".repeat(MAX_KEY_BYTES + 1), Arc::from([1u8])));
    }
    #[test]
    fn zero_budget_keeps_no_entries() {
        let mut c = MediaCache::new(0);
        assert!(!c.insert("a".into(), Arc::from([1u8])));
        assert!(c.is_empty());
    }
}
