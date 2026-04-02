// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::collections::HashMap;
use std::hash::Hash;

pub struct UnwindPoint(usize);
pub struct UnwindMap<K, V> {
    pub m: HashMap<K, V>,
    changes: Vec<(K, Option<V>)>,
}
impl<K: Eq + Hash + Clone, V> UnwindMap<K, V> {
    pub fn new() -> Self {
        Self {
            m: HashMap::new(),
            changes: Vec::new(),
        }
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.m.get(k)
    }

    pub fn insert(&mut self, k: K, v: V) {
        let old = self.m.insert(k.clone(), v);
        self.changes.push((k, old));
    }

    pub fn remove(&mut self, k: &K) {
        let old = self.m.remove(k);
        self.changes.push((k.clone(), old));
    }

    pub fn unwind_point(&mut self) -> UnwindPoint {
        UnwindPoint(self.changes.len())
    }

    pub fn unwind(&mut self, n: UnwindPoint) {
        let n = n.0;
        while self.changes.len() > n {
            let (k, old) = self.changes.pop().unwrap();
            match old {
                Some(v) => self.m.insert(k, v),
                None => self.m.remove(&k),
            };
        }
    }

    pub fn make_permanent(&mut self) {
        self.changes.clear();
    }
}
