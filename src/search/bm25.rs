use anyhow::Result;
use rusqlite::Connection;

use crate::db::schema::{Node, SearchResult};
use crate::db::queries;

pub struct Bm25Search<'a> {
    conn: &'a Connection,
}

impl<'a> Bm25Search<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn search(&self, project_id: i64, query: &str, limit: i64) -> Result<Vec<SearchResult>> {
        let fts_results = queries::fts_search(self.conn, project_id, query, limit)?;
        let results = fts_results
            .into_iter()
            .map(|(node, score)| {
                let snippet = node.source.clone().unwrap_or_default();
                let char_len = snippet.chars().count();
                let snippet = if char_len > 200 {
                    let truncated: String = snippet.chars().take(200).collect();
                    format!("{}...", truncated)
                } else {
                    snippet
                };
                SearchResult { node, score, snippet }
            })
            .collect();
        Ok(results)
    }

    pub fn search_by_name(&self, project_id: i64, pattern: &str, limit: i64) -> Result<Vec<Node>> {
        queries::search_nodes_by_name(self.conn, project_id, pattern, limit, 0)
    }
}
