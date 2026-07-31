// tests/wiki_test.rs
use codebase_synapse::graph::wiki::{render_wiki, WikiConfig};
use std::collections::HashMap;

#[test]
fn test_render_wiki_contains_project_name() {
    let cfg = WikiConfig {
        project_name: "my-app".into(),
        max_files_per_cluster: 10,
    };
    let clusters: HashMap<i64, Vec<String>> = HashMap::new();
    let md = render_wiki(&cfg, &clusters, &[], 0.0);
    assert!(md.contains("my-app"));
    assert!(md.contains("Architecture Wiki"));
}

#[test]
fn test_render_wiki_two_clusters_sorted() {
    let cfg = WikiConfig {
        project_name: "app".into(),
        max_files_per_cluster: 10,
    };
    let mut clusters = HashMap::new();
    clusters.insert(1i64, vec!["src/api.rs".into()]);
    clusters.insert(2i64, vec!["src/db.rs".into()]);
    let md = render_wiki(&cfg, &clusters, &[], 0.65);
    assert!(md.contains("Cluster 1"));
    assert!(md.contains("Cluster 2"));
    assert!(md.contains("src/api.rs"));
}

#[test]
fn test_render_wiki_deterministic() {
    let cfg = WikiConfig {
        project_name: "app".into(),
        max_files_per_cluster: 10,
    };
    let mut clusters = HashMap::new();
    clusters.insert(1i64, vec!["b.rs".into(), "a.rs".into()]);
    let md1 = render_wiki(&cfg, &clusters, &[], 0.5);
    let md2 = render_wiki(&cfg, &clusters, &[], 0.5);
    assert_eq!(md1, md2, "wiki must be deterministic");
}

#[test]
fn test_generate_wiki_no_panic_with_unclustered_edge_endpoints() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    codebase_synapse::db::schema::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES ('app', '/app')",
        [],
    )
    .unwrap();
    let project_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind) VALUES (?1, 'src/a.rs', 'file')",
        rusqlite::params![project_id],
    )
    .unwrap();
    let a_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind) VALUES (?1, 'src/b.rs', 'file')",
        rusqlite::params![project_id],
    )
    .unwrap();
    let b_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind, name) VALUES (?1, 'src/a.rs', 'function', 'helper')",
        rusqlite::params![project_id],
    )
    .unwrap();
    let helper_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO edges (project_id, source_node_id, target_node_id, kind) VALUES (?1, ?2, ?3, 'import')",
        rusqlite::params![project_id, a_id, b_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO edges (project_id, source_node_id, target_node_id, kind) VALUES (?1, ?2, ?3, 'import')",
        rusqlite::params![project_id, helper_id, b_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO file_clusters (project_id, file_path, cluster_id) VALUES (?1, 'src/a.rs', 1)",
        rusqlite::params![project_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO file_clusters (project_id, file_path, cluster_id) VALUES (?1, 'src/b.rs', 1)",
        rusqlite::params![project_id],
    )
    .unwrap();

    let cfg = WikiConfig {
        project_name: "app".into(),
        max_files_per_cluster: 10,
    };
    let md = codebase_synapse::graph::generate_wiki(&conn, project_id, &cfg).unwrap();
    assert!(md.contains("Architecture Wiki"));
}
