use crate::db::{self, schema::Node};
use anyhow::Result;
use regex::Regex;
use rusqlite::Connection;
use serde_json::Value as JsonValue;
use std::sync::LazyLock;
use toml::Value as TomlValue;

static MODULE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^module\s+(\S+)").unwrap());
static REQUIRE_GO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\S+)\s+(\S+)").unwrap());
static REQUIRE_TXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([a-zA-Z0-9_\-]+)\s*(?:[>=<~!]+(.*))?").unwrap());
static REQUIRE_TXT_PIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-zA-Z0-9_\-]+)\s*(?:[>=<~!]+(.*))?").unwrap());

pub fn is_manifest_file(file_path: &str) -> bool {
    let fp = file_path.to_lowercase().replace('\\', "/");
    fp.ends_with("/package.json")
        || fp.ends_with("/cargo.toml")
        || fp.ends_with("/go.mod")
        || fp.ends_with("/pyproject.toml")
        || fp.ends_with("/requirements.txt")
}

pub fn extract_and_insert_manifest(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    source: &str,
    file_node_id: i64,
) -> Result<()> {
    let filename = file_path.to_lowercase().replace('\\', "/");
    let name_only = filename.split('/').next_back().unwrap_or(file_path);

    let mut dependencies = Vec::new();
    let mut pkg_name = "manifest".to_string();

    if name_only == "package.json" {
        if let Ok(json) = serde_json::from_str::<JsonValue>(source) {
            if let Some(n) = json.get("name").and_then(|n| n.as_str()) {
                pkg_name = n.to_string();
            }
            // Add dependencies
            if let Some(deps) = json.get("dependencies").and_then(|d| d.as_object()) {
                for (dep, ver) in deps {
                    dependencies.push((dep.clone(), ver.as_str().unwrap_or("").to_string()));
                }
            }
            if let Some(dev_deps) = json.get("devDependencies").and_then(|d| d.as_object()) {
                for (dep, ver) in dev_deps {
                    dependencies.push((dep.clone(), ver.as_str().unwrap_or("").to_string()));
                }
            }
        }
    } else if name_only == "cargo.toml" {
        if let Ok(toml) = toml::from_str::<TomlValue>(source) {
            if let Some(package) = toml.get("package") {
                if let Some(n) = package.get("name").and_then(|n| n.as_str()) {
                    pkg_name = n.to_string();
                }
            }
            // Standard dependencies
            if let Some(deps) = toml.get("dependencies").and_then(|d| d.as_table()) {
                for (dep, val) in deps {
                    let version = match val {
                        TomlValue::String(s) => s.clone(),
                        TomlValue::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        _ => String::new(),
                    };
                    dependencies.push((dep.clone(), version));
                }
            }
            // Dev dependencies
            if let Some(deps) = toml.get("dev-dependencies").and_then(|d| d.as_table()) {
                for (dep, val) in deps {
                    let version = match val {
                        TomlValue::String(s) => s.clone(),
                        TomlValue::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        _ => String::new(),
                    };
                    dependencies.push((dep.clone(), version));
                }
            }
        }
    } else if name_only == "go.mod" {
        let lines: Vec<&str> = source.lines().collect();
        // Go module name
        for line in &lines {
            if let Some(cap) = MODULE_RE.captures(line) {
                pkg_name = cap.get(1).unwrap().as_str().to_string();
                break;
            }
        }
        // Requires
        let mut in_require = false;
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("require (") {
                in_require = true;
                continue;
            } else if trimmed.starts_with(")") {
                in_require = false;
                continue;
            }

            if trimmed.starts_with("require ") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 3 {
                    dependencies.push((parts[1].to_string(), parts[2].to_string()));
                }
            } else if in_require && !trimmed.is_empty() {
                if let Some(cap) = REQUIRE_GO_RE.captures(trimmed) {
                    dependencies.push((
                        cap.get(1).unwrap().as_str().to_string(),
                        cap.get(2).unwrap().as_str().to_string(),
                    ));
                }
            }
        }
    } else if name_only == "requirements.txt" {
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(cap) = REQUIRE_TXT_RE.captures(line) {
                let dep = cap.get(1).unwrap().as_str().to_string();
                let ver = cap
                    .get(2)
                    .map(|v| v.as_str().trim().to_string())
                    .unwrap_or_default();
                dependencies.push((dep, ver));
            }
        }
    } else if name_only == "pyproject.toml" {
        if let Ok(toml) = toml::from_str::<TomlValue>(source) {
            if let Some(project) = toml.get("project") {
                if let Some(n) = project.get("name").and_then(|n| n.as_str()) {
                    pkg_name = n.to_string();
                }
                // Dependencies list
                if let Some(deps) = project.get("dependencies").and_then(|d| d.as_array()) {
                    for d_val in deps {
                        if let Some(d_str) = d_val.as_str() {
                            if let Some(cap) = REQUIRE_TXT_PIN_RE.captures(d_str) {
                                let dep = cap.get(1).unwrap().as_str().to_string();
                                let ver = cap
                                    .get(2)
                                    .map(|v| v.as_str().trim().to_string())
                                    .unwrap_or_default();
                                dependencies.push((dep, ver));
                            }
                        }
                    }
                }
            }
        }
    }

    // Insert the manifest node
    let qn = format!("__manifest__{}__{}", name_only, pkg_name);
    let manifest_node = Node {
        id: 0,
        project_id,
        file_path: file_path.to_string(),
        kind: "manifest".to_string(),
        name: Some(name_only.to_string()),
        qualified_name: Some(qn),
        signature: None,
        doc_comment: None,
        start_line: 1,
        end_line: source.lines().count() as i64,
        complexity: None,
        is_exported: true,
        content_hash: None,
        source: None,
        metadata: Some(serde_json::json!({ "package_name": pkg_name, "dependencies_count": dependencies.len() }).to_string()),
        created_at: String::new(),
        updated_at: String::new(),
    };

    let manifest_node_id = db::queries::insert_node(conn, project_id, &manifest_node)?;

    // Link file containing manifest
    db::queries::insert_edge(
        conn,
        project_id,
        file_node_id,
        manifest_node_id,
        "contains",
        None,
    )?;

    // Link dependencies
    for (dep_name, version) in dependencies {
        let dep_qn = format!("__library__{}", dep_name);
        let lib_node = Node {
            id: 0,
            project_id,
            file_path: file_path.to_string(),
            kind: "library".to_string(),
            name: Some(dep_name),
            qualified_name: Some(dep_qn),
            signature: None,
            doc_comment: None,
            start_line: 1,
            end_line: 1,
            complexity: None,
            is_exported: false,
            content_hash: None,
            source: None,
            metadata: Some(serde_json::json!({ "version": version }).to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };

        let lib_node_id = db::queries::insert_node(conn, project_id, &lib_node)?;
        db::queries::insert_edge(
            conn,
            project_id,
            manifest_node_id,
            lib_node_id,
            "depends_on",
            None,
        )?;
    }

    Ok(())
}
