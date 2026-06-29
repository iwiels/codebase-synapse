use anyhow::Result;
use std::path::PathBuf;

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub struct Installer;

impl Installer {
    pub fn run(_dry_run: bool) -> Result<()> {
        let home = home_dir();
        let project_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Detect OS for custom config paths
        let (claude_path, cursor_path, vscode_path, zed_path) = if cfg!(windows) {
            (
                home.join("AppData\\Roaming\\Claude\\claude_desktop_config.json"),
                project_dir.join(".cursor\\mcp.json"),
                project_dir.join(".vscode\\mcp.json"),
                home.join(".config\\zed\\settings.json"),
            )
        } else if cfg!(target_os = "macos") {
            (
                home.join("Library/Application Support/Claude/claude_desktop_config.json"),
                project_dir.join(".cursor/mcp.json"),
                project_dir.join(".vscode/mcp.json"),
                home.join(".config/zed/settings.json"),
            )
        } else {
            (
                home.join(".config/Claude/claude_desktop_config.json"),
                project_dir.join(".cursor/mcp.json"),
                project_dir.join(".vscode/mcp.json"),
                home.join(".config/zed/settings.json"),
            );
            (
                home.join(".config/Claude/claude_desktop_config.json"),
                project_dir.join(".cursor/mcp.json"),
                project_dir.join(".vscode/mcp.json"),
                home.join(".config/zed/settings.json"),
            )
        };

        let cmd = if cfg!(windows) { "npx.cmd" } else { "npx" };

        println!();
        println!("  ⚙️  codebase-synapse - MCP Configuration Guide");
        println!("  ============================================");
        println!("  To use codebase-synapse as an MCP server, add it to your client config.");
        println!();
        println!("  📋 MCP JSON Config block:");
        println!("  --------------------------------------------");
        println!("  {{");
        println!("    \"mcpServers\": {{");
        println!("      \"codebase-synapse\": {{");
        println!("        \"command\": \"{}\",", cmd);
        println!("        \"args\": [\"-y\", \"codebase-synapse\", \"--project-root\", \".\"]");
        println!("      }}");
        println!("    }}");
        println!("  }}");
        println!("  --------------------------------------------");
        println!();
        println!("  📁 Common configuration file paths for your OS:");
        println!();
        println!("    • Claude Desktop:");
        println!("      👉 {}", claude_path.display());
        println!();
        println!("    • Cursor (Project level):");
        println!("      👉 {}", cursor_path.display());
        println!();
        println!("    • VS Code (with Claude Dev/Continue):");
        println!("      👉 {}", vscode_path.display());
        println!();
        println!("    • Zed:");
        println!("      👉 {}", zed_path.display());
        println!();
        println!("  💡 Pro-Tip: Make sure you have Node.js (v18+) installed.");
        println!("  Once added, restart your AI agent to begin indexing your codebase!");
        println!();

        Ok(())
    }
}
