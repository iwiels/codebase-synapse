use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "codebase-synapse",
    about = "MCP server for codebase indexing & memory"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(short = 'd', long, default_value = "~/.codebase-synapse")]
    pub data_dir: String,

    #[arg(short = 'p', long, default_value = "")]
    pub project_root: String,

    #[arg(long, default_value = "false")]
    pub graph_only: bool,

    #[arg(long, default_value = "info")]
    pub log_level: String,

    #[arg(long, default_value = "false")]
    pub watch: bool,

    #[arg(long)]
    pub run_tool: Option<String>,

    #[arg(long)]
    pub tool_args: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Detect and configure AI agents (Claude Code, Cursor, Zed, etc.) to use this MCP server
    Install {
        /// Only detect, don't write any config files
        #[arg(long)]
        dry_run: bool,
    },
    /// Export or import the indexed graph as a compressed artifact
    Artifact {
        #[command(subcommand)]
        action: ArtifactAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ArtifactAction {
    /// Export graph to a compressed .zst file
    Export {
        /// Output file path (default: codebase-graph.zst)
        output: Option<String>,
    },
    /// Import graph from a compressed .zst file
    Import {
        /// Input file path
        input: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub data_dir: PathBuf,
    pub project_root: Option<PathBuf>,
    pub graph_only: bool,
    pub log_level: String,
    pub watch: bool,
}

impl Config {
    pub fn from_cli(cli: &Cli) -> anyhow::Result<Self> {
        let data_dir = if cli.data_dir.starts_with("~") {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(cli.data_dir.replacen("~", &home, 1))
        } else {
            PathBuf::from(&cli.data_dir)
        };

        let project_root = if cli.project_root.is_empty() {
            std::env::current_dir().ok()
        } else {
            Some(PathBuf::from(&cli.project_root))
        };

        std::fs::create_dir_all(&data_dir)?;

        Ok(Self {
            data_dir,
            project_root,
            graph_only: cli.graph_only,
            log_level: cli.log_level.clone(),
            watch: cli.watch,
        })
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("codebase.db")
    }
}
