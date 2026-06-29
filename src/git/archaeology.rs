use anyhow::Result;
use git2::{DiffOptions, Repository};
use rusqlite::params;
use tracing::{info, warn};

use crate::db::schema::GitCommitRecord;
use crate::git::classify_intent;

pub struct GitArchaeologist<'a> {
    conn: &'a rusqlite::Connection,
    project_id: i64,
}

impl<'a> GitArchaeologist<'a> {
    pub fn new(conn: &'a rusqlite::Connection, project_id: i64) -> Self {
        Self { conn, project_id }
    }

    pub fn index_history(&self, repo_path: &str, max_commits: usize) -> Result<usize> {
        let repo = match Repository::open(repo_path) {
            Ok(r) => r,
            Err(e) => {
                warn!("Cannot open git repo at {}: {}", repo_path, e);
                return Ok(0);
            }
        };

        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let mut count = 0;
        for oid_result in revwalk.take(max_commits) {
            let oid = match oid_result { Ok(o) => o, Err(_) => continue };
            let commit = match repo.find_commit(oid) { Ok(c) => c, Err(_) => continue };

            let hash = oid.to_string();
            let short_hash = hash[..8].to_string();
            let message = commit.message().unwrap_or("").to_string();
            let author = commit.author().name().unwrap_or("unknown").to_string();
            let timestamp = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
            let intent = classify_intent(&message);

            let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
            let this_tree = commit.tree().ok();
            let mut changed_files: Vec<String> = Vec::new();
            let mut files_changed_count = 0i64;

            if let (Some(pt), Some(tt)) = (parent_tree, this_tree) {
                let mut diff_opts = DiffOptions::new();
                if let Ok(diff) = repo.diff_tree_to_tree(Some(&pt), Some(&tt), Some(&mut diff_opts)) {
                    for delta in diff.deltas() {
                        if let Some(path) = delta.new_file().path() {
                            changed_files.push(path.to_string_lossy().to_string());
                        }
                        files_changed_count += 1;
                    }
                }
            }

            self.conn.execute(
                "INSERT OR IGNORE INTO git_commits
                 (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![self.project_id, hash, short_hash, message.trim(), author,
                        timestamp, intent, files_changed_count],
            )?;

            for file_path in &changed_files {
                let node_ids: Vec<i64> = self.conn.prepare(
                    "SELECT id FROM nodes WHERE project_id = ?1 AND file_path LIKE ?2"
                )?.query_map(
                    params![self.project_id, format!("%{}", file_path)],
                    |row| row.get(0),
                )?.filter_map(|r| r.ok()).collect();

                for node_id in node_ids {
                    self.conn.execute(
                        "INSERT OR IGNORE INTO commit_node_links
                         (project_id, commit_hash, node_id, change_type)
                         VALUES (?1, ?2, ?3, 'modified')",
                        params![self.project_id, &hash, node_id],
                    )?;
                }
            }

            count += 1;
            if count % 200 == 0 { info!("Indexed {} git commits", count); }
        }

        info!("Git archaeology: {} commits indexed", count);
        Ok(count)
    }

    fn get_node_history(&self, node_id: i64) -> Result<Vec<GitCommitRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT gc.id, gc.project_id, gc.hash, gc.short_hash, gc.message, gc.author,
                    gc.timestamp, gc.intent_kind, gc.files_changed
             FROM git_commits gc
             JOIN commit_node_links cnl ON cnl.commit_hash = gc.hash
             WHERE cnl.node_id = ?1 AND gc.project_id = ?2
             ORDER BY gc.timestamp DESC LIMIT 50"
        )?;
        let rows = stmt.query_map(params![node_id, self.project_id], |row| {
            Ok(GitCommitRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                hash: row.get(2)?,
                short_hash: row.get(3)?,
                message: row.get(4)?,
                author: row.get(5)?,
                timestamp: row.get(6)?,
                intent_kind: row.get(7)?,
                files_changed: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn archaeology_narrative(&self, node_id: i64) -> Result<serde_json::Value> {
        let history = self.get_node_history(node_id)?;
        if history.is_empty() {
            return Ok(serde_json::json!({
                "node_id": node_id,
                "narrative": "No git history found. Run index_git_history first.",
                "commits": []
            }));
        }

        let oldest = history.last().ok_or_else(|| anyhow::anyhow!("empty history"))?;
        let newest = history.first().ok_or_else(|| anyhow::anyhow!("empty history"))?;
        let feat_count   = history.iter().filter(|c| c.intent_kind == "feat").count();
        let fix_count    = history.iter().filter(|c| c.intent_kind == "fix").count();
        let refactor_count = history.iter().filter(|c| c.intent_kind == "refactor").count();
        let perf_count   = history.iter().filter(|c| c.intent_kind == "perf").count();

        let narrative = format!(
            "First introduced in commit {} ({}) by {} on {}. \
             Total: {} commits — {} feature additions, {} bug fixes, {} refactors, {} perf improvements. \
             Last touched: {} by {} (\"{}\")",
            oldest.short_hash, oldest.intent_kind, oldest.author, &oldest.timestamp[..10],
            history.len(), feat_count, fix_count, refactor_count, perf_count,
            &newest.timestamp[..10], newest.author,
            newest.message.lines().next().unwrap_or("").trim()
        );

        Ok(serde_json::json!({
            "node_id": node_id,
            "narrative": narrative,
            "total_commits": history.len(),
            "intent_breakdown": {
                "feat": feat_count, "fix": fix_count,
                "refactor": refactor_count, "perf": perf_count,
                "other": history.len() - feat_count - fix_count - refactor_count - perf_count
            },
            "introduced_by": {
                "hash": oldest.short_hash, "author": oldest.author,
                "date": &oldest.timestamp[..10],
                "message": oldest.message.lines().next().unwrap_or("").trim()
            },
            "last_touched": {
                "hash": newest.short_hash, "author": newest.author,
                "date": &newest.timestamp[..10],
                "message": newest.message.lines().next().unwrap_or("").trim()
            },
            "commits": history.iter().take(20).map(|c| serde_json::json!({
                "hash": c.short_hash, "intent": c.intent_kind,
                "author": c.author, "date": &c.timestamp[..10],
                "message": c.message.lines().next().unwrap_or("").trim()
            })).collect::<Vec<_>>()
        }))
    }
}
