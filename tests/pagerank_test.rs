// tests/pagerank_test.rs
use tempfile::tempdir;
use rusqlite::Connection;

#[test]
fn test_pagerank_columns_exist() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = Connection::open(&db_path).unwrap();
    codebase_memory::db::schema::migrate(&conn).unwrap();
    
    // node_pagerank table must exist
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='node_pagerank'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 1, "node_pagerank table must exist after migration");
    
    // file_clusters table must exist
    let count2: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='file_clusters'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(count2, 1, "file_clusters table must exist after migration");
}

#[test]
fn test_get_all_import_edges_empty() {
    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    codebase_memory::db::schema::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES ('p', '/tmp')",
        [],
    ).unwrap();

    let edges = codebase_memory::db::queries::get_all_import_edges(&conn, 1).unwrap();
    assert!(edges.is_empty());
}

#[test]
fn test_update_pagerank_and_get() {
    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    codebase_memory::db::schema::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES ('p', '/tmp')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind, start_line, end_line) VALUES (1, 'src/lib.rs', 'file', 0, 100)",
        [],
    ).unwrap();
    let node_id: i64 = conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap();

    codebase_memory::db::queries::update_node_pagerank(&conn, node_id, 1, 0.42).unwrap();
    let rank = codebase_memory::db::queries::get_node_pagerank(&conn, node_id).unwrap();
    assert!((rank - 0.42).abs() < 1e-9);
}

#[test]
fn test_pagerank_inner_isolated_node() {
    use codebase_memory::graph::pagerank::pagerank_inner;
    let nodes = vec![1i64];
    let edges: Vec<(i64, i64)> = vec![];
    let prev = std::collections::HashMap::new();
    let ranks = pagerank_inner(&nodes, &edges, 0.85, 20, 1e-6, &prev);
    assert!((ranks[&1] - 1.0).abs() < 0.01, "isolated node rank: {}", ranks[&1]);
}

#[test]
fn test_pagerank_inner_hub_beats_leaf() {
    use codebase_memory::graph::pagerank::pagerank_inner;
    // node 2 is the hub (imported by node 1)
    let nodes = vec![1i64, 2i64];
    let edges = vec![(1i64, 2i64)];
    let prev = std::collections::HashMap::new();
    let ranks = pagerank_inner(&nodes, &edges, 0.85, 30, 1e-6, &prev);
    assert!(ranks[&2] > ranks[&1],
        "hub (node2) rank must exceed leaf (node1): node1={} node2={}", ranks[&1], ranks[&2]);
}

#[test]
fn test_pagerank_ranks_sum_to_approximately_one() {
    use codebase_memory::graph::pagerank::pagerank_inner;
    let nodes = vec![1i64, 2i64, 3i64];
    let edges = vec![(1, 2), (2, 3), (3, 1)];
    let prev = std::collections::HashMap::new();
    let ranks = pagerank_inner(&nodes, &edges, 0.85, 30, 1e-6, &prev);
    let total: f64 = ranks.values().sum();
    assert!((total - 1.0).abs() < 0.05, "ranks must sum ~1.0, got {}", total);
}

#[test]
fn test_pagerank_warm_start_same_result() {
    use codebase_memory::graph::pagerank::pagerank_inner;
    let nodes = vec![1i64, 2i64];
    let edges = vec![(1, 2)];
    let prev_empty = std::collections::HashMap::new();
    let cold = pagerank_inner(&nodes, &edges, 0.85, 50, 1e-12, &prev_empty);
    let warm = pagerank_inner(&nodes, &edges, 0.85, 50, 1e-12, &cold);
    for &id in &nodes {
        assert!((cold[&id] - warm[&id]).abs() < 1e-9,
            "warm-start must give same result as cold for node {}", id);
    }
}
