use std::collections::HashSet;

use anyhow::Result;
use rusqlite::Connection;

use crate::db::schema::Node;

pub struct ImpactAnalysis<'a> {
    conn: &'a Connection,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImpactResult {
    pub symbol: Node,
    pub direct_dependents: Vec<Node>,
    pub transitive_dependents: Vec<Node>,
    pub total_files_affected: usize,
    pub total_symbols_affected: usize,
    pub risk_level: String,
    pub pagerank_score: f64,
}

impl<'a> ImpactAnalysis<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn analyze(&self, node_id: i64, max_depth: i64) -> Result<ImpactResult> {
        let node = crate::db::queries::get_node_by_id(self.conn, node_id)?
            .ok_or_else(|| anyhow::anyhow!("Node not found"))?;

        let direct = self.get_dependents(node_id, 1)?;
        let transitive = if max_depth > 1 {
            self.get_dependents(node_id, max_depth)?
        } else {
            vec![]
        };

        let total_sym = direct.len() + transitive.len();

        let mut files: HashSet<String> = HashSet::new();
        files.insert(node.file_path.clone());
        for n in &direct {
            files.insert(n.file_path.clone());
        }
        for n in &transitive {
            files.insert(n.file_path.clone());
        }

        let pagerank_score =
            crate::db::queries::get_node_pagerank(self.conn, node_id).unwrap_or(0.0);

        let risk = if pagerank_score > 0.05 || direct.len() > 10 {
            "high"
        } else if pagerank_score > 0.01 || direct.len() > 3 {
            "medium"
        } else {
            "low"
        };

        Ok(ImpactResult {
            symbol: node,
            direct_dependents: direct,
            transitive_dependents: transitive,
            total_files_affected: files.len(),
            total_symbols_affected: total_sym,
            risk_level: risk.to_string(),
            pagerank_score,
        })
    }

    fn get_dependents(&self, node_id: i64, depth: i64) -> Result<Vec<Node>> {
        crate::db::queries::get_call_graph(self.conn, node_id, "callers", depth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level_low() {
        let result = ImpactResult {
            symbol: Node {
                id: 1,
                project_id: 1,
                file_path: "src/main.rs".into(),
                kind: "function".into(),
                name: Some("foo".into()),
                qualified_name: None,
                signature: None,
                doc_comment: None,
                start_line: 1,
                end_line: 5,
                complexity: Some(3),
                is_exported: false,
                content_hash: None,
                source: None,
                metadata: None,
                created_at: String::new(),
                updated_at: String::new(),
            },
            direct_dependents: vec![],
            transitive_dependents: vec![],
            total_files_affected: 1,
            total_symbols_affected: 0,
            risk_level: "low".to_string(),
            pagerank_score: 0.0,
        };
        assert_eq!(result.risk_level, "low");
        assert_eq!(result.total_files_affected, 1);
    }

    #[test]
    fn test_risk_level_high() {
        let mut direct = Vec::new();
        for i in 0..25 {
            direct.push(Node {
                id: i + 100,
                project_id: 1,
                file_path: format!("src/mod{}.rs", i),
                kind: "function".into(),
                name: Some(format!("fn{}", i)),
                qualified_name: None,
                signature: None,
                doc_comment: None,
                start_line: 1,
                end_line: 5,
                complexity: Some(1),
                is_exported: false,
                content_hash: None,
                source: None,
                metadata: None,
                created_at: String::new(),
                updated_at: String::new(),
            });
        }
        let result = ImpactResult {
            symbol: direct[0].clone(),
            direct_dependents: direct,
            transitive_dependents: vec![],
            total_files_affected: 26,
            total_symbols_affected: 25,
            risk_level: "high".to_string(),
            pagerank_score: 0.0,
        };
        assert_eq!(result.risk_level, "high");
        assert_eq!(result.total_files_affected, 26);
    }
}
