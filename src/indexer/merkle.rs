use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::util::hash;

pub struct MerkleTree {
    file_hashes: HashMap<String, u64>,
}

impl MerkleTree {
    pub fn new() -> Self {
        Self {
            file_hashes: HashMap::new(),
        }
    }

    pub fn compute_file_hash(&self, path: &Path) -> Result<u64> {
        let content = std::fs::read(path)?;
        Ok(hash::content_hash(&content))
    }

    pub fn has_changed(&self, path: &Path) -> Result<bool> {
        let rel_path = path.to_string_lossy().to_string();
        let current_hash = self.compute_file_hash(path)?;
        match self.file_hashes.get(&rel_path) {
            Some(old_hash) => Ok(*old_hash != current_hash),
            None => Ok(true),
        }
    }

    pub fn update_hash(&mut self, path: &Path) -> Result<u64> {
        let rel_path = path.to_string_lossy().to_string();
        let current_hash = self.compute_file_hash(path)?;
        self.file_hashes.insert(rel_path, current_hash);
        Ok(current_hash)
    }

    pub fn remove_path(&mut self, path: &Path) {
        self.file_hashes.remove(&path.to_string_lossy().to_string());
    }
}

impl Default for MerkleTree {
    fn default() -> Self {
        Self::new()
    }
}
