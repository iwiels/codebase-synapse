# codebase-synapse 🧠

> **The MCP server that gives your AI agent a brain.**

[![npm](https://img.shields.io/npm/v/codebase-synapse.svg)](https://www.npmjs.com/package/codebase-synapse)
[![npm downloads](https://img.shields.io/npm/dm/codebase-synapse.svg)](https://www.npmjs.com/package/codebase-synapse)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![MCP Compatible](https://img.shields.io/badge/MCP-Compatible-blue.svg)](https://modelcontextprotocol.io/)

Stop letting your AI agent read entire files to answer a single question.
**codebase-synapse** indexes your codebase into a local knowledge graph so your agent can query *exactly* what it needs — call chains, blast radius, architectural decisions, git history — in a single tool call.

---

## The Problem

When you ask Claude Code or Cursor *"what breaks if I change this function?"*, the agent reads **entire files** to figure it out. That's slow, expensive, and hits context limits fast.

## The Solution

codebase-synapse builds a **persistent, local knowledge graph** of your codebase using Tree-sitter AST parsing and SQLite. Your AI agent queries the graph instead of reading files — getting precise, structured answers in milliseconds.

```
# Without codebase-synapse:
Agent reads 47 files to understand one function → 12,000 tokens

# With codebase-synapse:
Agent calls impact_analysis() → 200 tokens, same answer
```

---

## One Command to Set Up Everything

```bash
npx codebase-synapse install
```

That's it. The installer **automatically detects** which AI agents you have on your machine and configures all of them:

```
  🔍 codebase-synapse installer
  /Users/you/my-project

  ✓ Detected: Claude Code, Cursor, VS Code

  Select AI agents to configure:
  ❯ ◉ Claude Code
    ◉ Cursor
    ◉ VS Code

  ✓ Claude Code  → ~/.claude/.mcp.json
  ✓ Cursor       → .cursor/mcp.json
  ✓ VS Code      → .vscode/mcp.json

  Done! Restart your AI agent to use codebase-synapse.
```

**Supported agents — detected automatically:**

| Agent | Auto-detected? | Config Written |
|---|:---:|---|
| Claude Code | ✅ | `~/.claude/.mcp.json` |
| OpenCode | ✅ | `~/.config/opencode/mcp.json` |
| Cursor | ✅ | `.cursor/mcp.json` |
| VS Code | ✅ | `.vscode/mcp.json` |
| Zed | ✅ | `~/.config/zed/settings.json` |
| Gemini CLI | ✅ | `~/.config/gemini/settings.json` |
| Aider | ✅ | `.aider.conf.yml` |
| Continue.dev | ✅ | `~/.continue/config.json` |

No copy-pasting JSON. No reading documentation. Just run one command.

---

## Manual Config (for any MCP client)

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

---

## What Can Your Agent Do Now?

After indexing, just tell your agent:

> *"What files would break if I rename this function?"*
> *"Show me all callers of `processPayment` across the entire codebase"*
> *"Why does this code exist? Show me the git history"*
> *"What are the highest-risk files to edit right now?"*

These are answered in **one tool call** instead of reading dozens of files.

---

## Key Features

### 🎯 Context Budgeting (`prepare_task_context`)
Describe your task in plain English. Synapse assembles the optimal context bundle — relevant symbols, callers/callees, impact analysis, related notes — tuned to your token budget.

### 🕸️ Knowledge Graph + Blast Radius Analysis
Every function, class, import, and call relationship is mapped. Before touching a file, run `impact_analysis` to see exactly what you'd break.

### 🔍 Hybrid Semantic Search
BM25 full-text search + local vector embeddings (all-MiniLM-L6-v2, runs offline) fused with Reciprocal Rank Fusion. No OpenAI API required.

### 📜 Git Archaeology
`git_archaeology` doesn't just show you commits — it narrates *why* code exists, combining commit messages, churn rates, and complexity scores to explain the evolutionary history of any symbol.

### 🏛️ Architecture Decision Records (ADRs)
Your agent can read and write ADRs directly. Decisions are part of the knowledge graph, not a forgotten markdown file.

### 💾 Persistent Memory
Facts, insights, and decisions survive agent resets. Store them at session or project level.

### 🛡️ codebase-guard *(Claude Code only)*
An optional `PreToolUse` hook that automatically **blocks edits to architectural hubs** (files with high PageRank + blast radius) and requires an impact review first. Prevents accidental breakage of critical infrastructure.

---

## How It Compares

| Feature | **codebase-synapse** | CodeGraphContext | CodeGraph | codebase-memory-mcp |
|---|:---:|:---:|:---:|:---:|
| Auto-install for 8+ agents | **✅** | ❌ | ❌ | ❌ |
| `prepare_task_context` (token budget) | **✅** | ❌ | ❌ | ❌ |
| Git Archaeology & Narratives | **✅** | ❌ | ❌ | ❌ |
| Architecture Decision Records | **✅** | ❌ | ❌ | ❌ |
| Cypher-like graph queries | **✅** | ❌ | ❌ | ❌ |
| Boundary violation detection | **✅** | ❌ | ❌ | ❌ |
| codebase-guard hook | **✅** | ❌ | ❌ | ❌ |
| Hybrid semantic search | **✅** | ✅ | ✅ | ✅ |
| Incremental re-indexing | **✅** | ✅ | ✅ | ✅ |
| Local / offline (no API keys) | **✅** | ✅ | ✅ | ❌ |

---

## MCP Tools Reference

| Category | Tool | Description |
|---|---|---|
| **Context** | `prepare_task_context` | Token-optimal context bundle for a task |
| **Indexing** | `index_repository` | Build the knowledge graph |
| | `reindex_changed` | Incremental update for changed files |
| **Search** | `search_symbol` | Find any symbol by name/pattern |
| | `search_code` | BM25 full-text code search |
| | `hybrid_search` | BM25 + vector search with RRF |
| | `find_similar` | Semantically similar code fragments |
| **Graph** | `get_callers` / `get_callees` | Incoming/outgoing call chains |
| | `get_dependents` / `get_imports` | Dependency edges |
| | `impact_analysis` | Blast-radius before touching a file |
| | `find_path` | Call path between two symbols |
| | `query_graph` | Cypher-like graph queries |
| **Git & Quality** | `git_archaeology` | Why does this code exist? |
| | `get_hotspots` | Churn × complexity debt map |
| | `index_git_history` | Index full commit history |
| **Architecture** | `manage_adr` | CRUD architectural decisions |
| | `check_boundaries` | Detect import boundary violations |
| | `suggest_boundaries` | Auto-generate rules from clusters |
| | `get_clusters` | Leiden community detection |
| **Memory** | `memory_store` / `memory_search` | Persistent project notes |
| | `session_remember` / `session_recall` | Short-term session memory |
| **Projects** | `list_projects` | All indexed projects |
| | `project_overview` | Graph stats and summary |
| | `get_architecture` | High-level architectural view |

---

## Build from Source

Requires [Rust](https://rustup.rs/):

```bash
git clone https://github.com/codebase-synapse/index.git
cd index
cargo build --release
```

```bash
cargo test --no-default-features
```

---

## License

MIT — see [LICENSE](./LICENSE).
