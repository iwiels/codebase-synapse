use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct DecayEntry {
    pub key: String,
    pub access_count: u64,
    pub last_access: Instant,
    pub created: Instant,
}

pub struct DecayScorer {
    entries: Vec<DecayEntry>,
    half_life: Duration,
}

impl DecayScorer {
    pub fn new(half_life_secs: u64) -> Self {
        Self {
            entries: Vec::new(),
            half_life: Duration::from_secs(half_life_secs),
        }
    }

    pub fn record_access(&mut self, key: &str) {
        let now = Instant::now();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.key == key) {
            entry.access_count += 1;
            entry.last_access = now;
        } else {
            self.entries.push(DecayEntry {
                key: key.to_string(),
                access_count: 1,
                last_access: now,
                created: now,
            });
        }
    }

    pub fn score(&self, key: &str) -> f64 {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|entry| {
                let elapsed = entry.last_access.elapsed();
                let age_factor = (-(elapsed.as_secs_f64()) / self.half_life.as_secs_f64()).exp();
                let frequency_factor = (entry.access_count as f64).ln_1p();
                age_factor * frequency_factor
            })
            .unwrap_or(0.0)
    }

    pub fn prune_below(&mut self, threshold: f64) {
        let keys: Vec<String> = self
            .entries
            .iter()
            .filter(|e| self.score(&e.key) < threshold)
            .map(|e| e.key.clone())
            .collect();
        self.entries.retain(|e| !keys.contains(&e.key));
    }

    pub fn entries_below(&self, threshold: f64) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| self.score(&e.key) < threshold)
            .map(|e| e.key.clone())
            .collect()
    }
}

impl Default for DecayScorer {
    fn default() -> Self {
        Self::new(3600)
    }
}
