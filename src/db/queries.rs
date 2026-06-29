use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, Connection};

use super::schema::*;

// ── Projects ──

pub fn upsert_project(conn: &Connection, name: &str, root_path: &str) -> Result<Project> {
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES (?1, ?2)
         ON CONFLICT(name) DO UPDATE SET root_path = ?2, indexed_at = datetime('now')",
        params![name, root_path],
    )?;
    Ok(conn.query_row(
        "SELECT id, name, root_path, indexed_at, node_count, edge_count, config FROM projects WHERE name = ?1",
        params![name],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                root_path: row.get(2)?,
                indexed_at: row.get(3)?,
                node_count: row.get(4)?,
                edge_count: row.get(5)?,
                config: row.get(6)?,
            })
        },
    )?)
}

pub fn get_project(conn: &Connection, name: &str) -> Result<Option<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, root_path, indexed_at, node_count, edge_count, config
         FROM projects WHERE name = ?1",
    )?;
    let mut rows = stmt.query(params![name])?;
    match rows.next()? {
        Some(row) => Ok(Some(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            root_path: row.get(2)?,
            indexed_at: row.get(3)?,
            node_count: row.get(4)?,
            edge_count: row.get(5)?,
            config: row.get(6)?,
        })),
        None => Ok(None),
    }
}

pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, root_path, indexed_at, node_count, edge_count, config
         FROM projects ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            root_path: row.get(2)?,
            indexed_at: row.get(3)?,
            node_count: row.get(4)?,
            edge_count: row.get(5)?,
            config: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete_project(conn: &Connection, project_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM nodes WHERE project_id = ?1",
        params![project_id],
    )?;
    conn.execute("DELETE FROM projects WHERE id = ?1", params![project_id])?;
    Ok(())
}

pub fn update_project_counts(conn: &Connection, project_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE projects SET
            node_count = (SELECT COUNT(*) FROM nodes WHERE project_id = ?1),
            edge_count = (SELECT COUNT(*) FROM edges WHERE project_id = ?1)
         WHERE id = ?1",
        params![project_id],
    )?;
    Ok(())
}

// ── Nodes ──

pub fn insert_node(conn: &Connection, project_id: i64, node: &Node) -> Result<i64> {
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind, name, qualified_name, signature,
            doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            project_id,
            node.file_path,
            node.kind,
            node.name,
            node.qualified_name,
            node.signature,
            node.doc_comment,
            node.start_line,
            node.end_line,
            node.complexity,
            node.is_exported,
            node.content_hash,
            node.source,
            node.metadata,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_node_by_id(conn: &Connection, id: i64) -> Result<Option<Node>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
            doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
            created_at, updated_at
         FROM nodes WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_node(row)?)),
        None => Ok(None),
    }
}

