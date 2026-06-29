use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use inquire::{Confirm, MultiSelect};
use serde_json::json;

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agent {
    ClaudeCode,
    OpenCode,
    Cursor,
    VSCode,
    Zed,
    GeminiCli,
    Aider,
    ContinueDev,
}

impl Agent {
    fn name(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::OpenCode => "OpenCode",
            Agent::Cursor => "Cursor",
            Agent::VSCode => "VS Code",
            Agent::Zed => "Zed",
            Agent::GeminiCli => "Gemini CLI",
            Agent::Aider => "Aider",
            Agent::ContinueDev => "Continue.dev",
        }
    }

    fn config_paths(&self, home: &Path, project_dir: &Path) -> Vec<PathBuf> {
        match self {
            Agent::ClaudeCode => vec![home.join(".claude").join(".mcp.json")],
            Agent::OpenCode => vec![
                home.join(".config").join("opencode").join("mcp.json"),
                project_dir.join(".opencode").join("mcp.json"),
            ],
            Agent::Cursor => vec![
                project_dir.join(".cursor").join("mcp.json"),
                home.join(".cursor").join("mcp.json"),
            ],
            Agent::VSCode => vec![project_dir.join(".vscode").join("mcp.json")],
            Agent::Zed => vec![home.join(".config").join("zed").join("settings.json")],
            Agent::GeminiCli => vec![home.join(".config").join("gemini").join("settings.json")],
            Agent::Aider => vec![
                project_dir.join(".aider.conf.yml"),
                home.join(".aider.conf.yml"),
            ],
            Agent::ContinueDev => vec![
                home.join(".continue").join("config.json"),
                project_dir.join(".continue").join("config.json"),
            ],
        }
    }

    fn detected(&self, home: &Path, project_dir: &Path) -> bool {
        self.config_paths(home, project_dir)
            .iter()
            .any(|p| p.exists())
    }

    fn mcp_config() -> serde_json::Value {
        let cmd = if cfg!(windows) { "npx.cmd" } else { "npx" };
        json!({
            "command": cmd,
            "args": ["-y", "codebase-synapse", "--project-root", "."]
        })
    }

    fn write_config(&self, home: &Path, project_dir: &Path) -> Result<Option<PathBuf>> {
        match self {
            Agent::ClaudeCode => {
                let dir = home.join(".claude");
                let path = dir.join(".mcp.json");
                fs::create_dir_all(&dir)?;
                Self::merge_and_write(&path, "mcpServers", home, project_dir)
            }
            Agent::OpenCode => {
                let paths = vec![
                    home.join(".config").join("opencode").join("mcp.json"),
                    project_dir.join(".opencode").join("mcp.json"),
                ];
                let mut last = None;
                for path in &paths {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    Self::merge_and_write(path, "mcp", home, project_dir)?;
                    last = Some(path.clone());
                }
                Ok(last)
            }
            Agent::Cursor | Agent::VSCode => {
                let paths = self.config_paths(home, project_dir);
                let path = paths.into_iter().next().unwrap_or_else(|| {
                    project_dir
                        .join(match self {
                            Agent::Cursor => ".cursor",
                            _ => ".vscode",
                        })
                        .join("mcp.json")
                });
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                Self::merge_and_write(&path, "mcpServers", home, project_dir)
            }
            Agent::Zed => {
                let path = home.join(".config").join("zed").join("settings.json");
                let dir = path.parent().unwrap();
                fs::create_dir_all(dir)?;
                let mut cfg: serde_json::Value = if path.exists() {
                    let content = fs::read_to_string(&path)?;
                    serde_json::from_str(&content).unwrap_or(json!({}))
                } else {
                    json!({})
                };
                cfg["mcpServers"]["codebase-synapse"] = Self::mcp_config();
                fs::write(&path, serde_json::to_string_pretty(&cfg)?)?;
                Ok(Some(path))
            }
            Agent::GeminiCli => {
                let path = home.join(".config").join("gemini").join("settings.json");
                let dir = path.parent().unwrap();
                fs::create_dir_all(dir)?;
                let mut cfg: serde_json::Value = if path.exists() {
                    let content = fs::read_to_string(&path)?;
                    serde_json::from_str(&content).unwrap_or(json!({}))
                } else {
                    json!({})
                };
                cfg["mcpServers"]["codebase-synapse"] = Self::mcp_config();
                fs::write(&path, serde_json::to_string_pretty(&cfg)?)?;
                Ok(Some(path))
            }
            _ => Ok(None),
        }
    }

    fn merge_and_write(
        path: &Path,
        key: &str,
        _home: &Path,
        _project_dir: &Path,
    ) -> Result<Option<PathBuf>> {
        let mut cfg: serde_json::Value = if path.exists() {
            let content = fs::read_to_string(path)?;
            serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };
        ObjectMerge(&mut cfg).set_nested(key, "codebase-synapse", Self::mcp_config());
        fs::write(path, serde_json::to_string_pretty(&cfg)?)?;
        Ok(Some(path.to_path_buf()))
    }
}

