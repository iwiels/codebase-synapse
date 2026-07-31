use anyhow::Result;
use regex::Regex;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::db::{self, schema::Node};

static CALL_RE: OnceLock<Regex> = OnceLock::new();
static KEYWORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn get_call_re() -> &'static Regex {
    CALL_RE.get_or_init(|| Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap())
}

fn get_keywords() -> &'static HashSet<&'static str> {
    KEYWORDS.get_or_init(|| {
        [
            "if",
            "while",
            "for",
            "switch",
            "catch",
            "match",
            "return",
            "let",
            "fn",
            "function",
            "impl",
            "struct",
            "class",
            "interface",
            "using",
            "import",
            "super",
            "this",
            "self",
            "assert",
            "println",
            "print",
            "expect",
            "unwrap",
            "panic",
            "log",
            "debug",
            "info",
            "warn",
            "error",
        ]
        .iter()
        .cloned()
        .collect()
    })
}

/// Determine the language of a source file from its extension.
fn language_of(file_path: &str) -> String {
    std::path::Path::new(file_path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// A lightweight call graph resolver (Hybrid LSP) that maps raw source function calls to database target nodes.
pub fn resolve_project_calls(conn: &Connection, project_id: i64) -> Result<()> {
    // 1. Delete all existing 'calls' edges for this project to rebuild the call graph
    conn.execute(
        "DELETE FROM edges WHERE project_id = ?1 AND kind = 'calls'",
        rusqlite::params![project_id],
    )?;

    // 2. Fetch all nodes in the project
    let mut stmt = conn.prepare(
        "SELECT id, file_path, kind, name, qualified_name, start_line, end_line, complexity, is_exported, source, metadata
         FROM nodes
         WHERE project_id = ?1"
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        Ok(Node {
            id: row.get(0)?,
            project_id,
            file_path: row.get(1)?,
            kind: row.get(2)?,
            name: row.get(3)?,
            qualified_name: row.get(4)?,
            signature: None,
            doc_comment: None,
            start_line: row.get(5)?,
            end_line: row.get(6)?,
            complexity: row.get(7)?,
            is_exported: row.get(8)?,
            content_hash: None,
            source: row.get(9)?,
            metadata: row.get(10)?,
            created_at: String::new(),
            updated_at: String::new(),
        })
    })?;

    let mut nodes = Vec::new();
    for r in rows {
        nodes.push(r?);
    }

    // Index functions/methods for fast lookup by name and path
    // Map: name -> Vec<Node> (global, used for uniqueness checks)
    let mut func_by_name: HashMap<String, Vec<&Node>> = HashMap::new();
    // Map: (file_path, name) -> Vec<Node> (same-file resolution)
    let mut func_by_file_name: HashMap<(String, String), Vec<&Node>> = HashMap::new();
    // Map: (language, name) -> Vec<Node> (cross-file resolution, language-aware)
    let mut func_by_lang_name: HashMap<(String, String), Vec<&Node>> = HashMap::new();

    for node in &nodes {
        if node.kind == "function" || node.kind == "method" {
            if let Some(ref name) = node.name {
                func_by_name.entry(name.clone()).or_default().push(node);
                func_by_file_name
                    .entry((node.file_path.clone(), name.clone()))
                    .or_default()
                    .push(node);
                func_by_lang_name
                    .entry((language_of(&node.file_path), name.clone()))
                    .or_default()
                    .push(node);
            }
        }
    }

    // 3. For each function/method node, find which other functions/methods it calls
    for source_node in &nodes {
        if source_node.kind != "function" && source_node.kind != "method" {
            continue;
        }

        let source_code = match &source_node.source {
            Some(src) => src,
            None => continue,
        };

        // Extract call names, tracking whether each is a method call
        // (`obj.method(...)`) vs a free function call (`name(...)`)
        let mut call_targets: HashSet<(String, bool)> = HashSet::new();
        for cap in get_call_re().captures_iter(source_code) {
            let name = cap.get(1).unwrap().as_str();
            if !get_keywords().contains(name) {
                let is_method = cap
                    .get(0)
                    .map(|m| m.start() > 0 && source_code[..m.start()].ends_with('.'))
                    .unwrap_or(false);
                call_targets.insert((name.to_string(), is_method));
            }
        }

        // For each call name, resolve the target node ID
        for (target_name, is_method) in call_targets {
            let mut resolved_id = None;

            if is_method {
                // Method calls resolve only within the same file to avoid
                // false positives like `.map()` on arrays or `.filter()` on
                // iterables.
                if let Some(nodes) = func_by_file_name
                    .get(&(source_node.file_path.clone(), target_name.clone()))
                {
                    if let Some(method) = nodes.iter().find(|n| n.kind == "method") {
                        resolved_id = Some(method.id);
                    }
                }
            } else {
                // Signal 1: Local file match (High priority)
                if let Some(nodes) = func_by_file_name
                    .get(&(source_node.file_path.clone(), target_name.clone()))
                {
                    let local_node = nodes
                        .iter()
                        .find(|n| n.kind == "function")
                        .or_else(|| nodes.first());
                    if let Some(local_node) = local_node {
                        resolved_id = Some(local_node.id);
                    }
                }

                // Signal 2: Directory proximity match (same language only)
                if resolved_id.is_none() {
                    let source_lang = language_of(&source_node.file_path);
                    if let Some(candidates) =
                        func_by_lang_name.get(&(source_lang, target_name.clone()))
                    {
                        let source_dir = std::path::Path::new(&source_node.file_path)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();

                        for cand in candidates {
                            let cand_dir = std::path::Path::new(&cand.file_path)
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();

                            if cand_dir == source_dir {
                                resolved_id = Some(cand.id);
                                break;
                            }
                        }
                    }
                }

                // Signal 3: Global match (Fallback) — only when the name is
                // unique across the whole project and the symbol is exported,
                // to avoid resolving common names to unrelated symbols.
                if resolved_id.is_none() {
                    if let Some(candidates) = func_by_name.get(&target_name) {
                        if candidates.len() == 1 && candidates[0].is_exported {
                            resolved_id = Some(candidates[0].id);
                        }
                    }
                }
            }

            // If we resolved a target, insert the 'calls' edge
            if let Some(target_id) = resolved_id {
                // Prevent self-recursion edges if we want clean call graphs (or allow them if required)
                if target_id != source_node.id {
                    db::queries::insert_edge(
                        conn,
                        project_id,
                        source_node.id,
                        target_id,
                        "calls",
                        None,
                    )?;
                }
            }
        }
    }

    Ok(())
}
