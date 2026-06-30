# codebase-synapse

An MCP server that indexes your codebase into a local knowledge graph.

[![npm](https://img.shields.io/npm/v/codebase-synapse.svg)](https://www.npmjs.com/package/codebase-synapse)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

---

## What It Does

codebase-synapse parses your code with [Tree-sitter](https://tree-sitter.github.io/tree-sitter/), builds a knowledge graph of all functions, classes, imports, and call relationships, and stores it in a local SQLite database. Your AI agent queries the graph via MCP instead of reading raw files.

**Supported languages:** Rust, TypeScript, JavaScript, Python, Go, Java, C, C++, C#, PHP.

---

## Setup

### 1. Add to your MCP client config

Add this JSON block to the configuration file of your AI client:

```json
{
  "mcpServers": {
    "codebase-synapse": {
      "command": "npx",
      "args": ["-y", "codebase-synapse", "--project-root", "."]
    }
  }
}
```

> **Note:** On Windows, use `"command": "npx.cmd"` instead of `"npx"`.

### Where to paste it

| Client | Config file location |
|---|---|
| Claude Desktop (macOS) | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Claude Desktop (Windows) | `%APPDATA%\Claude\claude_desktop_config.json` |
| Cursor | `.cursor/mcp.json` (in your project root) |
| VS Code | `.vscode/mcp.json` (in your project root) |
| Zed | `~/.config/zed/settings.json` |

### 2. Restart your AI agent

After saving the config, restart your AI client. The server starts automatically when the agent needs it.

### 3. Index your project

Tell your AI agent:

> "Index this project"

The agent will call `index_repository` and the knowledge graph gets built. After that, all tools are available.

---

## How It Works

1. **npx downloads the binary** — a native Rust binary for your platform (Windows, macOS, Linux). No compilation needed.
2. **The server starts via stdio** — your AI client launches it as a child process and communicates over stdin/stdout using the MCP protocol.
3. **Data is stored in `~/.codebase-synapse/`** — a SQLite database with the knowledge graph, embeddings, and memory. Nothing is written inside your project directory.
4. **Your agent calls tools** — instead of reading files, the agent queries the graph for callers, callees, impact analysis, etc.

---

## MCP Tools

These are the actual tools your AI agent can call:

### Indexing

| Tool | What it does |
|---|---|
| `index_repository` | Parse and index an entire codebase |
| `reindex_changed` | Incrementally update only changed files |
| `index_git_history` | Index git commit history (enables hotspots and archaeology) |

### Search

| Tool | What it does |
|---|---|
| `search_symbol` | Find symbols by name or pattern |
| `search_code` | Full-text code search (BM25) |
| `semantic_search` | Vector similarity search using local embeddings |
| `hybrid_search` | Combined BM25 + vector search with Reciprocal Rank Fusion |
| `find_similar` | Find structurally similar functions |
| `find_symbol_everywhere` | Search across all indexed projects |

### Graph Traversal

| Tool | What it does |
|---|---|
| `get_callers` | Who calls this function? |
| `get_callees` | What does this function call? |
| `get_imports` | What does a file import? |
| `get_dependents` | What depends on this symbol? |
| `impact_analysis` | Blast radius — what breaks if you change this? |
| `find_path` | Find the call path between two symbols |
| `find_dead_code` | Find potentially unused functions |

### Context & Editing

| Tool | What it does |
|---|---|
| `prepare_task_context` | Given a task description, assembles relevant symbols, deps, and memories within a token budget |
| `get_context` | Get a symbol with its callers and callees |
| `get_edit_context` | Everything needed before editing a symbol |
| `get_working_set` | Most-accessed symbols (useful for session preloading) |

### Architecture

| Tool | What it does |
|---|---|
| `project_overview` | High-level project statistics |
| `get_architecture` | Languages, entry points, hotspots, dead code |
| `get_file_structure` | Structural overview of a single file |
| `get_clusters` | Leiden community detection — groups files into modules |
| `check_boundaries` | Detect import boundary violations |
| `suggest_boundaries` | Auto-generate boundary rules from clusters |
| `generate_wiki` | Generate a Markdown architecture wiki |
| `get_route_map` | List HTTP routes and their handlers |
| `query_graph` | Run openCypher-like queries on the graph |

### Git & Quality

| Tool | What it does |
|---|---|
| `git_archaeology` | Why does this code exist? Commit history narrative for a symbol |
| `get_hotspots` | Technical debt: high complexity × high git churn |
| `technical_debt_map` | File-level debt ranking |
| `get_contracts` | What tests verify this symbol? |
| `get_recent_semantic_changes` | What changed structurally in the last N hours? |

### Memory

| Tool | What it does |
|---|---|
| `memory_store` | Store a persistent note, fact, or decision |
| `memory_search` | Search stored memories |
| `memory_list` | List memories by kind |
| `memory_delete` | Delete a memory |
| `session_remember` | Store a key-value fact in the current session |
| `session_recall` | Recall a session fact |

### Project Management

| Tool | What it does |
|---|---|
| `list_projects` | List all indexed projects |
| `delete_project` | Remove a project from the index |
| `manage_adr` | CRUD for Architecture Decision Records |
| `link_cross_project` | Create dependency edges between projects |
| `get_stats` | Detailed statistics about indexed data |
| `get_status` | Server health and index status |
| `get_pagerank` | PageRank authority score for a node |

---

## Build from Source

Requires [Rust](https://rustup.rs/):

```bash
git clone https://github.com/iwiels/codebase-synapse.git
cd codebase-synapse
cargo build --release
```

Run tests:

```bash
cargo test --no-default-features
```

---

## License

MIT
