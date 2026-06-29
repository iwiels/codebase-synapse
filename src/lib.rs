//! codebase-synapse: MCP server for codebase indexing, knowledge graph, semantic search, and persistent memory.
//!
//! Architecture:
//! - `config` — CLI args and runtime configuration
//! - `db` — SQLite schema, queries, and connection management (WAL mode)
//! - `parser` — Tree-sitter parsing and entity extraction (10 languages)
//! - `graph` — Knowledge graph builder, traversal, PageRank, Leiden clustering, boundary enforcement
//! - `indexer` — Repository indexing pipeline: walk, parse, extract, build graph, detect calls/routes/manifests/infra
//! - `search` — BM25 full-text, vector cosine, hybrid RRF search
//! - `embedding` — Candle-based all-MiniLM-L6-v2 embeddings (offline, feature-gated)
//! - `memory` — Persistent note/fact/decision store + session memory
//! - `mcp` — MCP protocol transport (stdio) + 42 tool handlers + architecture overview
//! - `git` — Git archaeology, intent classification, hotspot analysis
//! - `access` — Node access tracking for working set computation
//! - `context` — Budgeted context preparation for AI agents
//! - `cypher` — Nom-based Cypher parser → SQL recursive CTE planner
//! - `similarity` — MinHash + LSH for structural code similarity
//! - `semantic` — Multi-signal scoring (token overlap, directory proximity, AST profile)
//! - `cli` — Interactive TUI installer and artifact export/import

pub mod access;
pub mod config;
pub mod context;
pub mod cypher;
pub mod db;
pub mod embedding;
pub mod git;
pub mod graph;
pub mod indexer;
pub mod mcp;
pub mod memory;
pub mod parser;
pub mod search;
pub mod semantic;
pub mod similarity;
pub mod util;

pub mod cli;

pub use config::Config;
