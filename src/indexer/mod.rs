//! Repository indexing pipeline: file walking, tree-sitter parsing, entity extraction, call graph, routes, manifests, IaC.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use tracing::{info, warn};

use crate::config::Config;
use crate::db;
use crate::graph::GraphBuilder;
use crate::parser;
use crate::parser::extractors;
use crate::util::hash;

pub mod calls;
pub mod infra;
pub mod manifests;
pub mod merkle;
pub mod routes;
pub mod walker;
mod watcher;
pub use watcher::FileWatcher;

type IndexResult = Result<Option<(String, String, String, extractors::ExtractionResult)>>;

pub struct Indexer {
    _config: Arc<Config>,
    conn: Arc<std::sync::Mutex<Connection>>,
}

impl Indexer {
    pub fn new(config: Arc<Config>, conn: Arc<std::sync::Mutex<Connection>>) -> Self {
        Self {
            _config: config,
            conn,
        }
    }

    pub fn index_repository(&self, repo_path: &str) -> Result<()> {
        use rayon::prelude::*;

        let path = Path::new(repo_path);
        if !path.exists() {
            anyhow::bail!("Repository path does not exist: {}", repo_path);
        }

        let repo_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        info!("Indexing repository: {} at {}", repo_name, repo_path);

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        let project = db::queries::upsert_project(&conn, repo_name, repo_path)
            .map_err(|e| anyhow::anyhow!("Failed to upsert project '{}': {}", repo_name, e))?;
        let project_id = project.id;

        let files = walker::walk_files(path)
            .with_context(|| format!("Failed to walk files in {}", repo_path))?;
        info!("Found {} files to index", files.len());

        // Step 1: Fetch all existing file states to avoid locking during parallel hashing
        let existing_states =
            db::queries::get_all_file_states(&conn, project_id).unwrap_or_else(|e| {
                warn!(
                    "Failed to fetch file states (proceeding without cache): {}",
                    e
                );
                std::collections::HashMap::new()
            });

        // Release lock before starting parallel CPU-bound parsing/extraction
        drop(conn);

        // Step 2: Parallel file reading, hashing, parsing, and extraction
        let results: Vec<IndexResult> = files
            .par_iter()
            .map(|file_path| {
                let file_path_str = file_path.to_string_lossy().to_string();
                let source = match std::fs::read_to_string(file_path) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Failed to read {}: {}", file_path.display(), e);
                        return Ok(None);
                    }
                };

                let content_hash = hash::content_hash(source.as_bytes());
                let content_hash_str = hash::hash_to_string(content_hash);

                if let Some(existing_hash) = existing_states.get(&file_path_str) {
                    if existing_hash == &content_hash_str {
                        return Ok(None); // Skip this file
                    }
                }

                let parsed = parser::parse_file(file_path, &source)?;

                let extraction_res = match &parsed {
                    Some(p) => match extractors::get_extractor(&p.language) {
                        Some(ext) => ext.extract(&source),
                        None => extractors::ExtractionResult {
                            entities: vec![],
                            relations: vec![],
                        },
                    },
                    None => extractors::ExtractionResult {
                        entities: vec![],
                        relations: vec![],
                    },
                };

                Ok(Some((
                    file_path_str,
                    content_hash_str,
                    source,
                    extraction_res,
                )))
            })
            .collect();

        // Step 3: Re-acquire lock and write to DB in a single transaction
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        let tx = conn.transaction()?;

        let mut builder = GraphBuilder::new(&tx, project_id);
        let mut indexed = 0;
        let mut skipped = 0;

        for r in results {
            match r? {
                Some((file_path_str, content_hash_str, source_str, extraction_res)) => {
                    db::queries::delete_nodes_by_file(&tx, project_id, &file_path_str)?;
                    let node_ids = builder.add_file_nodes(&file_path_str, extraction_res)?;
                    if !node_ids.is_empty() {
                        let file_node_id = node_ids[0];
                        routes::extract_and_insert_routes(
                            &tx,
                            project_id,
                            &file_path_str,
                            &source_str,
                            file_node_id,
                        )?;
                        if manifests::is_manifest_file(&file_path_str) {
                            manifests::extract_and_insert_manifest(
                                &tx,
                                project_id,
                                &file_path_str,
                                &source_str,
                                file_node_id,
                            )?;
                        }
                        if infra::is_infra_file(&file_path_str) {
                            infra::extract_and_insert_infra(
                                &tx,
                                project_id,
                                &file_path_str,
                                &source_str,
                                file_node_id,
                            )?;
                        }
                    }
                    db::queries::upsert_file_state(
                        &tx,
                        project_id,
                        &file_path_str,
                        &content_hash_str,
                    )?;
                    indexed += 1;
                }
                None => {
                    skipped += 1;
                }
            }
        }

        builder.update_counts()?;
        tx.commit()?;

        // Auto-index git history (up to 1000 recent commits)
        {
            let arch = crate::git::GitArchaeologist::new(&conn, project_id);
            match arch.index_history(repo_path, 1000) {
                Ok(n) => info!("Auto-indexed {} git commits", n),
                Err(e) => warn!("Git indexing skipped: {}", e),
            }
        }

        // Resolve function calls (calls edges)
        info!("Resolving lightweight call graph...");
        calls::resolve_project_calls(&conn, project_id)?;

        // Resolve behavioral contracts (test → symbol edges)
        Self::resolve_test_contracts(&conn, project_id, repo_path)?;

        // Find structurally similar functions/methods and link them with SIMILAR_TO edges
        info!("Running structural code similarity detection...");
        match crate::similarity::find_similar_pairs(&conn, project_id, 0.70) {
            Ok(pairs) => {
                info!("Found {} structurally similar function pairs", pairs.len());
                for (id1, id2, score) in pairs {
                    let metadata = serde_json::json!({ "jaccard_score": score }).to_string();
                    if let Err(e) = db::queries::insert_edge(
                        &conn,
                        project_id,
                        id1,
                        id2,
                        "similar_to",
                        Some(&metadata),
                    ) {
                        warn!("Failed to insert similarity edge: {}", e);
                    }
                }
            }
            Err(e) => warn!("Similarity detection failed: {}", e),
        }

        info!(
            "Indexing complete: {} files indexed, {} skipped ({} total)",
            indexed,
            skipped,
            files.len()
        );

        // Step 6: Compute PageRank (warm-start from previous values)
        {
            let pr_config = crate::graph::pagerank::PageRankConfig::default();
            if let Err(e) = crate::graph::compute_pagerank(&conn, project_id, &pr_config) {
                warn!("PageRank computation failed (non-fatal): {}", e);
            } else {
                info!("PageRank computed for project_id={}", project_id);
            }
        }

        // Step 7: Compute Leiden clusters
        {
            let leiden_cfg = crate::graph::LeidenConfig::default();
            match crate::graph::compute_clusters(&conn, project_id, &leiden_cfg) {
                Ok(r) => info!(
                    "Leiden: {} clusters, modularity={:.4}",
                    r.cluster_count, r.modularity
                ),
                Err(e) => warn!("Leiden clustering failed (non-fatal): {}", e),
            }
        }

        Ok(())
    }

    pub fn incremental_update(&self, repo_path: &str, changed_files: &[String]) -> Result<()> {
        let path = Path::new(repo_path);
        let repo_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Phase 1: Resolve project (brief lock)
        let project_id = {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
            let project = db::queries::get_project(&conn, repo_name)
                .map_err(|e| anyhow::anyhow!("Failed to get project '{}': {}", repo_name, e))?;
            match project {
                Some(p) => p.id,
                None => {
                    drop(conn);
                    self.index_repository(repo_path)?;
                    return Ok(());
                }
            }
        };

        // Phase 2: Read & extract files outside the DB lock (CPU-bound work)
        struct FileUpdate {
            file_path: String,
            content_hash: String,
            source: String,
            extraction: extractors::ExtractionResult,
            old_nodes: Vec<crate::db::schema::Node>,
        }

        let mut updates: Vec<FileUpdate> = Vec::new();

        for file_path in changed_files {
            let full_path = path.join(file_path.trim_start_matches('/'));
            if !full_path.exists() {
                // Mark deleted files for removal in Phase 3
                updates.push(FileUpdate {
                    file_path: file_path.clone(),
                    content_hash: String::new(),
                    source: String::new(),
                    extraction: extractors::ExtractionResult {
                        entities: vec![],
                        relations: vec![],
                    },
                    old_nodes: vec![],
                });
                continue;
            }

            let source = match std::fs::read_to_string(&full_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let content_hash = hash::content_hash(source.as_bytes());
            let content_hash_str = hash::hash_to_string(content_hash);

            // Check if file actually changed
            {
                let conn = self
                    .conn
                    .lock()
                    .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
                let existing_hash = db::queries::get_file_state_hash(&conn, project_id, file_path)?;
                if existing_hash.as_deref() == Some(&content_hash_str) {
                    continue;
                }
            }

            let parsed = match parser::parse_file(&full_path, &source)? {
                Some(p) => Some(p),
                None => {
                    let is_special =
                        manifests::is_manifest_file(file_path) || infra::is_infra_file(file_path);
                    if is_special {
                        None
                    } else {
                        continue;
                    }
                }
            };

            let extraction = match parsed {
                Some(p) => match extractors::get_extractor(&p.language) {
                    Some(ext) => ext.extract(&source),
                    None => continue,
                },
                None => extractors::ExtractionResult {
                    entities: vec![],
                    relations: vec![],
                },
            };

            // Snapshot old nodes for semantic diff
            let old_nodes = {
                let conn = self
                    .conn
                    .lock()
                    .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
                db::queries::get_nodes_by_file(&conn, project_id, file_path).unwrap_or_default()
            };

            updates.push(FileUpdate {
                file_path: file_path.clone(),
                content_hash: content_hash_str,
                source,
                extraction,
                old_nodes,
            });
        }

        if updates.is_empty() {
            return Ok(());
        }

        let changed_count = updates.len();

        // Phase 3: Write to DB in a single transaction (atomic)
        {
            let mut conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
            let tx = conn.transaction()?;
            let mut builder = GraphBuilder::new(&tx, project_id);

            for update in &updates {
                if update.source.is_empty() && update.content_hash.is_empty() {
                    // Deleted file
                    db::queries::delete_nodes_by_file(&tx, project_id, &update.file_path)?;
                    db::queries::delete_file_state(&tx, project_id, &update.file_path)?;
                    continue;
                }

                db::queries::delete_nodes_by_file(&tx, project_id, &update.file_path)?;
                let node_ids =
                    builder.add_file_nodes(&update.file_path, update.extraction.clone())?;
                if !node_ids.is_empty() {
                    let file_node_id = node_ids[0];
                    routes::extract_and_insert_routes(
                        &tx,
                        project_id,
                        &update.file_path,
                        &update.source,
                        file_node_id,
                    )?;
                    if manifests::is_manifest_file(&update.file_path) {
                        manifests::extract_and_insert_manifest(
                            &tx,
                            project_id,
                            &update.file_path,
                            &update.source,
                            file_node_id,
                        )?;
                    }
                    if infra::is_infra_file(&update.file_path) {
                        infra::extract_and_insert_infra(
                            &tx,
                            project_id,
                            &update.file_path,
                            &update.source,
                            file_node_id,
                        )?;
                    }
                }
                Self::record_semantic_diff(
                    &tx,
                    project_id,
                    &update.file_path,
                    &update.old_nodes,
                    &update.extraction.entities,
                )?;
                db::queries::upsert_file_state(
                    &tx,
                    project_id,
                    &update.file_path,
                    &update.content_hash,
                )?;
            }

            builder.update_counts()?;
            tx.commit()?;

            // Phase 3b: Resolve call graph (outside transaction, uses conn directly)
            info!(
                "Resolving call graph (incremental, {} files)...",
                changed_count
            );
            calls::resolve_project_calls(&conn, project_id)?;

            // Phase 4: Conditional expensive recomputation
            // Skip similarity when <5 files changed — O(n²) not worth it for small edits
            if changed_count >= 5 {
                info!(
                    "Running structural similarity detection ({} files changed)...",
                    changed_count
                );
                match crate::similarity::find_similar_pairs(&conn, project_id, 0.70) {
                    Ok(pairs) => {
                        for (id1, id2, score) in pairs {
                            let metadata =
                                serde_json::json!({ "jaccard_score": score }).to_string();
                            if let Err(e) = db::queries::insert_edge(
                                &conn,
                                project_id,
                                id1,
                                id2,
                                "similar_to",
                                Some(&metadata),
                            ) {
                                warn!("Failed to insert similarity edge: {}", e);
                            }
                        }
                    }
                    Err(e) => warn!("Similarity detection failed: {}", e),
                }
            }

            // Skip PageRank/Leiden when <10 files changed — not worth recomputing
            if changed_count >= 10 {
                let pr_config = crate::graph::pagerank::PageRankConfig::default();
                if let Err(e) = crate::graph::compute_pagerank(&conn, project_id, &pr_config) {
                    warn!("PageRank computation failed (non-fatal): {}", e);
                }
                let leiden_cfg = crate::graph::LeidenConfig::default();
                match crate::graph::compute_clusters(&conn, project_id, &leiden_cfg) {
                    Ok(r) => info!(
                        "Leiden: {} clusters, modularity={:.4}",
                        r.cluster_count, r.modularity
                    ),
                    Err(e) => warn!("Leiden clustering failed (non-fatal): {}", e),
                }
            }
        }

        info!("Incremental update: {} files updated", changed_count);
        Ok(())
    }

    fn resolve_test_contracts(
        conn: &rusqlite::Connection,
        project_id: i64,
        repo_path: &str,
    ) -> Result<()> {
        use crate::parser::extractors::is_test_file;

        let test_files: Vec<(i64, String)> = conn
            .prepare("SELECT id, file_path FROM nodes WHERE project_id = ?1 AND kind = 'file'")?
            .query_map(params![project_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .filter_map(|r| r.ok())
            .filter(|(_, fp)| is_test_file(fp))
            .collect();

        for (file_node_id, file_path) in &test_files {
            let full_path = std::path::Path::new(repo_path)
                .join(file_path.trim_start_matches('/').trim_start_matches('\\'));
            let source = match std::fs::read_to_string(&full_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let parsed = match parser::parse_file(&full_path, &source)? {
                Some(p) => p,
                None => continue,
            };
            let tested_symbols =
                crate::parser::extractors::extract_tested_symbols(&source, &parsed.language);
            for symbol in &tested_symbols {
                let targets: Vec<i64> = conn
                    .prepare(
                        "SELECT id FROM nodes
                     WHERE project_id = ?1 AND name = ?2
                       AND kind NOT IN ('file', 'test_contract')",
                    )?
                    .query_map(params![project_id, symbol], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                for target_id in targets {
                    db::queries::insert_edge(
                        conn,
                        project_id,
                        *file_node_id,
                        target_id,
                        "test_of",
                        None,
                    )?;
                }
            }
        }
        info!(
            "Test contracts resolved for {} test files",
            test_files.len()
        );
        Ok(())
    }

    fn record_semantic_diff(
        conn: &rusqlite::Connection,
        project_id: i64,
        file_path: &str,
        old_nodes: &[crate::db::schema::Node],
        new_entities: &[crate::parser::extractors::ExtractedEntity],
    ) -> Result<()> {
        let old_names: std::collections::HashSet<&str> =
            old_nodes.iter().filter_map(|n| n.name.as_deref()).collect();
        let new_names: std::collections::HashSet<&str> = new_entities
            .iter()
            .filter_map(|e| e.name.as_deref())
            .collect();

        for name in old_names.difference(&new_names) {
            db::queries::insert_semantic_change(
                conn,
                project_id,
                file_path,
                Some(name),
                "function",
                &format!("Symbol '{}' was removed", name),
            )?;
        }
        for name in new_names.difference(&old_names) {
            db::queries::insert_semantic_change(
                conn,
                project_id,
                file_path,
                Some(name),
                "function",
                &format!("Symbol '{}' was introduced", name),
            )?;
        }
        for old_node in old_nodes {
            if let Some(old_name) = &old_node.name {
                if let Some(new_e) = new_entities
                    .iter()
                    .find(|e| e.name.as_deref() == Some(old_name.as_str()))
                {
                    let old_c = old_node.complexity.unwrap_or(0);
                    let new_c = new_e.complexity.unwrap_or(0);
                    let delta = new_c - old_c;
                    if delta.abs() > 5 {
                        let dir = if delta > 0 { "grew" } else { "shrank" };
                        db::queries::insert_semantic_change(
                            conn,
                            project_id,
                            file_path,
                            Some(old_name),
                            &old_node.kind,
                            &format!(
                                "'{}' {} in complexity by {} lines",
                                old_name,
                                dir,
                                delta.abs()
                            ),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn setup_indexer() -> (Indexer, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let conn = Arc::new(std::sync::Mutex::new(db::open(&db_path).unwrap()));
        let config = Arc::new(crate::config::Config {
            data_dir: tmp.path().to_path_buf(),
            project_root: None,
            graph_only: false,
            log_level: "info".to_string(),
            watch: false,
        });
        (Indexer::new(config, conn), tmp)
    }

    fn create_test_file(dir: &std::path::Path, name: &str, content: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    fn absolute_path(file: &str) -> String {
        // The watcher sends OS-native absolute paths; incremental_update expects them
        std::path::PathBuf::from(file).to_string_lossy().to_string()
    }

    #[test]
    fn test_incremental_adds_new_file() {
        let (indexer, tmp) = setup_indexer();
        let repo = tmp.path();

        // Create a test file
        let file = create_test_file(repo, "test.rs", "fn main() {}");

        // Index it
        indexer.index_repository(repo.to_str().unwrap()).unwrap();

        // Verify node exists
        {
            let conn = indexer.conn.lock().unwrap();
            let nodes: Vec<String> = conn
                .prepare("SELECT name FROM nodes WHERE project_id = 1 AND kind = 'function'")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();

            assert!(nodes.contains(&"main".to_string()));
        }

        // Modify the file
        std::fs::write(&file, "fn main() {}\nfn helper() {}").unwrap();

        // Incremental update
        let abs = absolute_path(&file);
        indexer
            .incremental_update(repo.to_str().unwrap(), &[abs])
            .unwrap();

        // Verify both nodes exist
        let conn = indexer.conn.lock().unwrap();
        let nodes: Vec<String> = conn
            .prepare("SELECT name FROM nodes WHERE project_id = 1 AND kind = 'function'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(nodes.contains(&"main".to_string()));
        assert!(nodes.contains(&"helper".to_string()));
    }

    #[test]
    fn test_incremental_removes_deleted_file() {
        let (indexer, tmp) = setup_indexer();
        let repo = tmp.path();

        // Create and index a file
        let file = create_test_file(repo, "delete_me.rs", "fn gone() {}");
        indexer.index_repository(repo.to_str().unwrap()).unwrap();

        // Verify node exists
        {
            let conn = indexer.conn.lock().unwrap();
            let count: i64 = conn
                .prepare("SELECT COUNT(*) FROM nodes WHERE project_id = 1 AND name = 'gone'")
                .unwrap()
                .query_row([], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }

        // Delete the file and run incremental update
        std::fs::remove_file(&file).unwrap();
        let abs = absolute_path(&file);
        indexer
            .incremental_update(repo.to_str().unwrap(), &[abs])
            .unwrap();

        // Verify node is gone
        let conn = indexer.conn.lock().unwrap();
        let count: i64 = conn
            .prepare("SELECT COUNT(*) FROM nodes WHERE project_id = 1 AND name = 'gone'")
            .unwrap()
            .query_row([], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_incremental_skips_unchanged_file() {
        let (indexer, tmp) = setup_indexer();
        let repo = tmp.path();

        // Create and index a file
        let file = create_test_file(repo, "stable.rs", "fn stable() {}");
        indexer.index_repository(repo.to_str().unwrap()).unwrap();

        // Incremental update with same content — should skip
        let abs = absolute_path(&file);
        indexer
            .incremental_update(repo.to_str().unwrap(), &[abs])
            .unwrap();

        // Should still have exactly 1 node (no duplicates)
        let conn = indexer.conn.lock().unwrap();
        let count: i64 = conn
            .prepare("SELECT COUNT(*) FROM nodes WHERE project_id = 1 AND name = 'stable'")
            .unwrap()
            .query_row([], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_incremental_records_semantic_diff() {
        let (indexer, tmp) = setup_indexer();
        let repo = tmp.path();

        // Index a file with function "old_func"
        let file = create_test_file(repo, "diff.rs", "fn old_func() {}");
        indexer.index_repository(repo.to_str().unwrap()).unwrap();

        // Replace "old_func" with "new_func"
        std::fs::write(&file, "fn new_func() {}").unwrap();
        let abs = absolute_path(&file);
        indexer
            .incremental_update(repo.to_str().unwrap(), &[abs])
            .unwrap();

        // Verify semantic changes were recorded
        let conn = indexer.conn.lock().unwrap();
        let changes: Vec<String> = conn
            .prepare("SELECT change_summary FROM semantic_changes WHERE project_id = 1")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(changes
            .iter()
            .any(|c| c.contains("old_func") && c.contains("removed")));
        assert!(changes
            .iter()
            .any(|c| c.contains("new_func") && c.contains("introduced")));
    }
}
