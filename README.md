# codebase-synapse

An MCP server that indexes your codebase into a local knowledge graph, and provides intelligent safety hooks and context for your AI agent.

[![npm](https://img.shields.io/npm/v/codebase-synapse.svg)](https://www.npmjs.com/package/codebase-synapse)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)

---

## 🛡️ The Hero Feature: `codebase-guard`

**Your AI agent is about to confidently edit a critical architectural hub. `codebase-guard` stops it.**

Unlike other MCP servers that just provide search, `codebase-synapse` includes a native safety hook (`codebase-guard`) for Claude Code and other agents. It blocks writes to high-impact files (high PageRank, large blast radius) automatically until the agent explicitly runs an `impact_analysis` to understand what they are about to break.

```json
// Example PreToolUse rejection from codebase-guard
{
  "behavior": "block",
  "message": "⚠️ HIGH-IMPACT FILE: `src/auth_middleware.rs` (PageRank: 0.12, Blast Radius: 47 files)\n\nBefore editing this architectural hub:\n1. Call `impact_analysis` with this node's ID\n2. Review the affected files listed\n3. This block lifts automatically after impact_analysis runs"
}
```

### Why codebase-synapse?

1. **Safety First:** `codebase-guard` acts as an automated senior engineer reviewing your agent's blast radius.
2. **Deep Intelligence:** FMEA Risk Analysis, PageRank authority scoring, Leiden clustering, and Git archaeology (who wrote this and why?).
3. **Token Efficiency:** A single structural query (`explain_code`) in <1ms replaces the agent blindly reading 40 files to understand dependencies.

---

## 🚀 Setup

You can configure `codebase-synapse` in your AI client using one of the following methods.

### Option A: Global Installation (Recommended for Performance)

Installing the package globally avoids registry check delays and network latency from `npx`:

```bash
npm install -g codebase-synapse
```

Then add this JSON block to your AI client's configuration:

```json
{
  "mcpServers": {
    "codebase-synapse": {
      "command": "codebase-synapse",
      "args": []
    }
  }
}
```

### Option B: Quick Start (via `npx` - No installation needed)

Add this JSON block to the configuration file of your AI client:

```json
{
  "mcpServers": {
    "codebase-synapse": {
      "command": "npx",
      "args": ["-y", "codebase-synapse"]
    }
  }
}
```

> **Note:** On Windows, use `"command": "npx.cmd"` instead of `"npx"`.

### Configuring `codebase-guard` (Claude Code)

To enable the safety hook in Claude Code, add this to your project's `.claude.json`:

```json
{
  "hooks": {
    "PreToolUse": "codebase-guard"
  }
}
```

*(Ensure you have `codebase-guard` installed or compiled in your path).*

### Where to paste the MCP configuration

| Client | Config file location |
|---|---|
| Claude Desktop (macOS) | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Claude Desktop (Windows) | `%APPDATA%\Claude\claude_desktop_config.json` |
| Cursor | `.cursor/mcp.json` (in your project root) |
| VS Code | `.vscode/mcp.json` (in your project root) |
| Zed | `~/.config/zed/settings.json` |

After saving the config, restart your AI client. Tell your AI agent: **"Index this project"**.

---

## 🛠️ MCP Tools Reference (40+ Tools)

<details>
<summary>Click to view the full list of available MCP tools</summary>

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
| `prepare_task_context` | Assembles relevant symbols, deps, and memories within a token budget |
| `get_context` | Get a symbol with its callers and callees |
| `get_edit_context` | Everything needed before editing a symbol |
| `get_working_set` | Most-accessed symbols (useful for session preloading) |
| `explain_code` | Synthesizes graph + git + memory + ADRs into one explanation |
| `suggest_tests` | Finds functions needing tests based on complexity and PageRank |
| `plan_change` | FMEA risk-ordered change planning with test impact analysis |

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
| `detect_change_coupling` | Mines Git history to detect logical co-change couplings between files |
| `evaluate_plan_risk` | Computes FMEA risk scores and orders proposed changes (supports RIPPLE intent-aware pruning) |

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

</details>

---

## 🔧 Build from Source

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
