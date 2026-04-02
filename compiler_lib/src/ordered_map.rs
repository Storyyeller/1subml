// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct OrderedMap<K, V> {
    pub keys: Vec<K>,
    pub m: HashMap<K, V>,
}
impl<K: Eq + std::hash::Hash + Clone, V> OrderedMap<K, V> {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            m: HashMap::new(),
        }
    }

    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        let old = self.m.insert(k.clone(), v);
        if old.is_none() {
            self.keys.push(k);
        }
        old
    }

    pub fn entry_or_insert_with(&mut self, k: K, f: impl FnOnce() -> V) -> &mut V {
        self.m.entry(k.clone()).or_insert_with(|| {
            self.keys.push(k);
            f()
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.keys.iter().filter_map(|k| self.m.get(k).map(|v| (k, v)))
    }

    pub fn into_iter(mut self) -> impl Iterator<Item = (K, V)> {
        self.keys.into_iter().map(move |k| {
            let v = self.m.remove(&k).unwrap();
            (k, v)
        })
    }

    pub fn retain(&mut self, f: impl Fn(&K) -> bool) {
        self.m.retain(|k, _| f(k));
        // Also remove any extra keys left from unrelated deletions
        // since journal removal may have left extra keys in the list.
        self.keys.retain(|k| self.m.contains_key(k));
    }
}
