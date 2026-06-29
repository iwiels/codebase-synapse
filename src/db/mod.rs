//! SQLite database layer: schema (12 tables, FTS5, triggers), queries, and connection management.

pub mod schema;
pub mod queries;

use std::path::Path;
use std::sync::Mutex;
use anyhow::Result;
use rusqlite::Connection;

pub use schema::NodeKind;
pub use schema::EdgeKind;

pub type SharedConnection = Mutex<Connection>;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    schema::migrate(&conn)?;
    Ok(conn)
}
