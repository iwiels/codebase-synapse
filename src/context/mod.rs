use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::db;
use crate::db::schema::Node;
use crate::graph::{GraphTraversal, ImpactAnalysis};
use crate::search::{Bm25Search, HybridSearch};

pub struct ContextBudget<'a> {
    conn: &'a Connection,
    project_id: i64,
}

impl<'a> ContextBudget<'a> {
    pub fn new(conn: &'a Connection, project_id: i64) -> Self {
        Self { conn, project_id }
    }

    /// Assemble an optimal context bundle for a coding task.
    /// Steps: (1) hybrid/BM25 search → (2) impact analysis on top hit →
    ///        (3) memory notes → (4) callers/callees for top 3 → (5) return budgeted JSON.
    pub fn prepare(
        &self,
        task_description: &str,
        max_tokens: usize,
        query_vec: Option<&[f32]>,
    ) -> Result<Value> {
        let limit = 15i64;

        // Step 1: Find relevant symbols
        let relevant_nodes: Vec<Node> = if query_vec.is_some() {
            let hybrid = HybridSearch::new(self.conn);
            hybrid.search(self.project_id, task_description, query_vec, limit as usize)?
                .into_iter().map(|r| r.node).collect()
        } else {
            let bm25 = Bm25Search::new(self.conn);
            bm25.search(self.project_id, task_description, limit)?
                .into_iter().map(|r| r.node).collect()
        };

        // Step 2: Impact analysis on most relevant node
        let impact = relevant_nodes.first().and_then(|n| {
            ImpactAnalysis::new(self.conn).analyze(n.id, 3).ok()
        });

        // Step 3: Related memory notes
        let memories = db::queries::search_memory_notes(
            self.conn, self.project_id, task_description, 10
        ).unwrap_or_else(|e| {
            tracing::warn!("Memory search failed: {}", e);
            vec![]
        });

        // Step 4: Graph context for top 3 nodes
        let traversal = GraphTraversal::new(self.conn);
        let graph_context: Vec<Value> = relevant_nodes.iter().take(3).map(|node| {
            let callers = traversal.find_callers(node.id, 1).unwrap_or_default();
            let callees = traversal.find_callees(node.id, 1).unwrap_or_default();
            json!({
                "node_id": node.id, "name": node.name, "kind": node.kind,
                "file": node.file_path, "signature": node.signature,
                "callers_count": callers.len(), "callees_count": callees.len(),
                "callers": callers.iter().map(|n| json!({"id": n.id, "name": n.name, "file": n.file_path})).collect::<Vec<_>>(),
                "callees": callees.iter().map(|n| json!({"id": n.id, "name": n.name, "file": n.file_path})).collect::<Vec<_>>()
            })
        }).collect();

        // Step 5: Assemble budgeted bundle (source excluded to save tokens)
        Ok(json!({
            "task": task_description,
            "symbol_count": relevant_nodes.len(),
            "relevant_symbols": relevant_nodes.iter().map(|n| json!({
                "id": n.id, "name": n.name, "kind": n.kind,
                "file": n.file_path,
                "lines": format!("{}–{}", n.start_line, n.end_line),
                "complexity": n.complexity, "signature": n.signature,
                "doc": n.doc_comment
            })).collect::<Vec<_>>(),
            "graph_context": graph_context,
            "impact_analysis": impact,
            "related_memories": memories,
            "budget_note": format!(
                "Max {} tokens. Source code excluded — use get_context(node_id) for full source of specific symbols.",
                max_tokens
            )
        }))
    }
}
