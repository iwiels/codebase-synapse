use anyhow::Result;
use rusqlite::Connection;
use crate::db::{self, schema::Node};
use std::sync::LazyLock;
use regex::Regex;
use serde_yaml::Value as YamlValue;

static FROM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^FROM\s+(\S+)").unwrap());
static EXPOSE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^EXPOSE\s+(.+)").unwrap());

pub fn is_infra_file(file_path: &str) -> bool {
    let fp = file_path.to_lowercase().replace('\\', "/");
    let filename = fp.split('/').next_back().unwrap_or(&fp);
    filename == "dockerfile"
        || filename.starts_with("dockerfile.")
        || fp.ends_with(".yaml")
        || fp.ends_with(".yml")
}

pub fn extract_and_insert_infra(
    conn: &Connection,
    project_id: i64,
    file_path: &str,
    source: &str,
    file_node_id: i64,
) -> Result<()> {
    let filename = file_path.to_lowercase().replace('\\', "/");
    let name_only = filename.split('/').next_back().unwrap_or(file_path);

    if name_only == "dockerfile" || name_only.starts_with("dockerfile.") {
        let mut base_image = String::new();
        let mut exposed_ports = Vec::new();

        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(cap) = FROM_RE.captures(line) {
                base_image = cap.get(1).unwrap().as_str().to_string();
            } else if let Some(cap) = EXPOSE_RE.captures(line) {
                let ports_str = cap.get(1).unwrap().as_str();
                for p in ports_str.split_whitespace() {
                    exposed_ports.push(p.to_string());
                }
            }
        }

        let qn = format!("__dockerfile__{}", name_only);
        let docker_node = Node {
            id: 0,
            project_id,
            file_path: file_path.to_string(),
            kind: "dockerfile".to_string(),
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
            metadata: Some(serde_json::json!({
                "base_image": base_image,
                "exposed_ports": exposed_ports
            }).to_string()),
            created_at: String::new(),
            updated_at: String::new(),
        };

        let docker_node_id = db::queries::insert_node(conn, project_id, &docker_node)?;
        db::queries::insert_edge(conn, project_id, file_node_id, docker_node_id, "contains", None)?;

        if !base_image.is_empty() {
            let img_qn = format!("__docker_image__{}", base_image);
            let img_node = Node {
                id: 0,
                project_id,
                file_path: file_path.to_string(),
                kind: "docker_image".to_string(),
                name: Some(base_image),
                qualified_name: Some(img_qn),
                signature: None,
                doc_comment: None,
                start_line: 1,
                end_line: 1,
                complexity: None,
                is_exported: false,
                content_hash: None,
                source: None,
                metadata: None,
                created_at: String::new(),
                updated_at: String::new(),
            };

            let img_node_id = db::queries::insert_node(conn, project_id, &img_node)?;
            db::queries::insert_edge(conn, project_id, docker_node_id, img_node_id, "derived_from", None)?;
        }
    } else if filename.ends_with(".yaml") || filename.ends_with(".yml") {
        // Multi-document YAML support
        for doc in source.split("---") {
            let doc = doc.trim();
            if doc.is_empty() {
                continue;
            }

            if let Ok(yaml) = serde_yaml::from_str::<YamlValue>(doc) {
                // Check if it looks like K8s Resource
                let api_version = yaml.get("apiVersion").and_then(|v| v.as_str());
                let kind = yaml.get("kind").and_then(|v| v.as_str());
                let metadata = yaml.get("metadata").and_then(|v| v.as_mapping());

                if let (Some(api_version), Some(k8s_kind), Some(metadata)) = (api_version, kind, metadata) {
                    let k8s_kind = k8s_kind.to_string();
                    let k8s_name = metadata
                        .get(YamlValue::String("name".to_string()))
                        .and_then(|n| n.as_str())
                        .unwrap_or("unnamed")
                        .to_string();

                    // Parse container images if present (e.g. Deployment / Pod spec)
                    let mut images = Vec::new();
                    extract_images_from_yaml(&yaml, &mut images);

                    let qn = format!("__k8s_resource__{}__{}", k8s_kind, k8s_name);
                    let k8s_node = Node {
                        id: 0,
                        project_id,
                        file_path: file_path.to_string(),
                        kind: "k8s_resource".to_string(),
                        name: Some(format!("{}: {}", k8s_kind, k8s_name)),
                        qualified_name: Some(qn),
                        signature: None,
                        doc_comment: None,
                        start_line: 1,
                        end_line: doc.lines().count() as i64,
                        complexity: None,
                        is_exported: true,
                        content_hash: None,
                        source: None,
                        metadata: Some(serde_json::json!({
                            "apiVersion": api_version,
                            "resource_kind": k8s_kind,
                            "resource_name": k8s_name,
                            "container_images": images
                        }).to_string()),
                        created_at: String::new(),
                        updated_at: String::new(),
                    };

                    let k8s_node_id = db::queries::insert_node(conn, project_id, &k8s_node)?;
                    db::queries::insert_edge(conn, project_id, file_node_id, k8s_node_id, "contains", None)?;

                    for img in images {
                        let img_qn = format!("__docker_image__{}", img);
                        let img_node = Node {
                            id: 0,
                            project_id,
                            file_path: file_path.to_string(),
                            kind: "docker_image".to_string(),
                            name: Some(img),
                            qualified_name: Some(img_qn),
                            signature: None,
                            doc_comment: None,
                            start_line: 1,
                            end_line: 1,
                            complexity: None,
                            is_exported: false,
                            content_hash: None,
                            source: None,
                            metadata: None,
                            created_at: String::new(),
                            updated_at: String::new(),
                        };

                        let img_node_id = db::queries::insert_node(conn, project_id, &img_node)?;
                        db::queries::insert_edge(conn, project_id, k8s_node_id, img_node_id, "deploys_image", None)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn extract_images_from_yaml(val: &YamlValue, images: &mut Vec<String>) {
    match val {
        YamlValue::Mapping(map) => {
            if let Some(img_val) = map.get(YamlValue::String("image".to_string())) {
                if let Some(img_str) = img_val.as_str() {
                    images.push(img_str.to_string());
                }
            }
            for (_, v) in map {
                extract_images_from_yaml(v, images);
            }
        }
        YamlValue::Sequence(seq) => {
            for v in seq {
                extract_images_from_yaml(v, images);
            }
        }
        _ => {}
    }
}