static ALL_AGENTS: &[Agent] = &[
    Agent::ClaudeCode,
    Agent::OpenCode,
    Agent::Cursor,
    Agent::VSCode,
    Agent::Zed,
    Agent::GeminiCli,
    Agent::Aider,
    Agent::ContinueDev,
];

pub struct Installer;

impl Installer {
    pub fn run(dry_run: bool) -> Result<()> {
        let home = home_dir();
        let project_dir = std::env::current_dir().context("Cannot determine current directory")?;

        println!();
        println!("  🔍 codebase-synapse installer");
        println!("  {}", project_dir.display());
        println!();

        // Detect all agents
        let mut detected = Vec::new();
        let mut not_detected = Vec::new();

        for agent in ALL_AGENTS {
            if agent.detected(&home, &project_dir) {
                detected.push(agent);
            } else {
                not_detected.push(agent);
            }
        }

        if detected.is_empty() {
            println!("  No AI agents detected.");
            println!();
            println!("  Install one of these and run `index install` again:");
            println!();
            for agent in not_detected {
                let paths = agent.config_paths(&home, &project_dir);
                let path_str = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("    • {}  ({})", agent.name(), path_str);
            }
            println!();
            println!("  You can also add this to any MCP client manually:");
            println!();
            println!("  {{\n    \"mcpServers\": {{\n      \"codebase-synapse\": {{\n        \"command\": \"npx\",\n        \"args\": [\"-y\", \"codebase-synapse\", \"--project-root\", \".\"]\n      }}\n    }}\n  }}");
            println!();
            return Ok(());
        }

        // Show detected agents with multi-select
        let options: Vec<&Agent> = detected.to_vec();
        let default_idx: Vec<usize> = (0..options.len()).collect();

        let selection = MultiSelect::new(
            "  Select AI agents to configure:",
            options.iter().map(|a| a.name()).collect(),
        )
        .with_default(&default_idx)
        .with_help_message("↑↓ move, Space toggle, Enter confirm")
        .prompt();

        let selected = match selection {
            Ok(s) => {
                let names: Vec<&str> = s.iter().map(|s| s.as_ref()).collect();
                options
                    .into_iter()
                    .filter(|a| names.contains(&a.name()))
                    .collect::<Vec<_>>()
            }
            Err(_) => {
                println!("  Cancelled.");
                return Ok(());
            }
        };

        if selected.is_empty() {
            println!("  No agents selected. Nothing to do.");
            return Ok(());
        }

        println!();
        println!("  Selected:");
        for agent in &selected {
            println!("    ✓ {}", agent.name());
        }
        println!();

        if dry_run {
            println!("  Dry-run mode: no files were modified.");
            return Ok(());
        }

        // Confirm if installing to project-level configs
        let has_project_configs = selected
            .iter()
            .any(|a| matches!(a, Agent::OpenCode | Agent::Cursor | Agent::VSCode));
        if has_project_configs {
            let ok = Confirm::new(
                "  Write project-level MCP configs? (will be added to version control)",
            )
            .with_default(false)
            .prompt()
            .unwrap_or(false);
            if !ok {
                println!("  Skipping project-level configs.");
                return Ok(());
            }
        }

        // Write configs
        for agent in &selected {
            match agent.write_config(&home, &project_dir) {
                Ok(Some(path)) => println!("  ✓ {} → {}", agent.name(), path.display()),
                Ok(None) => println!("  ✓ {} configured", agent.name()),
                Err(e) => eprintln!("  ✗ {} failed: {}", agent.name(), e),
            }
        }

        println!();
        println!("  Done! Restart your AI agent to use codebase-synapse.");
        println!();
        println!("  Next steps:");
        println!("    1. In your AI agent, say: \"Index this project\"");
        println!("    2. Or run:  npx codebase-synapse --project-root .");
        println!();
        println!("  Manual MCP config (for other agents):");
        println!("  {{\n    \"mcpServers\": {{\n      \"codebase-synapse\": {{\n        \"command\": \"npx\",\n        \"args\": [\"-y\", \"codebase-synapse\", \"--project-root\", \".\"]\n      }}\n    }}\n  }}");

        Ok(())
    }
}

// Helper: set a nested key in a JSON object (e.g. "mcpServers" → "mcpServers.codebase-synapse")
struct ObjectMerge<'a>(pub &'a mut serde_json::Value);

impl ObjectMerge<'_> {
    fn set_nested(&mut self, parent: &str, child: &str, value: serde_json::Value) {
        let obj = self.0.as_object_mut().expect("expected object");
        let entry = obj.entry(parent.to_string()).or_insert_with(|| json!({}));
        if let serde_json::Value::Object(ref mut map) = entry {
            map.insert(child.to_string(), value);
        } else {
            *entry = json!({child: value});
        }
    }
}
