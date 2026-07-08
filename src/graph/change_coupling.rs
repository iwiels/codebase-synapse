use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::schema::Node;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChangeCoupling {
    pub source_file: String,
    pub target_file: String,
    pub co_change_count: i64,
    pub source_node_id: i64,
    pub target_node_id: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CouplingEdge {
    pub node_id: i64,
    pub coupled_node_id: i64,
    pub co_change_count: i64,
}

/// Detect change coupling patterns from git commit history.
///
/// Analyzes which files frequently change together across commits,
/// even if there is no explicit code dependency between them.
/// Stores results as `change_coupling` edges with `{"co_changes": N}` metadata.
pub fn detect_and_store(
    conn: &Connection,
    project_id: i64,
    min_co_changes: i64,
) -> Result<Vec<ChangeCoupling>> {
    // Find all pairs of nodes that co-change in the same commit
    // Only consider nodes that represent files (kind = 'file')
    let mut pairs = conn.prepare(
        "SELECT cnl1.commit_hash, n1.id AS src_id, n1.file_path AS src_path,
                n2.id AS dst_id, n2.file_path AS dst_path
         FROM commit_node_links cnl1
         JOIN commit_node_links cnl2
           ON cnl1.commit_hash = cnl2.commit_hash
           AND cnl1.node_id < cnl2.node_id
         JOIN nodes n1 ON n1.id = cnl1.node_id AND n1.project_id = ?1 AND n1.kind = 'file'
         JOIN nodes n2 ON n2.id = cnl2.node_id AND n2.project_id = ?1 AND n2.kind = 'file'
         WHERE cnl1.project_id = ?1",
    )?;

    let mut pair_counts: std::collections::HashMap<(i64, i64, String, String), i64> =
        std::collections::HashMap::new();

    {
        let mut rows = pairs.query(rusqlite::params![project_id])?;
        while let Some(row) = rows.next()? {
            let _commit_hash: String = row.get(0)?;
            let src_id: i64 = row.get(1)?;
            let src_path: String = row.get(2)?;
            let dst_id: i64 = row.get(3)?;
            let dst_path: String = row.get(4)?;

            let key = (src_id, dst_id, src_path, dst_path);
            *pair_counts.entry(key).or_insert(0) += 1;
        }
    }

    // Filter by minimum co-change threshold and store edges
    let mut couplings = Vec::new();

    // First, delete existing change_coupling edges for this project
    conn.execute(
        "DELETE FROM edges WHERE project_id = ?1 AND kind = 'change_coupling'",
        rusqlite::params![project_id],
    )?;

    for ((src_id, dst_id, src_path, dst_path), count) in &pair_counts {
        if *count >= min_co_changes {
            let metadata = serde_json::json!({"co_changes": count}).to_string();

            conn.execute(
                "INSERT OR REPLACE INTO edges (project_id, source_node_id, target_node_id, kind, metadata)
                 VALUES (?1, ?2, ?3, 'change_coupling', ?4)",
                rusqlite::params![project_id, src_id, dst_id, metadata],
            )?;

            couplings.push(ChangeCoupling {
                source_file: src_path.clone(),
                target_file: dst_path.clone(),
                co_change_count: *count,
                source_node_id: *src_id,
                target_node_id: *dst_id,
            });
        }
    }

    Ok(couplings)
}

/// Get all change couplings for a given node (bidirectional).
pub fn get_couplings_for_node(conn: &Connection, node_id: i64) -> Result<Vec<CouplingEdge>> {
    let mut edges = Vec::new();

    // Outgoing: source = node_id
    {
        let mut stmt = conn.prepare(
            "SELECT target_node_id, metadata FROM edges
             WHERE source_node_id = ?1 AND kind = 'change_coupling'",
        )?;
        let rows = stmt.query_map(rusqlite::params![node_id], |row| {
            let target_id: i64 = row.get(0)?;
            let meta: Option<String> = row.get(1)?;
            Ok((target_id, meta))
        })?;
        for row in rows.flatten() {
            let co_changes = parse_co_changes(&row.1);
            edges.push(CouplingEdge {
                node_id,
                coupled_node_id: row.0,
                co_change_count: co_changes,
            });
        }
    }

    // Incoming: target = node_id
    {
        let mut stmt = conn.prepare(
            "SELECT source_node_id, metadata FROM edges
             WHERE target_node_id = ?1 AND kind = 'change_coupling'",
        )?;
        let rows = stmt.query_map(rusqlite::params![node_id], |row| {
            let source_id: i64 = row.get(0)?;
            let meta: Option<String> = row.get(1)?;
            Ok((source_id, meta))
        })?;
        for row in rows.flatten() {
            let co_changes = parse_co_changes(&row.1);
            // Avoid duplicates
            if !edges.iter().any(|e| e.coupled_node_id == row.0) {
                edges.push(CouplingEdge {
                    node_id,
                    coupled_node_id: row.0,
                    co_change_count: co_changes,
                });
            }
        }
    }

    Ok(edges)
}

/// Get all change couplings for a given node, returning full Node info.
pub fn get_coupled_nodes(conn: &Connection, node_id: i64) -> Result<Vec<(Node, i64)>> {
    let couplings = get_couplings_for_node(conn, node_id)?;
    let mut result = Vec::new();

    for c in couplings {
        if let Ok(Some(node)) = crate::db::queries::get_node_by_id(conn, c.coupled_node_id) {
            result.push((node, c.co_change_count));
        }
    }

    Ok(result)
}

/// Get the maximum co-change count across the project (for normalization).
pub fn get_max_co_changes(conn: &Connection, project_id: i64) -> Result<i64> {
    let max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(CAST(json_extract(metadata, '$.co_changes') AS INTEGER)), 0)
         FROM edges
         WHERE project_id = ?1 AND kind = 'change_coupling'",
        rusqlite::params![project_id],
        |row| row.get(0),
    )?;
    Ok(max)
}

/// Get the number of distinct coupled files for a node.
pub fn get_coupling_count(conn: &Connection, node_id: i64) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edges
         WHERE (source_node_id = ?1 OR target_node_id = ?1) AND kind = 'change_coupling'",
        rusqlite::params![node_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Get the total evolutionary-coupling strength for a node: the sum of co-change
/// frequencies across all its coupled files. This is the frequency signal used by
/// the risk model (how often this file changes together with others), not just the
/// count of distinct coupled neighbors.
pub fn get_coupling_strength(conn: &Connection, node_id: i64) -> Result<i64> {
    let edges = get_couplings_for_node(conn, node_id)?;
    let total: i64 = edges.iter().map(|e| e.co_change_count).sum();
    Ok(total)
}

fn parse_co_changes(meta: &Option<String>) -> i64 {
    meta.as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("co_changes")?.as_i64())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_co_changes_valid() {
        let meta = Some(r#"{"co_changes": 5}"#.to_string());
        assert_eq!(parse_co_changes(&meta), 5);
    }

    #[test]
    fn test_parse_co_changes_none() {
        assert_eq!(parse_co_changes(&None), 1);
    }

    #[test]
    fn test_parse_co_changes_invalid() {
        assert_eq!(parse_co_changes(&Some("not json".to_string())), 1);
    }
}
