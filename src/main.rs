use std::sync::{Arc, Mutex};

use anyhow::Context;
use clap::Parser;
use serde_json::json;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use codebase_synapse::cli;
use codebase_synapse::config::{Cli, Commands};
use codebase_synapse::db;
use codebase_synapse::embedding;
use codebase_synapse::indexer::{FileWatcher, Indexer};
use codebase_synapse::mcp::{McpTransport, ToolRegistry};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Install { dry_run }) => {
            return cli::install::Installer::run(*dry_run);
        }
        Some(Commands::Artifact { action }) => {
            let config = Arc::new(codebase_synapse::Config::from_cli(&cli)?);
            let conn = Arc::new(Mutex::new(db::open(&config.db_path())?));
            return match action {
                codebase_synapse::config::ArtifactAction::Export { output } => {
                    let conn = conn.lock().expect("DB lock poisoned");
                    cli::artifact::export_graph(&conn, output.as_deref())?;
                    Ok(())
                }
                codebase_synapse::config::ArtifactAction::Import { input } => {
                    let imported = cli::artifact::import_graph(input)?;
                    info!("Imported graph to {}", imported.display());
                    Ok(())
                }
            };
        }
        None => {}
    }

    let config = Arc::new(
        codebase_synapse::Config::from_cli(&cli).context("Failed to parse configuration")?,
    );

    let filter = EnvFilter::builder()
        .parse(format!("codebase_synapse={}", cli.log_level))
        .context("Invalid log level")?;
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(true)
        .init();

    info!(
        "Starting codebase-synapse v{} (data_dir: {})",
        env!("CARGO_PKG_VERSION"),
        config.data_dir.display()
    );

    let conn = Arc::new(Mutex::new(
        db::open(&config.db_path()).context("Failed to open database")?,
    ));
    info!("Database opened at {}", config.db_path().display());

    let embedder = embedding::create_embedder();
    info!("Embedder initialized ({} dims)", embedder.dimensions());

    let indexer = Arc::new(Indexer::new(config.clone(), conn.clone()));

    if !cli.project_root.is_empty() {
        if let Some(ref project_root) = config.project_root {
            let repo_path = project_root.to_string_lossy().to_string();
            if cli.run_tool.is_none() {
                info!("Spawning background auto-indexing for: {}", repo_path);
                let indexer_clone = indexer.clone();
                std::thread::spawn(move || match indexer_clone.index_repository(&repo_path) {
                    Err(e) => error!("Failed to index repository in background: {}", e),
                    _ => info!("Background auto-indexing complete"),
                });
            }
        }
    }

    let registry = Arc::new(ToolRegistry::new(
        conn,
        config.clone(),
        indexer.clone(),
        embedder,
    ));

    if let Some(tool_name) = cli.run_tool {
        let params = cli
            .tool_args
            .as_deref()
            .map(|a| serde_json::from_str(a).unwrap_or(json!({})))
            .unwrap_or(json!({}));
        let result = registry
            .handle(&tool_name, params)
            .with_context(|| format!("Tool '{}' failed", tool_name))?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    if config.watch {
        if let Some(ref project_root) = config.project_root {
            let watcher = FileWatcher::new(indexer, project_root.to_string_lossy().to_string());
            watcher.start().context("Failed to start file watcher")?;
        }
    }

    info!("Starting MCP server (stdio transport)");
    let transport = McpTransport::new(registry);
    transport.run().context("MCP server exited with error")?;

    Ok(())
}
