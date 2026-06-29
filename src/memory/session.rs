use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub key: String,
    pub value: String,
    pub timestamp: String,
    pub source: String,
}

pub struct SessionMemory {
    store: HashMap<String, SessionEntry>,
    max_entries: usize,
}

impl SessionMemory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            store: HashMap::new(),
            max_entries,
        }
    }

    pub fn remember(&mut self, key: &str, value: &str, source: &str) {
        let entry = SessionEntry {
            key: key.to_string(),
            value: value.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            source: source.to_string(),
        };

        if self.store.len() >= self.max_entries {
            if let Some(oldest_key) = self
                .store
                .iter()
                .min_by(|a, b| a.1.timestamp.cmp(&b.1.timestamp))
                .map(|(k, _)| k.clone())
            {
                self.store.remove(&oldest_key);
            }
        }

        self.store.insert(key.to_string(), entry);
    }

    pub fn recall(&self, key: &str) -> Option<&SessionEntry> {
        self.store.get(key)
    }

    pub fn search(&self, query: &str) -> Vec<&SessionEntry> {
        let q = query.to_lowercase();
        self.store
            .values()
            .filter(|e| e.key.to_lowercase().contains(&q) || e.value.to_lowercase().contains(&q))
            .collect()
    }

    pub fn all(&self) -> Vec<&SessionEntry> {
        self.store.values().collect()
    }

    pub fn clear(&mut self) {
        self.store.clear();
    }
}
