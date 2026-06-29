use crate::db::{self, schema::Node};
use anyhow::Result;
use regex::Regex;
use rusqlite::Connection;
use serde_json::json;
use std::sync::LazyLock;

static MULTI_SLASH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/{2,}").unwrap());
static TEMPLATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{[a-zA-Z0-9_]+\}").unwrap());
static FLASK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[a-zA-Z0-9_:]+>").unwrap());
static EXPRESS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":[a-zA-Z0-9_]+").unwrap());
static OPENAPI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[a-zA-Z0-9_]+\}").unwrap());

static ROUTE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
    Regex::new(r#"(?i)(?:app|router|route|api)\.(get|post|put|delete|patch|options|head)\s*\(\s*['"`]([^'"`\s?#]+)['"`]"#).unwrap(),
    Regex::new(r#"(?i)@(?:app|router|api|blueprint)\.(get|post|put|delete|patch|route)\s*\(\s*['"`]([^'"`\s?#]+)['"`]"#).unwrap(),
    Regex::new(r#"\.(GET|POST|PUT|DELETE|PATCH|Handle|HandleFunc)\s*\(\s*['"`]([^'"`\s?#]+)['"`]"#).unwrap(),
    Regex::new(r#"\.route\s*\(\s*['"`]([^'"`\s?#]+)['"`]\s*,\s*(get|post|put|delete|patch)"#).unwrap(),
    Regex::new(r#"(?i)@(GetMapping|PostMapping|PutMapping|DeleteMapping|RequestMapping)\s*\(\s*(?:value\s*=\s*)?['"`]([^'"`\s?#]+)['"`]"#).unwrap(),
]
});

pub fn canonicalize_path(path: &str) -> String {
    let mut clean = MULTI_SLASH_RE.replace_all(path, "/").to_string();
    clean = TEMPLATE_RE.replace_all(&clean, "{}").to_string();
    clean = FLASK_RE.replace_all(&clean, "{}").to_string();
    clean = EXPRESS_RE.replace_all(&clean, "{}").to_string();
    clean = OPENAPI_RE.replace_all(&clean, "{}").to_string();

    // Ensure leading slash
    if !clean.starts_with('/') {
        clean = format!("/{}", clean);
    }

    // Trim trailing slash (except if it is just "/")
    if clean.len() > 1 && clean.ends_with('/') {
        clean.pop();
    }

    clean
}

/// Extract HTTP routes from file source code and insert them as Route nodes connected to the file.
pub fn extract_and_insert_routes(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    source: &str,
    file_node_id: i64,
) -> Result<()> {
    let mut routes = Vec::new();

    for re in ROUTE_PATTERNS.iter() {
        for cap in re.captures_iter(source) {
            let mut method = cap
                .get(1)
                .map(|m| m.as_str().to_uppercase())
                .unwrap_or_else(|| "GET".to_string());
            let mut path = cap
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "/".to_string());

            // Special handling for Axum route where path is group 1 and method is group 2
            if re.as_str().contains(r"\.route") {
                path = cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "/".to_string());
                method = cap
                    .get(2)
                    .map(|m| m.as_str().to_uppercase())
                    .unwrap_or_else(|| "GET".to_string());
            }

            // Normalise Spring RequestMapping to GET by default
            if method.contains("MAPPING")
                && !method.contains("GET")
                && !method.contains("POST")
                && !method.contains("PUT")
                && !method.contains("DELETE")
            {
                method = "GET".to_string();
            }

            let canon_path = canonicalize_path(&path);
            routes.push((method, canon_path));
        }
    }

    routes.dedup();

    for (method, path) in routes {
        let qn = format!("__route__{}__{}", method, path);
        let name = format!("{} {}", method, path);

        let route_node = Node {
            id: 0,
            project_id,
            file_path: file_path.to_string(),
            kind: "route".to_string(),
            name: Some(name),
            qualified_name: Some(qn),
            signature: None,
            doc_comment: None,
            start_line: 1,
            end_line: 1,
            complexity: None,
            is_exported: true,
            content_hash: None,
            source: None,
            metadata: Some(json!({ "method": method, "path": path }).to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };

        // Insert route node (OR IGNORE/REPLACE if it already exists)
        let route_node_id = db::queries::insert_node(conn, project_id, &route_node)?;

        // Link file containing definition to route node
        db::queries::insert_edge(
            conn,
            project_id,
            file_node_id,
            route_node_id,
            "handles",
            None,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonicalize_express() {
        assert_eq!(canonicalize_path("/users/:id"), "/users/{}");
    }

    #[test]
    fn test_canonicalize_axum() {
        assert_eq!(canonicalize_path("/api/orders/{id}"), "/api/orders/{}");
    }

    #[test]
    fn test_canonicalize_flask() {
        assert_eq!(canonicalize_path("/posts/<int:post_id>"), "/posts/{}");
    }

    #[test]
    fn test_canonicalize_template() {
        assert_eq!(canonicalize_path("/docs/${version}"), "/docs/{}");
    }

    #[test]
    fn test_canonicalize_mixed() {
        assert_eq!(canonicalize_path("//api//v1/:id//"), "/api/v1/{}");
    }

    #[test]
    fn test_canonicalize_root() {
        assert_eq!(canonicalize_path("/"), "/");
    }

    #[test]
    fn test_canonicalize_plain() {
        assert_eq!(canonicalize_path("/health"), "/health");
    }
}
