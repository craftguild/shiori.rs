use std::hash::Hash;

use lru::LruCache;

use super::Projection;
use crate::catalog::Snapshot;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub id: String,
    pub hash: [u8; 32],
    pub profile: [u8; 32],
}

pub struct RenderCache {
    entries: LruCache<Key, (Projection, usize)>,
    bytes: usize,
    budget: usize,
}

impl RenderCache {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: LruCache::unbounded(),
            bytes: 0,
            budget,
        }
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
    pub fn get(&mut self, key: &Key) -> Option<Projection> {
        self.entries.get(key).map(|(value, _)| value.clone())
    }
    pub fn insert(&mut self, key: Key, value: Projection) {
        let bytes = value.retained_bytes() + key.id.capacity() + std::mem::size_of::<Key>() + 128;
        if bytes > self.budget {
            return;
        }
        if let Some((_, old_bytes)) = self.entries.pop(&key) {
            self.bytes -= old_bytes;
        }
        while self.bytes + bytes > self.budget {
            if let Some((_, (_, old_bytes))) = self.entries.pop_lru() {
                self.bytes -= old_bytes;
            }
        }
        self.entries.put(key, (value, bytes));
        self.bytes += bytes;
    }
    pub fn retain_current(&mut self, snapshot: &Snapshot) {
        let stale: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| {
                snapshot
                    .articles
                    .get(&key.id)
                    .is_none_or(|article| article.hash != key.hash)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            if let Some((_, bytes)) = self.entries.pop(&key) {
                self.bytes -= bytes;
            }
        }
    }
}
