use anyhow::Result;
use rusqlite::Connection;

use crate::db::schema::{Edge, Node};
use crate::db::queries;

pub struct GraphTraversal<'a> {
    conn: &'a Connection,
}

impl<'a> GraphTraversal<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn find_callers(&self, node_id: i64, max_depth: i64) -> Result<Vec<Node>> {
        queries::get_call_graph(self.conn, node_id, "callers", max_depth)
    }

    pub fn find_callees(&self, node_id: i64, max_depth: i64) -> Result<Vec<Node>> {
        queries::get_call_graph(self.conn, node_id, "callees", max_depth)
    }

    pub fn find_path(&self, from_id: i64, to_id: i64, max_depth: i64) -> Result<Vec<Edge>> {
        queries::find_path(self.conn, from_id, to_id, max_depth)
    }

    pub fn get_dependents(&self, node_id: i64) -> Result<Vec<(Edge, Node)>> {
        queries::get_edges_by_target(self.conn, node_id, Some("calls"))
    }

    pub fn get_dependencies(&self, node_id: i64) -> Result<Vec<(Edge, Node)>> {
        queries::get_edges_by_source(self.conn, node_id, Some("calls"))
    }

    pub fn find_dead_code(&self, project_id: i64) -> Result<Vec<Node>> {
        queries::find_dead_code(self.conn, project_id)
    }

    pub fn get_file_structure(&self, project_id: i64, file_path: &str) -> Result<Vec<Node>> {
        queries::get_file_structure(self.conn, project_id, file_path)
    }

    pub fn get_related_by_edge(&self, node_id: i64, edge_kind: &str, direction: &str) -> Result<Vec<Node>> {
        let related = match direction {
            "outgoing" => {
                let edges = queries::get_edges_by_source(self.conn, node_id, Some(edge_kind))?;
                edges.into_iter().map(|(_, n)| n).collect()
            }
            _ => {
                let edges = queries::get_edges_by_target(self.conn, node_id, Some(edge_kind))?;
                edges.into_iter().map(|(_, n)| n).collect()
            }
        };
        Ok(related)
    }
}
