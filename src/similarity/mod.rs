use std::collections::{HashMap, HashSet};
use rusqlite::Connection;
use anyhow::Result;
use xxhash_rust::xxh3::xxh3_64_with_seed;

use crate::db::schema::Node;

const MINHASH_SIZE: usize = 100;
const LSH_BANDS: usize = 20;
const LSH_ROWS: usize = 5; // 20 bands * 5 rows = 100 signatures

/// Normalize code by removing comments, formatting, and excess whitespace to ensure structural similarity matches.
pub fn normalize_code(code: &str) -> String {
    // 1. Remove single-line comments // or #
    let mut clean = String::new();
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }
        clean.push_str(trimmed);
    }
    // Remove all whitespace to ignore formatting differences completely
    clean.retain(|c| !c.is_whitespace());
    clean
}

/// Compute 100 MinHash values for a string using 3-character shingles and XXH3 seeds.
pub fn compute_minhash(text: &str) -> Vec<u64> {
    let normalized = normalize_code(text);
    let chars: Vec<char> = normalized.chars().collect();
    let mut signature = vec![u64::MAX; MINHASH_SIZE];

    // Handle empty or very short strings gracefully
    if chars.len() < 3 {
        return signature;
    }

    // 3-char shingling
    for i in 0..=(chars.len() - 3) {
        let shingle: String = chars[i..(i + 3)].iter().collect();
        
        // Compute minhash for 100 seeds
        for (seed, sig) in signature.iter_mut().enumerate().take(MINHASH_SIZE) {
            let hash = xxh3_64_with_seed(shingle.as_bytes(), seed as u64);
            if hash < *sig {
                *sig = hash;
            }
        }
    }

    signature
}

/// Estimate Jaccard similarity between two MinHash signatures.
pub fn estimate_similarity(sig_a: &[u64], sig_b: &[u64]) -> f64 {
    if sig_a.len() != MINHASH_SIZE || sig_b.len() != MINHASH_SIZE {
        return 0.0;
    }
    let mut matches = 0;
    for i in 0..MINHASH_SIZE {
        if sig_a[i] == sig_b[i] && sig_a[i] != u64::MAX {
            matches += 1;
        }
    }
    matches as f64 / MINHASH_SIZE as f64
}

/// LSH (Locality Sensitive Hashing) Index for fast candidate pair generation.
pub struct LshIndex {
    buckets: Vec<HashMap<u64, Vec<i64>>>,
}

impl Default for LshIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl LshIndex {
    pub fn new() -> Self {
        let mut buckets = Vec::with_capacity(LSH_BANDS);
        for _ in 0..LSH_BANDS {
            buckets.push(HashMap::new());
        }
        Self { buckets }
    }

