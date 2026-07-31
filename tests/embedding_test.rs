mod common;

use codebase_synapse::db;
use codebase_synapse::indexer::Indexer;
use common::MockEmbedder;
use std::sync::Arc;
use tempfile::TempDir;

fn write_repo(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/app.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n\ndef greet() -> str:\n    return \"hello\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/util.py"),
        "import math\n\ndef scale(x: float, k: float) -> float:\n    return x * k\n",
    )
    .unwrap();
}

fn embedding_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .unwrap()
}

fn pending_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM nodes n
         LEFT JOIN embeddings e ON e.node_id = n.id
         WHERE e.node_id IS NULL AND n.source IS NOT NULL",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn test_index_repository_stores_embeddings() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    write_repo(&root);

    let conn = db::open(&tmp.path().join("codebase.db")).unwrap();
    let conn = Arc::new(std::sync::Mutex::new(conn));
    let config = Arc::new(codebase_synapse::config::Config {
        data_dir: tmp.path().to_path_buf(),
        project_root: Some(tmp.path().to_path_buf()),
        graph_only: false,
        log_level: "info".to_string(),
    });
    let indexer = Indexer::new(config, conn.clone());
    let embedder: Arc<dyn codebase_synapse::embedding::Embedder> = Arc::new(MockEmbedder);

    indexer
        .index_repository_with_embedder(root.to_str().unwrap(), &embedder)
        .unwrap();

    let locked = conn.lock().unwrap();
    let count = embedding_count(&locked);
    assert!(count > 0, "no embeddings were stored during indexing");
    let total = locked
        .query_row(
            "SELECT COUNT(*) FROM nodes WHERE project_id = 1 AND source IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(count, total, "every node with source should be embedded");
    assert_eq!(pending_count(&locked), 0, "no node may be left unembedded");

    let dims: i64 = locked
        .query_row("SELECT dimensions FROM embeddings LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(dims, 384, "embeddings must use the embedder dimensions");
}

#[test]
fn test_index_repository_embedding_backfill_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    write_repo(&root);

    let conn = db::open(&tmp.path().join("codebase.db")).unwrap();
    let conn = Arc::new(std::sync::Mutex::new(conn));
    let config = Arc::new(codebase_synapse::config::Config {
        data_dir: tmp.path().to_path_buf(),
        project_root: Some(tmp.path().to_path_buf()),
        graph_only: false,
        log_level: "info".to_string(),
    });
    let indexer = Indexer::new(config, conn.clone());
    let embedder: Arc<dyn codebase_synapse::embedding::Embedder> = Arc::new(MockEmbedder);

    indexer
        .index_repository_with_embedder(root.to_str().unwrap(), &embedder)
        .unwrap();
    let after_first = {
        let locked = conn.lock().unwrap();
        embedding_count(&locked)
    };

    indexer
        .index_repository_with_embedder(root.to_str().unwrap(), &embedder)
        .unwrap();
    let after_second = {
        let locked = conn.lock().unwrap();
        embedding_count(&locked)
    };

    assert_eq!(
        after_first, after_second,
        "re-indexing must not duplicate embeddings"
    );
}

#[test]
fn test_incremental_update_backfills_embeddings() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    write_repo(&root);

    let conn = db::open(&tmp.path().join("codebase.db")).unwrap();
    let conn = Arc::new(std::sync::Mutex::new(conn));
    let config = Arc::new(codebase_synapse::config::Config {
        data_dir: tmp.path().to_path_buf(),
        project_root: Some(tmp.path().to_path_buf()),
        graph_only: false,
        log_level: "info".to_string(),
    });
    let indexer = Indexer::new(config, conn.clone());
    let embedder: Arc<dyn codebase_synapse::embedding::Embedder> = Arc::new(MockEmbedder);

    indexer
        .index_repository_with_embedder(root.to_str().unwrap(), &embedder)
        .unwrap();

    std::fs::write(
        root.join("src/app.py"),
        "def add(a: int, b: int) -> int:\n    return a + b\n\ndef greet() -> str:\n    return \"hello\"\n\ndef new_fn() -> None:\n    pass\n",
    )
    .unwrap();

    indexer
        .incremental_update_with_embedder(
            root.to_str().unwrap(),
            &[root.join("src/app.py").to_str().unwrap().to_string()],
            &embedder,
        )
        .unwrap();

    let locked = conn.lock().unwrap();
    assert_eq!(
        pending_count(&locked),
        0,
        "new nodes from incremental update must be embedded"
    );
    let new_fn: i64 = locked
        .query_row(
            "SELECT COUNT(*) FROM embeddings e
             JOIN nodes n ON n.id = e.node_id
             WHERE n.name = 'new_fn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(new_fn, 1, "newly indexed symbol must have an embedding");
}

#[test]
fn test_noop_embedder_skips_embedding() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    write_repo(&root);

    let conn = db::open(&tmp.path().join("codebase.db")).unwrap();
    let conn = Arc::new(std::sync::Mutex::new(conn));
    let config = Arc::new(codebase_synapse::config::Config {
        data_dir: tmp.path().to_path_buf(),
        project_root: Some(tmp.path().to_path_buf()),
        graph_only: false,
        log_level: "info".to_string(),
    });
    let indexer = Indexer::new(config, conn.clone());
    let noop: Arc<dyn codebase_synapse::embedding::Embedder> =
        Arc::new(codebase_synapse::embedding::NoopEmbedder);

    indexer
        .index_repository_with_embedder(root.to_str().unwrap(), &noop)
        .unwrap();

    let locked = conn.lock().unwrap();
    assert_eq!(
        embedding_count(&locked),
        0,
        "noop embedder must not write embeddings"
    );
}
