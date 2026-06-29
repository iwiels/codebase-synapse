// tests/sprint_integration.rs
use rusqlite::Connection;
use tempfile::tempdir;

fn setup() -> (tempfile::TempDir, Connection) {
    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    codebase_synapse::db::schema::migrate(&conn).unwrap();
    (dir, conn)
}

fn insert_project(conn: &Connection) -> i64 {
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES ('test', '/tmp')",
        [],
    )
    .unwrap();
    conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
        .unwrap()
}

fn insert_node(conn: &Connection, pid: i64, fp: &str, kind: &str) -> i64 {
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind, start_line, end_line, complexity) VALUES (?1,?2,?3,0,10,1)",
        rusqlite::params![pid, fp, kind],
    ).unwrap();
    conn.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
        .unwrap()
}

fn insert_edge(conn: &Connection, pid: i64, s: i64, d: i64, kind: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO edges (project_id, source_node_id, target_node_id, kind) VALUES (?1,?2,?3,?4)",
        rusqlite::params![pid, s, d, kind],
    ).unwrap();
}

#[test]
fn test_pagerank_hub_beats_leaf() {
    let (_dir, conn) = setup();
    let pid = insert_project(&conn);
    let lib = insert_node(&conn, pid, "src/lib.rs", "file");
    let main = insert_node(&conn, pid, "src/main.rs", "file");
    insert_edge(&conn, pid, main, lib, "imports");

    codebase_synapse::graph::compute_pagerank(
        &conn,
        pid,
        &codebase_synapse::graph::PageRankConfig::default(),
    )
    .unwrap();

    let r_lib = codebase_synapse::db::queries::get_node_pagerank(&conn, lib).unwrap();
    let r_main = codebase_synapse::db::queries::get_node_pagerank(&conn, main).unwrap();
    assert!(
        r_lib > r_main,
        "lib (hub) must have higher rank than main: lib={} main={}",
        r_lib,
        r_main
    );
}

#[test]
fn test_leiden_persists_clusters() {
    let (_dir, conn) = setup();
    let pid = insert_project(&conn);
    let nodes: Vec<i64> = (0..6)
        .map(|i| {
            let fp = if i < 3 {
                format!("src/api/{}.rs", i)
            } else {
                format!("src/db/{}.rs", i)
            };
            insert_node(&conn, pid, &fp, "file")
        })
        .collect();
    for &(s, d) in &[(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3)] {
        insert_edge(&conn, pid, nodes[s], nodes[d], "imports");
    }

    let report = codebase_synapse::graph::compute_clusters(
        &conn,
        pid,
        &codebase_synapse::graph::LeidenConfig::default(),
    )
    .unwrap();

    assert!(report.cluster_count >= 1);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_clusters WHERE project_id=?1",
            rusqlite::params![pid],
            |r| r.get(0),
        )
        .unwrap();
    assert!(count > 0, "file_clusters must be populated");
}

#[test]
fn test_boundary_violation_e2e() {
    use codebase_synapse::graph::boundaries::{check_boundaries, BoundaryConfig, BoundaryRule};
    let config = BoundaryConfig {
        boundary: vec![BoundaryRule {
            from: "src/api/**".into(),
            deny: vec!["src/db/**".into()],
            allow: vec![],
        }],
    };
    let files = vec![("src/api/u.rs".into(), 1i64), ("src/db/r.rs".into(), 2i64)];
    let edges = vec![(1i64, 2i64)];
    let v = check_boundaries(&config, &files, &edges).unwrap();
    assert_eq!(v.len(), 1);
}

#[test]
fn test_wiki_rendered_from_clusters_db() {
    let (_dir, conn) = setup();
    let pid = insert_project(&conn);
    let n1 = insert_node(&conn, pid, "src/api.rs", "file");
    conn.execute(
        "INSERT INTO file_clusters (project_id, file_path, cluster_id) VALUES (?1,'src/api.rs',1)",
        rusqlite::params![pid],
    )
    .unwrap();
    let cfg = codebase_synapse::graph::WikiConfig {
        project_name: "test".into(),
        max_files_per_cluster: 20,
    };
    let md = codebase_synapse::graph::generate_wiki(&conn, pid, &cfg).unwrap();
    assert!(md.contains("Cluster 1"), "wiki must contain Cluster 1");
    assert!(md.contains("src/api.rs"), "wiki must list the file");
    let _ = n1;
}

#[test]
fn test_hotspots_debt_map() {
    let (_dir, conn) = setup();
    let pid = insert_project(&conn);
    let n1 = insert_node(&conn, pid, "src/api.rs", "function");
    // Insert commit link
    conn.execute(
        "INSERT INTO git_commits (project_id, hash, short_hash, message, author, timestamp) VALUES (?1, 'abc', 'abc', 'init', 'vic', '2026-06-28')",
        rusqlite::params![pid],
    ).unwrap();
    conn.execute(
        "INSERT INTO commit_node_links (project_id, commit_hash, node_id) VALUES (?1, 'abc', ?2)",
        rusqlite::params![pid, n1],
    )
    .unwrap();

    let analyzer = codebase_synapse::git::HotspotAnalyzer::new(&conn);
    let hotspots = analyzer.get_hotspots(pid, 10).unwrap();
    assert_eq!(hotspots.len(), 1);

    let debt = analyzer.technical_debt_map(pid).unwrap();
    assert_eq!(debt.len(), 1);
}
