use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use tracing::{info, warn};

use crate::indexer::Indexer;

pub struct FileWatcher {
    indexer: Arc<Indexer>,
    repo_path: String,
}

impl FileWatcher {
    pub fn new(indexer: Arc<Indexer>, repo_path: String) -> Self {
        Self { indexer, repo_path }
    }

    pub fn start(&self) -> Result<()> {
        let repo_path = self.repo_path.clone();
        let indexer = self.indexer.clone();
        let (tx, rx) = mpsc::channel::<Vec<String>>();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    if let EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) =
                        event.kind
                    {
                        let paths: Vec<String> = event
                            .paths
                            .iter()
                            .filter_map(|p| {
                                let ext = p.extension()?.to_str()?;
                                if crate::parser::language::SUPPORTED_EXTENSIONS.contains(&ext) {
                                    Some(p.to_string_lossy().to_string())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if !paths.is_empty() {
                            let _ = tx.send(paths);
                        }
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        watcher.watch(Path::new(&repo_path), RecursiveMode::Recursive)?;
        info!("File watcher started on {}", repo_path);

        std::thread::spawn(move || {
            let mut last_update = std::time::Instant::now();
            let mut pending: Vec<String> = Vec::new();

            loop {
                std::thread::sleep(Duration::from_millis(100));
                while let Ok(paths) = rx.try_recv() {
                    pending.extend(paths);
                    last_update = std::time::Instant::now();
                }

                if !pending.is_empty() && last_update.elapsed() >= Duration::from_millis(500) {
                    let changed: Vec<String> = std::mem::take(&mut pending);
                    info!("Detected {} changed files, re-indexing", changed.len());
                    if let Err(e) = indexer.incremental_update(&repo_path, &changed) {
                        warn!("Incremental update error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }
}
