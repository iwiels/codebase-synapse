pub mod parser;
pub mod planner;

pub use parser::parse_cypher;
pub use planner::CypherPlanner;

use rusqlite::Connection;
use serde_json::Value;

/// Execute a Cypher query against the SQLite graph database.
pub fn query_graph(conn: &Connection, project_id: i64, query_str: &str) -> Result<Value, String> {
    let parsed_query = parse_cypher(query_str)?;
    let planner = CypherPlanner::new(conn);
    planner.execute(project_id, &parsed_query)
}
