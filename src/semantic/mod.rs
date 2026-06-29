use std::collections::HashSet;
use crate::db::schema::Node;
use crate::similarity::estimate_similarity;

/// Compute Jaccard overlap of code tokens (words).
pub fn token_overlap(code_a: &str, code_b: &str) -> f64 {
    let get_tokens = |text: &str| -> HashSet<String> {
        text.split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() > 1)
            .map(|s| s.to_lowercase())
            .collect()
    };

    let tokens_a = get_tokens(code_a);
    let tokens_b = get_tokens(code_b);

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }
    
    let intersection = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();
    intersection as f64 / union as f64
}

/// Compute folder proximity score (1.0 for same folder, decaying by parent level difference).
pub fn directory_proximity(path_a: &str, path_b: &str) -> f64 {
    let parts_a: Vec<&str> = path_a.split(&['/', '\\'][..]).collect();
    let parts_b: Vec<&str> = path_b.split(&['/', '\\'][..]).collect();

    let mut common = 0;
    for (i, p_a) in parts_a.iter().enumerate() {
        if i < parts_b.len() && p_a == &parts_b[i] {
            common += 1;
        } else {
            break;
        }
    }

    if common == 0 {
        return 0.0;
    }

    let max_len = std::cmp::max(parts_a.len(), parts_b.len());
    common as f64 / max_len as f64
}

/// Compute AST Profile Similarity (Complexity, Size, Export status).
pub fn ast_profile_similarity(node_a: &Node, node_b: &Node) -> f64 {
    // 1. Line count similarity
    let lines_a = (node_a.end_line - node_a.start_line).max(1) as f64;
    let lines_b = (node_b.end_line - node_b.start_line).max(1) as f64;
    let line_sim = 1.0 - (lines_a - lines_b).abs() / std::cmp::max(node_a.end_line - node_a.start_line, node_b.end_line - node_b.start_line).max(1) as f64;

    // 2. Complexity similarity
    let comp_a = node_a.complexity.unwrap_or(1) as f64;
    let comp_b = node_b.complexity.unwrap_or(1) as f64;
    let comp_sim = 1.0 - (comp_a - comp_b).abs() / comp_a.max(comp_b).max(1.0);

    // 3. Export matching
    let export_sim = if node_a.is_exported == node_b.is_exported { 1.0 } else { 0.5 };

    (line_sim * 0.4) + (comp_sim * 0.4) + (export_sim * 0.2)
}

/// Combine MinHash, Token overlap, Directory proximity, and AST profiles into a single semantic score.
pub fn compute_semantic_score(node_a: &Node, node_b: &Node, sig_a: &[u64], sig_b: &[u64]) -> f64 {
    let minhash_sim = estimate_similarity(sig_a, sig_b);

    let src_a = node_a.source.as_deref().unwrap_or("");
    let src_b = node_b.source.as_deref().unwrap_or("");
    let overlap_sim = token_overlap(src_a, src_b);

    let prox_sim = directory_proximity(&node_a.file_path, &node_b.file_path);
    let ast_sim = ast_profile_similarity(node_a, node_b);

    // Weights:
    // MinHash: 40%, Token Overlap: 30%, Directory Proximity: 15%, AST Profile: 15%
    (minhash_sim * 0.40) + (overlap_sim * 0.30) + (prox_sim * 0.15) + (ast_sim * 0.15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_proximity() {
        let p1 = "src/controllers/user.rs";
        let p2 = "src/controllers/auth.rs";
        let p3 = "src/models/user.rs";

        assert!(directory_proximity(p1, p2) > directory_proximity(p1, p3));
    }

    #[test]
    fn test_token_overlap() {
        let c1 = "fn parse(s: &str) -> Result<Config> {}";
        let c2 = "fn parse(input: &str) -> Result<Config> {}";
        let c3 = "struct Data { value: i32 }";

        assert!(token_overlap(c1, c2) > 0.60);
        assert!(token_overlap(c1, c3) < 0.20);
    }
}
