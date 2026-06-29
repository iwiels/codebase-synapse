# codebase-synapse 🧠

[![NPM Version](https://img.shields.io/npm/v/codebase-synapse.svg)](https://www.npmjs.com/package/codebase-synapse)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**codebase-synapse** is a high-performance Model Context Protocol (MCP) server that indexes your codebases into a local SQLite-backed knowledge graph. It enables AI agents (such as Claude Code, Cursor, and Windsurf) to perform deep semantic searches, trace call graphs, conduct blast-radius impact analysis, track architectural decisions (ADRs), and query git history narratives.

By replacing token-heavy file reads with targeted semantic and structural graph queries, **codebase-synapse** reduces agent token consumption by up to **90%**.

---

## Key Features

- **🎯 Context Budgeting (`prepare_task_context`):** Assembles an optimal, token-aware context bundle (relevant symbols, callers/callees, impact analysis, notes) for any natural-language task description in one request.
- **🔍 Hybrid Search:** Combines fast SQLite FTS5 (BM25) full-text search with local vector embeddings (via Candle/all-MiniLM-L6-v2) using Reciprocal Rank Fusion (RRF).
- **🕸️ Code Knowledge Graph:** Extracts AST nodes (classes, functions, variables) and relation edges (calls, imports, similar_to) using Tree-sitter.
- **📜 Git Archaeology (`git_archaeology`):** Narrates *why* code exists, combining git history, churn rates, and complexity to map technical debt hotspots.
- **🏛️ Architecture Decision Records (ADRs):** Exposes ADR CRUD tools so your AI agents can read and write architectural decisions.
- **💾 Persistent Memory:** Session and project-level memory stores for facts, insights, and decisions that survive agent resets.

---

## Installation & Usage

### The One-Liner (Recommended)

Run directly via `npx` (downloads the pre-built binary for your OS automatically):

```bash
npx codebase-synapse
```

### Config for Claude Desktop / Antigravity

Add this configuration to your `mcp_config.json`:

```json
{
  "mcpServers": {
    "codebase-synapse": {
      "command": "npx",
      "args": [
        "-y",
        "codebase-synapse"
      ]
    }
  }
}
```

---

## MCP Tools Reference

Here is a summary of the tools exposed by `codebase-synapse`:

| Category | Tool | Description |
|---|---|---|
| **Indexing** | `index_repository` | Index a codebase into the knowledge graph |
| | `reindex_changed` | Incremental update for modified files |
| **Search** | `search_symbol` | Search nodes by name or pattern |
| | `search_code` | Full-text FTS5 BM25 search over code |
| | `hybrid_search` | BM25 + Vector Semantic Search with RRF |
| **Graph** | `get_callers` / `get_callees` | Find incoming/outgoing call chains |
| | `get_dependents` / `get_imports` | Analyze imports and dependency edges |
| | `impact_analysis` | Blast-radius analysis before modifying code |
| | `find_path` | Trace call path between two symbols |
| **Git & Quality**| `git_archaeology` | Commit narrative for a specific symbol |
| | `get_hotspots` | Churn × complexity technical debt hotspots |
| | `get_contracts` | Map test contracts (`test_of` relations) |
| **Memory** | `memory_store` / `memory_search`| Persist codebase notes, facts, and insights |
| | `session_remember` / `session_recall`| Short-term key-value session memory |
| **ADR** | `manage_adr` | CRUD architectural decision records |

---

## Competitor Comparison

| Feature | **codebase-synapse** | **qartez-mcp** | **code-graph-mcp** |
|---|:---:|:---:|:---:|
| `prepare_task_context` (Context Bundle) | **✅ Yes** | ❌ No | ❌ No |
| `git_archaeology` (Git Narratives) | **✅ Yes** | ❌ No | ❌ No |
| `manage_adr` (Architecture Records) | **✅ Yes** | ❌ No | ❌ No |
| `query_graph` (Cypher queries) | **✅ Yes** | ❌ No | ❌ No |
| Hybrid Semantic Search | **✅ Yes** | ✅ Yes | ✅ Yes |
| Incremental Re-indexing | **✅ Yes** | ✅ Yes | ✅ Yes |
| Platform Packaging | **✅ Yes** | ✅ Yes | ✅ Yes |

---

## Build from Source

If you prefer to build from source, ensure you have Rust installed:

```bash
git clone https://github.com/codebase-synapse/index.git
cd index
cargo build --release
```

Run tests:

```bash
cargo test --no-default-features
```

---

## License

This project is licensed under the MIT License - see the LICENSE file for details.
