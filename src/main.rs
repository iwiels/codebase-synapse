use std::sync::{Arc, Mutex};

use anyhow::Context;
use clap::Parser;
use serde_json::json;
use tracing::info;
use tracing_subscriber::EnvFilter;

use codebase_synapse::cli;
use codebase_synapse::config::{Cli, Commands};
use codebase_synapse::db;
use codebase_synapse::embedding;
use codebase_synapse::indexer::Indexer;
use codebase_synapse::mcp::{McpTransport, ToolRegistry};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = Arc::new(
        codebase_synapse::Config::from_cli(&cli).context("Failed to parse configuration")?,
    );

    let filter = EnvFilter::builder()
        .parse(format!("codebase_synapse={}", cli.log_level))
        .context("Invalid log level")?;
    let is_mcp_server = cli.run_tool.is_none() && cli.command.is_none();
    // In MCP server mode logs go to a file in the data dir: writing to the
    // stderr pipe can block (or lose data) when the client does not drain
    // it, and opencode does not surface our stderr anyway.
    let log_writer: Box<dyn std::io::Write + Send + Sync> = if is_mcp_server {
        let path = config.data_dir.join("codebase-synapse.log");
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(f) => Box::new(std::io::BufWriter::new(f)),
            Err(e) => {
                eprintln!(
                    "WARNING: cannot open {} ({}); logging to stderr",
                    path.display(),
                    e
                );
                Box::new(std::io::stderr())
            }
        }
    } else {
        Box::new(std::io::stderr())
    };
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(log_writer))
        .with_env_filter(filter)
        .with_target(true)
        .init();

    if let Some(command) = &cli.command {
        return run_command(command, &config);
    }

    info!(
        "Starting codebase-synapse v{} (data_dir: {})",
        env!("CARGO_PKG_VERSION"),
        config.data_dir.display()
    );

    let conn = Arc::new(Mutex::new(
        db::open(&config.db_path()).context("Failed to open database")?,
    ));
    info!("Database opened at {}", config.db_path().display());

    // Lazy embedder: the BERT model (download + build) loads only on the
    // first semantic-search call, keeping server startup instant. Tools like
    // `list_projects` that never touch embeddings stay fast.
    let embedder = embedding::create_embedder();
    info!("Embedder ready (lazy load on first use)");

    let progress = Arc::new(codebase_synapse::mcp::ProgressSender::new());
    let registry = Arc::new(ToolRegistry::new(
        conn,
        config.clone(),
        embedder.clone(),
        progress,
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

    info!("Starting MCP server (stdio transport)");
    let transport = McpTransport::new(registry);
    transport.run().context("MCP server exited with error")?;

    Ok(())
}

fn run_command(command: &Commands, config: &Arc<codebase_synapse::Config>) -> anyhow::Result<()> {
    match command {
        Commands::Artifact { action } => {
            let conn = Arc::new(Mutex::new(db::open(&config.db_path())?));
            match action {
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
            }
        }
        Commands::Index { repo_path } => {
            // CLI-only indexing (Zoekt-style): open a dedicated connection,
            // index synchronously, and print a summary. The MCP server never
            // indexes — it only reads the index this command produced.
            let repo_path = repo_path.to_string_lossy().to_string();
            info!("Indexing {} into {}", repo_path, config.data_dir.display());
            let conn = Arc::new(Mutex::new(db::open(&config.db_path())?));
            let embedder = embedding::create_embedder();
            let indexer = Indexer::new(config.clone(), conn.clone());
            let started = std::time::Instant::now();
            indexer.index_repository_with_embedder(&repo_path, &embedder)?;
            println!("✓ Indexed {} in {:?}", repo_path, started.elapsed());
            Ok(())
        }
    }
}
