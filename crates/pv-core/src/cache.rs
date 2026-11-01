//! バイト数で上限を持つ LRU キャッシュ（L1: 処理済みビットマップ、L2: 圧縮済み元バイト列）。

use std::hash::Hash;
use std::sync::Arc;

use bytes::Bytes;
use lru::LruCache;
use pv_decode::Bitmap;

/// L1 のキー: (書庫ID, エントリID, Pipeline::cache_key, 目標サイズ)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageKey {
    pub book: u64,
    pub entry: u32,
    pub pipeline: u64,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawKey {
    pub book: u64,
    pub entry: u32,
}

pub trait Weighted {
    fn weight(&self) -> usize;
}

impl Weighted for Arc<Bitmap> {
    fn weight(&self) -> usize {
        self.data.len()
    }
}

impl Weighted for Bytes {
    fn weight(&self) -> usize {
        self.len()
    }
}

pub struct ByteLru<K: Hash + Eq, V: Weighted + Clone> {
    map: LruCache<K, V>,
    bytes: usize,
    budget: usize,
}

impl<K: Hash + Eq + Clone, V: Weighted + Clone> ByteLru<K, V> {
    pub fn new(budget: u64) -> Self {
        Self {
            map: LruCache::unbounded(),
            bytes: 0,
            budget: budget.min(usize::MAX as u64) as usize,
        }
    }

    pub fn get(&mut self, k: &K) -> Option<V> {
        self.map.get(k).cloned()
    }

    /// 上限を超えたら古いものから追い出す。直前に入れた 1 件は上限超過でも残す。
    pub fn put(&mut self, k: K, v: V) {
        self.bytes += v.weight();
        if let Some(old) = self.map.put(k, v) {
            self.bytes -= old.weight();
        }
        while self.bytes > self.budget && self.map.len() > 1 {
            if let Some((_, old)) = self.map.pop_lru() {
                self.bytes -= old.weight();
            }
        }
    }

    /// 条件を満たさないエントリを捨てる（onTrimMemory で先読み窓の外を解放する用途）。
    pub fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        let drop_keys: Vec<K> = self
            .map
            .iter()
            .filter(|(k, _)| !keep(k))
            .map(|(k, _)| k.clone())
            .collect();
        for k in drop_keys {
            if let Some(v) = self.map.pop(&k) {
                self.bytes -= v.weight();
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(n: usize) -> Bytes {
        Bytes::from(vec![0u8; n])
    }

    #[test]
    fn evicts_least_recently_used_over_budget() {
        let mut c = ByteLru::new(100);
        c.put(1, b(40));
        c.put(2, b(40));
        assert!(c.get(&1).is_some()); // 1 を最近使用にする
        c.put(3, b(40));
        assert!(c.get(&2).is_none());
        assert!(c.get(&1).is_some() && c.get(&3).is_some());
        assert_eq!(c.bytes(), 80);
    }

    #[test]
    fn replacing_key_adjusts_bytes() {
        let mut c = ByteLru::new(100);
        c.put(1, b(30));
        c.put(1, b(10));
        assert_eq!((c.bytes(), c.len()), (10, 1));
    }

    #[test]
    fn newest_item_survives_even_if_larger_than_budget() {
        let mut c = ByteLru::new(10);
        c.put(1, b(50));
        assert_eq!(c.len(), 1);
        c.put(2, b(50));
        assert_eq!((c.len(), c.bytes()), (1, 50));
        assert!(c.get(&2).is_some());
    }

    #[test]
    fn retain_drops_and_accounts() {
        let mut c = ByteLru::new(1000);
        for i in 0..5 {
            c.put(i, b(10));
        }
        c.retain(|k| *k >= 3);
        assert_eq!((c.len(), c.bytes()), (2, 20));
    }
}
