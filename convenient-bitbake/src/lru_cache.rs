//! LRU cache eviction policy

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// LRU cache with size limit
pub struct LruCache<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    order: VecDeque<K>,
}

impl<K: Eq + Hash + Clone, V> LruCache<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.map.contains_key(key) {
            self.touch(key);
            self.map.get(key)
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(old) = self.map.insert(key.clone(), value) {
            self.touch(&key);
            Some(old)
        } else {
            self.order.push_back(key.clone());
            if self.order.len() > self.capacity {
                if let Some(evicted) = self.order.pop_front() {
                    return self.map.remove(&evicted);
                }
            }
            None
        }
    }

    fn touch(&mut self, key: &K) {
        self.order.retain(|k| k != key);
        self.order.push_back(key.clone());
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> usize {
        self.map.len()
    }
}
