use anyhow::Result;
use rusqlite::Connection;

use crate::db::schema::SearchResult;
use crate::db::queries;

pub struct VectorSearch<'a> {
    conn: &'a Connection,
    _model_dim: usize,
}

impl<'a> VectorSearch<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn, _model_dim: 384 }
    }

    pub fn search(&self, project_id: i64, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let all = queries::get_all_embeddings(self.conn, project_id)?;
        if all.is_empty() {
            return Ok(vec![]);
        }

        let mut scored: Vec<(i64, f64)> = all
            .iter()
            .map(|(node_id, emb)| {
                let sim = Self::cosine_similarity(query_embedding, emb);
                (*node_id, sim)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored.retain(|(_, s)| *s > 0.0);

        let mut results = Vec::with_capacity(scored.len());
        for (node_id, score) in scored {
            if let Some(node) = queries::get_node_by_id(self.conn, node_id)? {
                let snippet = node.source.clone().unwrap_or_default();
                let char_len = snippet.chars().count();
                let snippet = if char_len > 200 {
                    let truncated: String = snippet.chars().take(200).collect();
                    format!("{}...", truncated)
                } else {
                    snippet
                };
                results.push(SearchResult { node, score, snippet });
            }
        }

        Ok(results)
    }

    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        (dot / (norm_a * norm_b)) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = VectorSearch::cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = VectorSearch::cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-6);
    }
}