pub fn get_nodes_by_file(conn: &Connection, project_id: i64, file_path: &str) -> Result<Vec<Node>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
            doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
            created_at, updated_at
         FROM nodes WHERE project_id = ?1 AND file_path = ?2
         ORDER BY start_line",
    )?;
    let rows = stmt.query_map(params![project_id, file_path], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn search_nodes_by_name(
    conn: &Connection,
    project_id: i64,
    pattern: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Node>> {
    let like_pattern = format!("%{}%", pattern);
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
            doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
            created_at, updated_at
         FROM nodes
         WHERE project_id = ?1 AND (name LIKE ?2 OR qualified_name LIKE ?2)
         ORDER BY CASE
            WHEN name = ?3 THEN 0
            WHEN name LIKE ?4 THEN 1
            WHEN qualified_name LIKE ?2 THEN 2
            ELSE 3
         END, name
         LIMIT ?5 OFFSET ?6",
    )?;
    let rows = stmt.query_map(
        params![
            project_id,
            like_pattern,
            pattern,
            format!("{}%", pattern),
            limit,
            offset
        ],
        row_to_node,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn fts_search(
    conn: &Connection,
    project_id: i64,
    query: &str,
    limit: i64,
) -> Result<Vec<(Node, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
            n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported, n.content_hash,
            n.source, n.metadata, n.created_at, n.updated_at,
            bm25(nodes_fts)
         FROM nodes_fts f
         JOIN nodes n ON n.id = f.rowid
         WHERE n.project_id = ?1 AND nodes_fts MATCH ?2
         ORDER BY bm25(nodes_fts)
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![project_id, query, limit], |row| {
        let node = row_to_node(row)?;
        let score: f64 = row.get(17)?;
        Ok((node, score))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete_nodes_by_file(conn: &Connection, project_id: i64, file_path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM edges WHERE source_node_id IN (SELECT id FROM nodes WHERE project_id = ?1 AND file_path = ?2)
         OR target_node_id IN (SELECT id FROM nodes WHERE project_id = ?1 AND file_path = ?2)",
        params![project_id, file_path],
    )?;
    conn.execute(
        "DELETE FROM nodes WHERE project_id = ?1 AND file_path = ?2",
        params![project_id, file_path],
    )?;
    Ok(())
}

pub fn get_all_nodes(conn: &Connection, project_id: i64) -> Result<Vec<Node>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
            doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
            created_at, updated_at
         FROM nodes WHERE project_id = ?1",
    )?;
    let rows = stmt.query_map(params![project_id], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_file_structure(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
) -> Result<Vec<Node>> {
    // Try exact match first, then suffix match (handles both absolute and relative paths)
    let nodes = get_nodes_by_file(conn, project_id, file_path)?;
    if !nodes.is_empty() {
        return Ok(nodes);
    }
    // Fallback: match by suffix (e.g. "src/data/dataset_preparation.py" matches "C:\...\src\data\dataset_preparation.py")
    let like_pattern = format!("%{}", file_path.replace('\\', "/"));
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
            doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
            created_at, updated_at
         FROM nodes WHERE project_id = ?1 AND REPLACE(file_path, '\\', '/') LIKE ?2
         ORDER BY start_line",
    )?;
    let rows = stmt.query_map(params![project_id, like_pattern], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

// ── Edges ──

pub fn insert_edge(
    conn: &Connection,
    project_id: i64,
    source_id: i64,
    target_id: i64,
    kind: &str,
    metadata: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO edges (project_id, source_node_id, target_node_id, kind, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![project_id, source_id, target_id, kind, metadata],
    )?;
    Ok(())
}

pub fn get_edges_by_source(
    conn: &Connection,
    node_id: i64,
    kind: Option<&str>,
) -> Result<Vec<(Edge, Node)>> {
    let sql = match kind {
        Some(_) => "SELECT e.id, e.project_id, e.source_node_id, e.target_node_id, e.kind, e.metadata,
                        n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                        n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported,
                        n.content_hash, n.source, n.metadata, n.created_at, n.updated_at
                     FROM edges e JOIN nodes n ON n.id = e.target_node_id
                     WHERE e.source_node_id = ?1 AND e.kind = ?2",
            None => "SELECT e.id, e.project_id, e.source_node_id, e.target_node_id, e.kind, e.metadata,
                        n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                        n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported,
                        n.content_hash, n.source, n.metadata, n.created_at, n.updated_at
                     FROM edges e JOIN nodes n ON n.id = e.target_node_id
                     WHERE e.source_node_id = ?1",
        };
    let mut stmt = conn.prepare(sql)?;
    let rows = match kind {
        Some(k) => stmt.query_map(params![node_id, k], row_to_edge_with_target)?,
        None => stmt.query_map(params![node_id], row_to_edge_with_target)?,
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_edges_by_target(
    conn: &Connection,
    node_id: i64,
    kind: Option<&str>,
) -> Result<Vec<(Edge, Node)>> {
    let sql = match kind {
        Some(_) => "SELECT e.id, e.project_id, e.source_node_id, e.target_node_id, e.kind, e.metadata,
                        n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                        n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported,
                        n.content_hash, n.source, n.metadata, n.created_at, n.updated_at
                     FROM edges e JOIN nodes n ON n.id = e.source_node_id
                     WHERE e.target_node_id = ?1 AND e.kind = ?2",
            None => "SELECT e.id, e.project_id, e.source_node_id, e.target_node_id, e.kind, e.metadata,
                        n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                        n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported,
                        n.content_hash, n.source, n.metadata, n.created_at, n.updated_at
                     FROM edges e JOIN nodes n ON n.id = e.source_node_id
                     WHERE e.target_node_id = ?1",
        };
    let mut stmt = conn.prepare(sql)?;
    let rows = match kind {
        Some(k) => stmt.query_map(params![node_id, k], row_to_edge_with_source)?,
        None => stmt.query_map(params![node_id], row_to_edge_with_source)?,
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_call_graph(
    conn: &Connection,
    node_id: i64,
    direction: &str,
    max_depth: i64,
) -> Result<Vec<Node>> {
    let sql = match direction {
        "callers" => "WITH RECURSIVE callers AS (
            SELECT e.source_node_id AS id, 1 AS depth FROM edges e WHERE e.target_node_id = ?1 AND e.kind = 'calls'
            UNION ALL
            SELECT e.source_node_id, c.depth + 1 FROM edges e
            JOIN callers c ON e.target_node_id = c.id AND e.kind = 'calls'
            WHERE c.depth < ?2
        )
        SELECT DISTINCT n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
            n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported, n.content_hash,
            n.source, n.metadata, n.created_at, n.updated_at
        FROM callers c JOIN nodes n ON n.id = c.id",
        _ => "WITH RECURSIVE callees AS (
            SELECT e.target_node_id AS id, 1 AS depth FROM edges e WHERE e.source_node_id = ?1 AND e.kind = 'calls'
            UNION ALL
            SELECT e.target_node_id, c.depth + 1 FROM edges e
            JOIN callees c ON e.source_node_id = c.id AND e.kind = 'calls'
            WHERE c.depth < ?2
        )
        SELECT DISTINCT n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
            n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported, n.content_hash,
            n.source, n.metadata, n.created_at, n.updated_at
        FROM callees c JOIN nodes n ON n.id = c.id",
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![node_id, max_depth], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn find_path(conn: &Connection, from_id: i64, to_id: i64, max_depth: i64) -> Result<Vec<Edge>> {
    let sql = "
        WITH RECURSIVE path AS (
            SELECT e.id, e.source_node_id, e.target_node_id, e.kind, e.metadata, e.created_at,
                   1 AS depth, printf('%d', e.source_node_id) AS trail
            FROM edges e WHERE e.source_node_id = ?1 AND e.kind = 'calls'
            UNION ALL
            SELECT e.id, e.source_node_id, e.target_node_id, e.kind, e.metadata, e.created_at,
                   p.depth + 1, p.trail || ',' || e.source_node_id
            FROM edges e JOIN path p ON e.source_node_id = p.target_node_id AND e.kind = 'calls'
            WHERE p.depth < ?3 AND instr(p.trail, printf('%d', e.source_node_id)) = 0
        )
        SELECT id, source_node_id, target_node_id, kind, metadata, created_at FROM path
        WHERE target_node_id = ?2
        LIMIT 1";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![from_id, to_id, max_depth], |row| {
        Ok(Edge {
            id: row.get(0)?,
            project_id: 0,
            source_node_id: row.get(1)?,
            target_node_id: row.get(2)?,
            kind: row.get(3)?,
            metadata: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn find_dead_code(conn: &Connection, project_id: i64) -> Result<Vec<Node>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
            n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported, n.content_hash,
            n.source, n.metadata, n.created_at, n.updated_at
         FROM nodes n
         WHERE n.project_id = ?1
           AND n.kind IN ('function', 'method')
           AND n.id NOT IN (
               SELECT e.target_node_id FROM edges e WHERE e.kind = 'calls'
           )
           AND n.name != 'main'
        ORDER BY n.file_path, n.start_line",
    )?;
    let rows = stmt.query_map(params![project_id], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

// ── Memory Notes ──

pub fn insert_memory_note(
    conn: &Connection,
    project_id: i64,
    content: &str,
    node_id: Option<i64>,
    kind: &str,
    tags: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO memory_notes (project_id, content, node_id, kind, tags)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![project_id, content, node_id, kind, tags],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn search_memory_notes(
    conn: &Connection,
    project_id: i64,
    query: &str,
    limit: i64,
) -> Result<Vec<MemoryNote>> {
    let pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT id, project_id, content, node_id, kind, tags, access_count, last_accessed, created_at, updated_at
         FROM memory_notes
         WHERE project_id = ?1 AND (content LIKE ?2 OR tags LIKE ?2)
         ORDER BY access_count DESC, last_accessed DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![project_id, pattern, limit], |row| {
        Ok(MemoryNote {
            id: row.get(0)?,
            project_id: row.get(1)?,
            content: row.get(2)?,
            node_id: row.get(3)?,
            kind: row.get(4)?,
            tags: row.get(5)?,
            access_count: row.get(6)?,
            last_accessed: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn list_memory_notes(
    conn: &Connection,
    project_id: i64,
    kind: Option<&str>,
) -> Result<Vec<MemoryNote>> {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match kind {
        Some(k) => (
            "SELECT id, project_id, content, node_id, kind, tags, access_count, last_accessed, created_at, updated_at
             FROM memory_notes WHERE project_id = ?1 AND kind = ?2
             ORDER BY created_at DESC".to_string(),
            vec![Box::new(project_id), Box::new(k.to_string())],
        ),
        None => (
            "SELECT id, project_id, content, node_id, kind, tags, access_count, last_accessed, created_at, updated_at
             FROM memory_notes WHERE project_id = ?1
             ORDER BY created_at DESC".to_string(),
            vec![Box::new(project_id)],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(MemoryNote {
            id: row.get(0)?,
            project_id: row.get(1)?,
            content: row.get(2)?,
            node_id: row.get(3)?,
            kind: row.get(4)?,
            tags: row.get(5)?,
            access_count: row.get(6)?,
            last_accessed: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete_memory_note(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM memory_notes WHERE id = ?1", params![id])?;
    Ok(())
}

// ── File States ──

pub fn get_all_file_states(
    conn: &Connection,
    project_id: i64,
) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT file_path, content_hash FROM file_states WHERE project_id = ?1")?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for r in rows {
        let (path, hash) = r?;
        map.insert(path, hash);
    }
    Ok(map)
}

pub fn get_file_state_hash(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
) -> Result<Option<String>> {
    let mut stmt = conn
        .prepare("SELECT content_hash FROM file_states WHERE project_id = ?1 AND file_path = ?2")?;
    let mut rows = stmt.query(params![project_id, file_path])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn upsert_file_state(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    content_hash: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO file_states (project_id, file_path, content_hash, mtime, last_indexed)
         VALUES (?1, ?2, ?3, datetime('now'), datetime('now'))",
        params![project_id, file_path, content_hash],
    )?;
    Ok(())
}

pub fn delete_file_state(conn: &Connection, project_id: i64, file_path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM file_states WHERE project_id = ?1 AND file_path = ?2",
        params![project_id, file_path],
    )?;
    Ok(())
}

// ── Embeddings ──

pub fn upsert_embedding(
    conn: &Connection,
    node_id: i64,
    embedding: &[f32],
    model: &str,
) -> Result<()> {
    let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute(
        "INSERT OR REPLACE INTO embeddings (node_id, embedding, model, dimensions)
         VALUES (?1, ?2, ?3, ?4)",
        params![node_id, bytes, model, embedding.len() as i64],
    )?;
    Ok(())
}

pub fn get_embedding(conn: &Connection, node_id: i64) -> Result<Option<Vec<f32>>> {
    let mut stmt =
        conn.prepare("SELECT embedding, dimensions FROM embeddings WHERE node_id = ?1")?;
    let mut rows = stmt.query(params![node_id])?;
    match rows.next()? {
        Some(row) => {
            let bytes: Vec<u8> = row.get(0)?;
            let _dims: usize = row.get::<_, i64>(1)? as usize;
            let floats: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            Ok(Some(floats))
        }
        None => Ok(None),
    }
}

pub fn get_all_embeddings(conn: &Connection, project_id: i64) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut stmt = conn.prepare(
        "SELECT e.node_id, e.embedding, e.dimensions
         FROM embeddings e
         JOIN nodes n ON n.id = e.node_id
         WHERE n.project_id = ?1",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        let node_id: i64 = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        let floats: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Ok((node_id, floats))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn count_embeddings(conn: &Connection, project_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM embeddings e JOIN nodes n ON n.id = e.node_id WHERE n.project_id = ?1",
        params![project_id],
        |row| row.get(0),
    ).map_err(Into::into)
}

// ── Stats ──

pub fn get_project_stats(conn: &Connection, project_id: i64) -> Result<HashMap<String, i64>> {
    let mut stats = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT kind, COUNT(*) FROM nodes WHERE project_id = ?1 GROUP BY kind ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (kind, count) = row?;
        stats.insert(kind, count);
    }
    stats.insert("total_nodes".into(), stats.values().sum());
    stats.insert(
        "total_edges".into(),
        conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?,
    );
    stats.insert(
        "total_files".into(),
        conn.query_row(
            "SELECT COUNT(DISTINCT file_path) FROM nodes WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?,
    );
    Ok(stats)
}

// ── Row mappers ──

fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<Node> {
    Ok(Node {
        id: row.get(0)?,
        project_id: row.get(1)?,
        file_path: row.get(2)?,
        kind: row.get(3)?,
        name: row.get(4)?,
        qualified_name: row.get(5)?,
        signature: row.get(6)?,
        doc_comment: row.get(7)?,
        start_line: row.get(8)?,
        end_line: row.get(9)?,
        complexity: row.get(10)?,
        is_exported: row.get(11)?,
        content_hash: row.get(12)?,
        source: row.get(13)?,
        metadata: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn row_to_node_offset(row: &rusqlite::Row, offset: usize) -> rusqlite::Result<Node> {
    Ok(Node {
        id: row.get(offset)?,
        project_id: row.get(offset + 1)?,
        file_path: row.get(offset + 2)?,
        kind: row.get(offset + 3)?,
        name: row.get(offset + 4)?,
        qualified_name: row.get(offset + 5)?,
        signature: row.get(offset + 6)?,
        doc_comment: row.get(offset + 7)?,
        start_line: row.get(offset + 8)?,
        end_line: row.get(offset + 9)?,
        complexity: row.get(offset + 10)?,
        is_exported: row.get(offset + 11)?,
        content_hash: row.get(offset + 12)?,
        source: row.get(offset + 13)?,
        metadata: row.get(offset + 14)?,
        created_at: row.get(offset + 15)?,
        updated_at: row.get(offset + 16)?,
    })
}

pub fn get_route_map(conn: &Connection, project_id: i64) -> Result<Vec<(Node, Option<Node>)>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.project_id, r.file_path, r.kind, r.name, r.qualified_name, r.signature,
                r.doc_comment, r.start_line, r.end_line, r.complexity, r.is_exported, r.content_hash,
                r.source, r.metadata, r.created_at, r.updated_at,
                f.id, f.project_id, f.file_path, f.kind, f.name, f.qualified_name, f.signature,
                f.doc_comment, f.start_line, f.end_line, f.complexity, f.is_exported, f.content_hash,
                f.source, f.metadata, f.created_at, f.updated_at
         FROM nodes r
         LEFT JOIN edges e ON e.target_node_id = r.id AND e.kind = 'handles'
         LEFT JOIN nodes f ON f.id = e.source_node_id
         WHERE r.project_id = ?1 AND r.kind = 'route'
         ORDER BY r.name"
    )?;

    let rows = stmt.query_map(params![project_id], |row| {
        let route = row_to_node_offset(row, 0)?;

        let has_handler = row.get::<_, Option<i64>>(17)?.is_some();
        let handler = if has_handler {
            Some(row_to_node_offset(row, 17)?)
        } else {
            None
        };

        Ok((route, handler))
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn row_to_edge_with_target(row: &rusqlite::Row) -> rusqlite::Result<(Edge, Node)> {
    let edge = Edge {
        id: row.get(0)?,
        project_id: row.get(1)?,
        source_node_id: row.get(2)?,
        target_node_id: row.get(3)?,
        kind: row.get(4)?,
        metadata: row.get(5)?,
    };
    let _target_fields: Vec<usize> = (6..23).collect();
    let node = Node {
        id: row.get(6)?,
        project_id: row.get(7)?,
        file_path: row.get(8)?,
        kind: row.get(9)?,
        name: row.get(10)?,
        qualified_name: row.get(11)?,
        signature: row.get(12)?,
        doc_comment: row.get(13)?,
        start_line: row.get(14)?,
        end_line: row.get(15)?,
        complexity: row.get(16)?,
        is_exported: row.get(17)?,
        content_hash: row.get(18)?,
        source: row.get(19)?,
        metadata: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    };
    Ok((edge, node))
}

fn row_to_edge_with_source(row: &rusqlite::Row) -> rusqlite::Result<(Edge, Node)> {
    let edge = Edge {
        id: row.get(0)?,
        project_id: row.get(1)?,
        source_node_id: row.get(2)?,
        target_node_id: row.get(3)?,
        kind: row.get(4)?,
        metadata: row.get(5)?,
    };
    let node = Node {
        id: row.get(6)?,
        project_id: row.get(7)?,
        file_path: row.get(8)?,
        kind: row.get(9)?,
        name: row.get(10)?,
        qualified_name: row.get(11)?,
        signature: row.get(12)?,
        doc_comment: row.get(13)?,
        start_line: row.get(14)?,
        end_line: row.get(15)?,
        complexity: row.get(16)?,
        is_exported: row.get(17)?,
        content_hash: row.get(18)?,
        source: row.get(19)?,
        metadata: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    };
    Ok((edge, node))
}

// ── Semantic Changes ──

pub fn insert_semantic_change(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    node_name: Option<&str>,
    node_kind: &str,
    change_summary: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO semantic_changes (project_id, file_path, node_name, node_kind, change_summary)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![project_id, file_path, node_name, node_kind, change_summary],
    )?;
    Ok(())
}

pub fn get_recent_semantic_changes(
    conn: &Connection,
    project_id: i64,
    hours: i64,
) -> Result<Vec<crate::db::schema::SemanticChange>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, node_name, node_kind, change_summary, changed_at
         FROM semantic_changes
         WHERE project_id = ?1
           AND changed_at >= datetime('now', printf('-%d hours', ?2))
         ORDER BY changed_at DESC LIMIT 200",
    )?;
    let rows = stmt.query_map(params![project_id, hours], |row| {
        Ok(crate::db::schema::SemanticChange {
            id: row.get(0)?,
            project_id: row.get(1)?,
            file_path: row.get(2)?,
            node_name: row.get(3)?,
            node_kind: row.get(4)?,
            change_summary: row.get(5)?,
            changed_at: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

// ── Node Access Log ──

pub fn record_node_access(conn: &Connection, node_id: i64, project_id: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO node_access_log (node_id, project_id, access_count, last_accessed)
         VALUES (?1, ?2, 1, datetime('now'))
         ON CONFLICT(node_id, project_id) DO UPDATE SET
           access_count = access_count + 1,
           last_accessed = datetime('now')",
        params![node_id, project_id],
    )?;
    Ok(())
}

pub fn get_working_set(conn: &Connection, project_id: i64, limit: i64) -> Result<Vec<(Node, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported, n.content_hash,
                n.source, n.metadata, n.created_at, n.updated_at,
                nal.access_count
         FROM node_access_log nal
         JOIN nodes n ON n.id = nal.node_id
         WHERE nal.project_id = ?1
           AND nal.last_accessed >= datetime('now', '-24 hours')
         ORDER BY
           CAST(nal.access_count AS REAL) /
           (1.0 + (julianday('now') - julianday(nal.last_accessed)) * 24.0) DESC
         LIMIT ?2"
    )?;
    let rows = stmt.query_map(params![project_id, limit], |row| {
        let node = row_to_node(row)?;
        let access_count: i64 = row.get(17)?;
        Ok((node, access_count))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

// ── Cross-Project ──

pub fn find_cross_project_symbol(conn: &Connection, symbol_name: &str) -> Result<Vec<Node>> {
    let pattern = format!("%::{}", symbol_name);
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
                doc_comment, start_line, end_line, complexity, is_exported, content_hash,
                source, metadata, created_at, updated_at
         FROM nodes
         WHERE name = ?1 OR qualified_name LIKE ?2
         ORDER BY project_id, name",
    )?;
    let rows = stmt.query_map(params![symbol_name, pattern], row_to_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn link_cross_project(
    conn: &Connection,
    project_id: i64,
    source_node_id: i64,
    target_node_id: i64,
) -> Result<()> {
    insert_edge(
        conn,
        project_id,
        source_node_id,
        target_node_id,
        "cross_project",
        None,
    )
}

// ── Architecture Decision Records (ADRs) ──

pub fn insert_adr(
    conn: &Connection,
    project_id: i64,
    title: &str,
    status: &str,
    context: &str,
    decision: &str,
    consequences: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO adrs (project_id, title, status, context, decision, consequences)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![project_id, title, status, context, decision, consequences],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_adr(conn: &Connection, adr_id: i64) -> Result<Option<crate::db::schema::Adr>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, status, context, decision, consequences, created_at, updated_at
         FROM adrs WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![adr_id], |row| {
        Ok(crate::db::schema::Adr {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            context: row.get(4)?,
            decision: row.get(5)?,
            consequences: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    if let Some(r) = rows.next() {
        Ok(Some(r?))
    } else {
        Ok(None)
    }
}

pub fn list_adrs(conn: &Connection, project_id: i64) -> Result<Vec<crate::db::schema::Adr>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, status, context, decision, consequences, created_at, updated_at
         FROM adrs WHERE project_id = ?1 ORDER BY id DESC"
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(crate::db::schema::Adr {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            context: row.get(4)?,
            decision: row.get(5)?,
            consequences: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn update_adr(
    conn: &Connection,
    adr_id: i64,
    title: &str,
    status: &str,
    context: &str,
    decision: &str,
    consequences: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE adrs
         SET title = ?1, status = ?2, context = ?3, decision = ?4, consequences = ?5, updated_at = datetime('now')
         WHERE id = ?6",
        params![title, status, context, decision, consequences, adr_id],
    )?;
    Ok(())
}

pub fn delete_adr(conn: &Connection, adr_id: i64) -> Result<()> {
    conn.execute("DELETE FROM adrs WHERE id = ?1", params![adr_id])?;
    Ok(())
}

// ── PageRank ──

/// Returns all (source_node_id, target_node_id) edges for PageRank computation.
pub fn get_all_import_edges(conn: &Connection, project_id: i64) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT source_node_id, target_node_id
         FROM edges
         WHERE project_id = ?1 AND kind IN ('imports', 'calls', 'references', 'depends_on')",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Returns all node IDs for a project.
pub fn get_all_node_ids(conn: &Connection, project_id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM nodes WHERE project_id = ?1")?;
    let rows = stmt.query_map(params![project_id], |row| row.get::<_, i64>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Upsert the PageRank score for a single node.
pub fn update_node_pagerank(
    conn: &Connection,
    node_id: i64,
    project_id: i64,
    rank: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO node_pagerank (node_id, project_id, pagerank, computed_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(node_id) DO UPDATE SET pagerank = ?3, computed_at = datetime('now')",
        params![node_id, project_id, rank],
    )?;
    Ok(())
}

/// Get the PageRank score for a node (returns 0.0 if not yet computed).
pub fn get_node_pagerank(conn: &Connection, node_id: i64) -> Result<f64> {
    match conn.query_row(
        "SELECT pagerank FROM node_pagerank WHERE node_id = ?1",
        params![node_id],
        |row| row.get::<_, f64>(0),
    ) {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0.0),
        Err(e) => Err(e.into()),
    }
}

/// Get top-N nodes by PageRank for a project.
pub fn get_top_pagerank_nodes(
    conn: &Connection,
    project_id: i64,
    limit: i64,
) -> Result<Vec<(i64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT node_id, pagerank FROM node_pagerank
         WHERE project_id = ?1
         ORDER BY pagerank DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![project_id, limit], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Load previous PageRank scores for warm-start.
pub fn get_prev_pageranks(
    conn: &Connection,
    project_id: i64,
) -> Result<std::collections::HashMap<i64, f64>> {
    let mut stmt =
        conn.prepare("SELECT node_id, pagerank FROM node_pagerank WHERE project_id = ?1")?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;
    rows.collect::<Result<std::collections::HashMap<_, _>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::migrate;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn insert_test_node(
        conn: &Connection,
        project_id: i64,
        name: &str,
        kind: &str,
        file_path: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO nodes (project_id, file_path, kind, name) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, file_path, kind, name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn test_upsert_and_get_project() {
        let conn = test_conn();
        let p = upsert_project(&conn, "test-proj", "/tmp/test").unwrap();
        assert_eq!(p.name, "test-proj");
        assert_eq!(p.root_path, "/tmp/test");

        let fetched = get_project(&conn, "test-proj").unwrap().unwrap();
        assert_eq!(fetched.id, p.id);
        assert_eq!(fetched.name, "test-proj");
    }

    #[test]
    fn test_get_project_not_found() {
        let conn = test_conn();
        let result = get_project(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_projects() {
        let conn = test_conn();
        upsert_project(&conn, "a", "/a").unwrap();
        upsert_project(&conn, "b", "/b").unwrap();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn test_delete_project() {
        let conn = test_conn();
        let p = upsert_project(&conn, "del-me", "/x").unwrap();
        delete_project(&conn, p.id).unwrap();
        assert!(get_project(&conn, "del-me").unwrap().is_none());
    }

    #[test]
    fn test_insert_and_get_node() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let nid = insert_test_node(&conn, p.id, "hello", "function", "main.rs");
        let node = get_node_by_id(&conn, nid).unwrap().unwrap();
        assert_eq!(node.name.unwrap(), "hello");
        assert_eq!(node.kind, "function");
    }

    #[test]
    fn test_insert_and_get_edge() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let n1 = insert_test_node(&conn, p.id, "a", "function", "a.rs");
        let n2 = insert_test_node(&conn, p.id, "b", "function", "b.rs");
        insert_edge(&conn, p.id, n1, n2, "calls", None).unwrap();

        let t = crate::graph::GraphTraversal::new(&conn);
        let callees = t.find_callees(n1, 1).unwrap();
        assert!(callees.iter().any(|n| n.id == n2));
        let callers = t.find_callers(n2, 1).unwrap();
        assert!(callers.iter().any(|n| n.id == n1));
    }

    #[test]
    fn test_delete_nodes_by_file() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        insert_test_node(&conn, p.id, "x", "function", "file.rs");
        insert_test_node(&conn, p.id, "y", "function", "file.rs");
        delete_nodes_by_file(&conn, p.id, "file.rs").unwrap();
        let nodes = get_nodes_by_file(&conn, p.id, "file.rs").unwrap();
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_fts_search() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        conn.execute(
            "INSERT INTO nodes (project_id, file_path, kind, name, source) VALUES (?1, 'a.rs', 'function', 'hello_world', 'fn hello_world() {}')",
            params![p.id],
        ).unwrap();
        conn.execute(
            "INSERT INTO nodes_fts (rowid, name, source) VALUES (?1, 'hello_world', 'fn hello_world() {}')",
            params![conn.last_insert_rowid()],
        ).unwrap();

        let bm25 = crate::search::Bm25Search::new(&conn);
        let results = bm25.search(p.id, "hello", 10).unwrap();
        assert!(!results.is_empty(), "FTS5 should find 'hello_world'");
    }

    #[test]
    fn test_memory_crud() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let store = crate::memory::MemoryStore::new(&conn);
        let id = store
            .store(p.id, "important note", None, "note", None)
            .unwrap();
        assert!(id > 0);

        let results = store.search(p.id, "important", 10).unwrap();
        assert!(!results.is_empty());

        let list = store.list(p.id, Some("note")).unwrap();
        assert_eq!(list.len(), 1);

        store.delete(id).unwrap();
        let after = store.list(p.id, Some("note")).unwrap();
        assert!(after.is_empty());
    }

    #[test]
    fn test_adr_crud() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let id = insert_adr(&conn, p.id, "Test ADR", "proposed", "ctx", "dec", "cons").unwrap();
        assert!(id > 0);

        let adr = get_adr(&conn, id).unwrap().unwrap();
        assert_eq!(adr.title, "Test ADR");

        let list = list_adrs(&conn, p.id).unwrap();
        assert_eq!(list.len(), 1);

        update_adr(&conn, id, "Updated", "accepted", "ctx2", "dec2", "cons2").unwrap();
        let updated = get_adr(&conn, id).unwrap().unwrap();
        assert_eq!(updated.status, "accepted");

        delete_adr(&conn, id).unwrap();
        assert!(get_adr(&conn, id).unwrap().is_none());
    }

    #[test]
    fn test_pagerank_scoring() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let nid = insert_test_node(&conn, p.id, "hub", "function", "hub.rs");

        update_node_pagerank(&conn, nid, p.id, 0.5).unwrap();
        let score = get_node_pagerank(&conn, nid).unwrap();
        assert!((score - 0.5).abs() < 1e-9);

        let top = get_top_pagerank_nodes(&conn, p.id, 10).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, nid);
        assert!((top[0].1 - 0.5).abs() < 1e-9);

        // Test missing node returns 0.0
        let missing = get_node_pagerank(&conn, 999).unwrap();
        assert!((missing - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_cross_project_linking() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let n1 = insert_test_node(&conn, p.id, "a", "function", "a.rs");
        let n2 = insert_test_node(&conn, p.id, "b", "function", "b.rs");
        link_cross_project(&conn, p.id, n1, n2).unwrap();
        // Verify edge exists directly in DB
        let mut stmt = conn
            .prepare("SELECT kind FROM edges WHERE source_node_id = ?1 AND target_node_id = ?2")
            .unwrap();
        let kind: String = stmt.query_row(params![n1, n2], |row| row.get(0)).unwrap();
        assert_eq!(kind, "cross_project");
    }

    #[test]
    fn test_get_all_embeddings_empty() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let result = get_all_embeddings(&conn, p.id).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_count_embeddings() {
        let conn = test_conn();
        let p = upsert_project(&conn, "p", "/p").unwrap();
        let count = count_embeddings(&conn, p.id).unwrap();
        assert_eq!(count, 0);
    }
}
