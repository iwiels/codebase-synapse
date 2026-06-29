use anyhow::Result;
use rusqlite::Connection;

use crate::db::queries;
use crate::db::schema::MemoryNote;

pub struct MemoryStore<'a> {
    conn: &'a Connection,
}

impl<'a> MemoryStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn store(
        &self,
        project_id: i64,
        content: &str,
        node_id: Option<i64>,
        kind: &str,
        tags: Option<&str>,
    ) -> Result<i64> {
        queries::insert_memory_note(self.conn, project_id, content, node_id, kind, tags)
    }

    pub fn search(&self, project_id: i64, query: &str, limit: i64) -> Result<Vec<MemoryNote>> {
        queries::search_memory_notes(self.conn, project_id, query, limit)
    }

    pub fn list(&self, project_id: i64, kind: Option<&str>) -> Result<Vec<MemoryNote>> {
        queries::list_memory_notes(self.conn, project_id, kind)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        queries::delete_memory_note(self.conn, id)
    }
}
