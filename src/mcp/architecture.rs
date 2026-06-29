use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashMap;

use crate::db::queries;

pub fn get_project_architecture(conn: &Connection, project_id: i64) -> Result<serde_json::Value> {
    // 1. Languages
    let mut stmt =
        conn.prepare("SELECT file_path FROM nodes WHERE project_id = ?1 AND kind = 'file'")?;
    let mut lang_counts: HashMap<String, usize> = HashMap::new();
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        let path: String = row.get(0)?;
        Ok(path)
    })?;
    for r in rows {
        let path = r?;
        let ext = path.split('.').next_back().unwrap_or("").to_lowercase();
        let lang = match ext.as_str() {
            "rs" => "Rust",
            "py" => "Python",
            "ts" | "tsx" => "TypeScript",
            "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
            "go" => "Go",
            "java" => "Java",
            "cs" => "C#",
            "php" => "PHP",
            "c" | "h" => "C",
            "cpp" | "cc" | "cxx" | "hpp" => "C++",
            "yaml" | "yml" => "YAML",
            "toml" => "TOML",
            "json" => "JSON",
            _ => "Unknown",
        };
        if lang != "Unknown" {
            *lang_counts.entry(lang.to_string()).or_default() += 1;
        }
    }

    // 2. Packages (extracted from manifests)
    let mut stmt = conn
        .prepare("SELECT name, metadata FROM nodes WHERE project_id = ?1 AND kind = 'manifest'")?;
    let mut packages = Vec::new();
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        let name: String = row.get(0)?;
        let metadata: Option<String> = row.get(1)?;
        Ok((name, metadata))
    })?;
    for r in rows {
        let (name, metadata) = r?;
        let pkg_name = metadata
            .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
            .and_then(|json| {
                json.get("package_name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or(name);
        packages.push(pkg_name);
    }

    // 3. Entry Points
    let mut stmt = conn.prepare(
        "SELECT DISTINCT file_path FROM nodes
         WHERE project_id = ?1 AND (
             name = 'main' OR
             file_path LIKE '%/main.rs' OR
             file_path LIKE '%/index.ts' OR
             file_path LIKE '%/index.js' OR
             file_path LIKE '%/app.py' OR
             file_path LIKE '%/server.ts' OR
             file_path LIKE '%/server.js'
         )",
    )?;
    let mut entry_points = Vec::new();
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        let path: String = row.get(0)?;
        Ok(path)
    })?;
    for r in rows {
        entry_points.push(r?);
    }

    // 4. Routes Map
    let route_map = queries::get_route_map(conn, project_id)?;
    let routes_json: Vec<serde_json::Value> = route_map
        .into_iter()
        .map(|(r, h)| {
            json!({
                "route": r,
                "handler": h
            })
        })
        .collect();

    // 5. Hotspots (Files with highest cumulative complexity or churn)
    let mut stmt = conn.prepare(
        "SELECT file_path, SUM(complexity) AS total_complexity, COUNT(*) AS symbol_count
         FROM nodes
         WHERE project_id = ?1 AND complexity IS NOT NULL
         GROUP BY file_path
         ORDER BY total_complexity DESC
         LIMIT 5",
    )?;
    let mut hotspots = Vec::new();
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        let file_path: String = row.get(0)?;
        let complexity: i64 = row.get(1)?;
        let symbols: i64 = row.get(2)?;
        Ok(json!({
            "file": file_path,
            "complexity": complexity,
            "symbols": symbols
        }))
    })?;
    for r in rows {
        hotspots.push(r?);
    }

    // 6. Dead Code Count
    let dead_code = queries::find_dead_code(conn, project_id)?;
    let dead_code_count = dead_code.len();

    // 7. Test Coverage status (files with tests vs files without)
    let mut stmt =
        conn.prepare("SELECT file_path FROM nodes WHERE project_id = ?1 AND kind = 'file'")?;
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        let path: String = row.get(0)?;
        Ok(path)
    })?;
    let mut files_with_tests = 0;
    let mut files_without_tests = 0;
    for r in rows {
        let p = r?;
        let p_lower = p.to_lowercase();
        if p_lower.contains("test") || p_lower.contains("spec") {
            files_with_tests += 1;
        } else {
            files_without_tests += 1;
        }
    }

    Ok(json!({
        "languages": lang_counts,
        "packages": packages,
        "entry_points": entry_points,
        "routes": routes_json,
        "hotspots": hotspots,
        "dead_code_count": dead_code_count,
        "test_coverage": {
            "files_with_tests": files_with_tests,
            "files_without_tests": files_without_tests
        }
    }))
}