    pub fn insert(&mut self, node_id: i64, signature: &[u64]) {
        if signature.len() != MINHASH_SIZE {
            return;
        }

        for band in 0..LSH_BANDS {
            let start = band * LSH_ROWS;
            let end = start + LSH_ROWS;
            let band_data = &signature[start..end];
            
            // Hash the band rows together
            let mut bytes = Vec::with_capacity(LSH_ROWS * 8);
            for &val in band_data {
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            let bucket_hash = xxh3_64_with_seed(&bytes, 42);

            self.buckets[band]
                .entry(bucket_hash)
                .or_default()
                .push(node_id);
        }
    }

    pub fn query(&self, signature: &[u64]) -> HashSet<i64> {
        let mut candidates = HashSet::new();
        if signature.len() != MINHASH_SIZE {
            return candidates;
        }

        for band in 0..LSH_BANDS {
            let start = band * LSH_ROWS;
            let end = start + LSH_ROWS;
            let band_data = &signature[start..end];
            
            let mut bytes = Vec::with_capacity(LSH_ROWS * 8);
            for &val in band_data {
                bytes.extend_from_slice(&val.to_le_bytes());
            }
            let bucket_hash = xxh3_64_with_seed(&bytes, 42);

            if let Some(list) = self.buckets[band].get(&bucket_hash) {
                for &id in list {
                    candidates.insert(id);
                }
            }
        }

        candidates
    }
}

/// Identify structurally similar function/method nodes in the project.
pub fn find_similar_pairs(
    conn: &Connection,
    project_id: i64,
    threshold: f64,
) -> Result<Vec<(i64, i64, f64)>> {
    // 1. Fetch all function and method nodes with source code
    let mut stmt = conn.prepare(
        "SELECT id, file_path, kind, name, qualified_name, start_line, end_line, complexity, is_exported, source, metadata
         FROM nodes
         WHERE project_id = ?1 AND kind IN ('function', 'method') AND source IS NOT NULL"
    )?;
    
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        Ok(Node {
            id: row.get(0)?,
            project_id,
            file_path: row.get(1)?,
            kind: row.get(2)?,
            name: row.get(3)?,
            qualified_name: row.get(4)?,
            signature: None,
            doc_comment: None,
            start_line: row.get(5)?,
            end_line: row.get(6)?,
            complexity: row.get(7)?,
            is_exported: row.get(8)?,
            content_hash: None,
            source: row.get(9)?,
            metadata: row.get(10)?,
            created_at: String::new(),
            updated_at: String::new(),
        })
    })?;

    let mut nodes = Vec::new();
    for r in rows {
        let n = r?;
        if let Some(ref src) = n.source {
            // Only process nodes with substantial source code to ignore trivial matches
            if src.len() >= 50 && (n.end_line - n.start_line) >= 3 {
                nodes.push(n);
            }
        }
    }

    // 2. Compute Minhases
    let mut signatures = HashMap::new();
    let mut lsh = LshIndex::new();

    for node in &nodes {
        if let Some(ref src) = node.source {
            let sig = compute_minhash(src);
            lsh.insert(node.id, &sig);
            signatures.insert(node.id, sig);
        }
    }

    // 3. Query LSH and compute Jaccard similarities
    let mut similar_pairs = Vec::new();
    let mut processed_pairs = HashSet::new();

    for node in &nodes {
        let sig = match signatures.get(&node.id) {
            Some(s) => s,
            None => continue,
        };
        let candidates = lsh.query(sig);

        for cand_id in candidates {
            if cand_id == node.id {
                continue;
            }

            // Order IDs to prevent duplicate pairs
            let min_id = std::cmp::min(node.id, cand_id);
            let max_id = std::cmp::max(node.id, cand_id);
            if !processed_pairs.insert((min_id, max_id)) {
                continue;
            }

            let cand_sig = match signatures.get(&cand_id) {
                Some(s) => s,
                None => continue,
            };
            let sim = estimate_similarity(sig, cand_sig);

            if sim >= threshold {
                similar_pairs.push((min_id, max_id, sim));
            }
        }
    }

    Ok(similar_pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minhash_similarity() {
        let code1 = "fn calculate_total(price: f64, tax_rate: f64, discount: f64) -> f64 {
            let subtotal = price * (1.0 + tax_rate);
            let final_total = subtotal - discount;
            println!(\"Final: {}\", final_total);
            return final_total;
        }";
        let code2 = "fn calculate_total(p: f64, t: f64, d: f64) -> f64 {\n    let sub = p * (1.0 + t);\n    let fin = sub - d;\n    println!(\"Final: {}\", fin);\n    return fin;\n}";
        let code3 = "fn format_string(name: &str, age: u32) -> String {
            let message = format!(\"Hello {}, you are {} years old\", name, age);
            println!(\"Log: {}\", message);
            return message;
        }";

        let sig1 = compute_minhash(code1);
        let sig2 = compute_minhash(code2);
        let sig3 = compute_minhash(code3);

        let sim12 = estimate_similarity(&sig1, &sig2);
        let sim13 = estimate_similarity(&sig1, &sig3);

        assert!(sim12 > 0.50, "Renaming variables and formatting should yield high similarity, got {}", sim12);
        assert!(sim13 < 0.20, "Different operations should yield lower similarity, got {}", sim13);
    }
}
