//! SQLite database layer: schema (12 tables, FTS5, triggers), queries, and connection management.

pub mod queries;
pub mod schema;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub use schema::EdgeKind;
pub use schema::NodeKind;

pub type SharedConnection = Mutex<Connection>;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    schema::migrate(&conn)?;
    Ok(conn)
}
