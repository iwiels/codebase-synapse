use anyhow::Result;
use rusqlite::Connection;

use crate::db;
use crate::db::schema::Node;

pub struct AccessTracker<'a> {
    conn: &'a Connection,
    project_id: i64,
}

impl<'a> AccessTracker<'a> {
    pub fn new(conn: &'a Connection, project_id: i64) -> Self {
        Self { conn, project_id }
    }

    /// Record that a node was queried by the AI (call from MCP tools that return node data)
    pub fn record(&self, node_id: i64) -> Result<()> {
        db::queries::record_node_access(self.conn, node_id, self.project_id)
    }

    /// Get most-recently-accessed nodes with temporal decay weighting
    pub fn working_set(&self, limit: i64) -> Result<Vec<(Node, i64)>> {
        db::queries::get_working_set(self.conn, self.project_id, limit)
    }
}
