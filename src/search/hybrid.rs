use super::bm25::Bm25Search;
use super::vector::VectorSearch;
use crate::db::schema::{Node, SearchResult};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

const RRF_K: f64 = 60.0;
const PAGERANK_BOOST: f64 = 5.0; // multiply RRF score by (1 + rank * BOOST)

pub struct HybridSearch<'a> {
    conn: &'a Connection,
    bm25: Bm25Search<'a>,
    vector: VectorSearch<'a>,
}

impl<'a> HybridSearch<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            bm25: Bm25Search::new(conn),
            vector: VectorSearch::new(conn),
        }
    }

    pub fn search(
        &self,
        project_id: i64,
        query: &str,
        query_embedding: Option<&[f32]>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let bm25_results = self.bm25.search(project_id, query, limit as i64)?;

        let mut merged: HashMap<i64, (f64, Node, String)> = HashMap::new();

        for (rank, result) in bm25_results.iter().enumerate() {
            let rrf = 1.0 / (RRF_K + (rank as f64 + 1.0));
            let pr = crate::db::queries::get_node_pagerank(self.conn, result.node.id)
                .unwrap_or_else(|e| {
                    tracing::warn!("PageRank query failed for node {}: {}", result.node.id, e);
                    0.0
                });
            let score = rrf * (1.0 + pr * PAGERANK_BOOST);
            merged.insert(
                result.node.id,
                (score, result.node.clone(), result.snippet.clone()),
            );
        }

        if let Some(embedding) = query_embedding {
            let vector_results = self.vector.search(project_id, embedding, limit)?;
            for (rank, result) in vector_results.iter().enumerate() {
                let rrf = 1.0 / (RRF_K + (rank as f64 + 1.0));
                let pr = crate::db::queries::get_node_pagerank(self.conn, result.node.id)
                    .unwrap_or_else(|e| {
                        tracing::warn!("PageRank query failed for node {}: {}", result.node.id, e);
                        0.0
                    });
                let score = rrf * (1.0 + pr * PAGERANK_BOOST);
                merged
                    .entry(result.node.id)
                    .and_modify(|(s, _, _)| *s += score)
                    .or_insert((score, result.node.clone(), result.snippet.clone()));
            }
        }

        let mut results: Vec<SearchResult> = merged
            .into_values()
            .map(|(score, node, snippet)| SearchResult {
                node,
                score,
                snippet,
            })
            .collect();
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_rrf_constant() {
        assert_eq!(RRF_K, 60.0);
    }
    #[test]
    fn test_rrf_score_decreasing() {
        let s1 = 1.0 / (RRF_K + 1.0);
        let s2 = 1.0 / (RRF_K + 2.0);
        assert!(s1 > s2);
    }
}
