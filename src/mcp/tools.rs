use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};
use tracing::info;

use crate::config::Config;
use crate::context::ContextBudget;
use crate::db;
use crate::db::schema::SearchResult;
use crate::embedding::Embedder;
use crate::git::{GitArchaeologist, HotspotAnalyzer};
use crate::graph::{GraphTraversal, ImpactAnalysis};
use crate::indexer::Indexer;
use crate::memory::{MemoryStore, SessionMemory};
use crate::search::{Bm25Search, HybridSearch};

type ToolHandler = Arc<dyn Fn(Value) -> Result<Value> + Send + Sync>;
type SharedConn = Arc<Mutex<Connection>>;

pub struct ToolRegistry {
    tools: HashMap<String, ToolDef>,
    handlers: HashMap<String, ToolHandler>,
}

struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

impl ToolRegistry {
    pub fn new(
        conn: SharedConn,
        config: Arc<Config>,
        indexer: Arc<Indexer>,
        embedder: Arc<dyn Embedder>,
    ) -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
            handlers: HashMap::new(),
        };

        let session = Arc::new(Mutex::new(SessionMemory::new(100)));

        registry.register_index_tools(&conn, &config, &indexer);
        registry.register_search_tools(&conn, &embedder);
        registry.register_graph_tools(&conn);
        registry.register_memory_tools(&conn, &session);
        registry.register_context_tools(&conn);
        registry.register_utility_tools(&conn, &config);
        registry.register_archaeology_tools(&conn, &embedder);

        registry
    }

    fn register(
        &mut self,
        name: &str,
        description: &str,
        input_schema: Value,
        handler: ToolHandler,
    ) {
        self.tools.insert(
            name.to_string(),
            ToolDef {
                name: name.to_string(),
                description: description.to_string(),
                input_schema,
            },
        );
        self.handlers.insert(name.to_string(), handler);
    }

    fn conn<T, F: FnOnce(&Connection) -> Result<T>>(conn: &SharedConn, f: F) -> Result<T> {
        let guard = conn
            .lock()
            .map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        f(&guard)
    }

    fn register_index_tools(
        &mut self,
        conn: &SharedConn,
        _config: &Arc<Config>,
        indexer: &Arc<Indexer>,
    ) {
        let _c = conn.clone();
        let idx = indexer.clone();
        self.register(
            "index_repository",
            "Index a repository into the knowledge graph",
            json!({"type":"object","properties":{"repo_path":{"type":"string"}},"required":["repo_path"]}),
            Arc::new(move |params| {
                let repo_path = params["repo_path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing repo_path"))?;
                info!("Indexing repository: {}", repo_path);
                idx.index_repository(repo_path)?;
                Ok(json!({"status":"indexed","repo_path":repo_path}))
            }),
        );

        let c2 = conn.clone();
        self.register(
            "list_projects",
            "List all indexed projects with statistics",
            json!({"type":"object","properties":{}}),
            Arc::new(move |_| {
                Self::conn(&c2, |conn| {
                    let projects = db::queries::list_projects(conn)?;
                    Ok(json!({"projects": projects}))
                })
            }),
        );

        let c3 = conn.clone();
        self.register(
            "delete_project",
            "Delete an indexed project",
            json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
            Arc::new(move |params| {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
                Self::conn(&c3, |conn| {
                    let project = db::queries::get_project(conn, name)?;
                    if let Some(p) = project {
                        db::queries::delete_project(conn, p.id)?;
                    }
                    Ok(json!({"status":"deleted"}))
                })
            }),
        );

        let _c4 = conn.clone();
        let idx2 = indexer.clone();
        self.register(
            "reindex_changed",
            "Incremental reindex of changed files",
            json!({"type":"object","properties":{"repo_path":{"type":"string"},"files":{"type":"array","items":{"type":"string"}}},"required":["repo_path"]}),
            Arc::new(move |params| {
                let repo_path = params["repo_path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing repo_path"))?;
                let files: Vec<String> = params["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                idx2.incremental_update(repo_path, &files)?;
                Ok(json!({"status":"updated","files":files.len()}))
            }),
        );
    }

    fn register_search_tools(&mut self, conn: &SharedConn, embedder: &Arc<dyn Embedder>) {
        let c = conn.clone();
        self.register(
            "search_symbol",
            "Search symbols by name or pattern",
            json!({"type":"object","properties":{"project":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","default":20}},"required":["project","query"]}),
            Arc::new(move |params| {
                let query = params["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                let limit = params["limit"].as_i64().unwrap_or(20);
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let bm25 = Bm25Search::new(conn);
                    let results = bm25.search_by_name(project.id, query, limit)?;
                    Ok(json!(results))
                })
            }),
        );

        let c2 = conn.clone();
        self.register(
            "search_code",
            "Full-text search across code (FTS5 + BM25)",
            json!({"type":"object","properties":{"project":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","default":20}},"required":["project","query"]}),
            Arc::new(move |params| {
                let query = params["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                let limit = params["limit"].as_i64().unwrap_or(20);
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c2, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let bm25 = Bm25Search::new(conn);
                    let results = bm25.search(project.id, query, limit)?;
                    Ok(serialize_search_results(results))
                })
            }),
        );

        let c3 = conn.clone();
        let emb = embedder.clone();
        self.register(
            "semantic_search",
            "Search code by semantic meaning using vector embeddings",
            json!({"type":"object","properties":{"project":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","default":10}},"required":["project","query"]}),
            Arc::new(move |params| {
                let query = params["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                let limit = params["limit"].as_i64().unwrap_or(10) as usize;
                let pname = params["project"].as_str().unwrap_or("default");
                let embedding = emb.embed(&[query])?;
                let query_vec = embedding.first().cloned().unwrap_or_default();
                Self::conn(&c3, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let hybrid = HybridSearch::new(conn);
                    let results = hybrid.search(project.id, query, Some(&query_vec), limit)?;
                    Ok(serialize_search_results(results))
                })
            }),
        );

        let c4 = conn.clone();
        let emb2 = embedder.clone();
        self.register(
            "hybrid_search",
            "Combined full-text + semantic search with RRF fusion",
            json!({"type":"object","properties":{"project":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","default":20}},"required":["project","query"]}),
            Arc::new(move |params| {
                let query = params["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                let limit = params["limit"].as_i64().unwrap_or(20) as usize;
                let pname = params["project"].as_str().unwrap_or("default");
                let embedding = emb2.embed(&[query])?;
                let query_vec = embedding.first().cloned().unwrap_or_default();
                Self::conn(&c4, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let hybrid = HybridSearch::new(conn);
                    let results = hybrid.search(project.id, query, Some(&query_vec), limit)?;
                    Ok(serialize_search_results(results))
                })
            }),
        );
    }

    fn register_graph_tools(&mut self, conn: &SharedConn) {
        let c = conn.clone();
        self.register("get_callers", "Find what calls a function or method",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"max_depth":{"type":"integer","default":3}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let depth = params["max_depth"].as_i64().unwrap_or(3);
                Self::conn(&c, |conn| {
                    let t = GraphTraversal::new(conn);
                    Ok(json!(t.find_callers(id, depth)?))
                })
            }),
        );
        let c2 = conn.clone();
        self.register("get_callees", "Find what a function or method calls",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"max_depth":{"type":"integer","default":3}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let depth = params["max_depth"].as_i64().unwrap_or(3);
                Self::conn(&c2, |conn| {
                    let t = GraphTraversal::new(conn);
                    Ok(json!(t.find_callees(id, depth)?))
                })
            }),
        );
        let c3 = conn.clone();
        self.register("get_imports", "Get all imports of a file or module",
            json!({"type":"object","properties":{"node_id":{"type":"integer"}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                Self::conn(&c3, |conn| {
                    let t = GraphTraversal::new(conn);
                    Ok(json!(t.get_related_by_edge(id, "imports", "outgoing")?))
                })
            }),
        );
        let c4 = conn.clone();
        self.register("get_dependents", "Find all code that directly depends on a symbol",
            json!({"type":"object","properties":{"node_id":{"type":"integer"}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                Self::conn(&c4, |conn| {
                    let t = GraphTraversal::new(conn);
                    let deps = t.get_dependents(id)?;
                    Ok(json!(deps.into_iter().map(|(_, n)| n).collect::<Vec<_>>()))
                })
            }),
        );
        let c5 = conn.clone();
        self.register("impact_analysis", "Analyze blast radius of changing a symbol",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"max_depth":{"type":"integer","default":5}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let depth = params["max_depth"].as_i64().unwrap_or(5);
                Self::conn(&c5, |conn| {
                    let impact = ImpactAnalysis::new(conn);
                    let result = impact.analyze(id, depth)?;

                    // Touch ack file so codebase-guard allows edits for the next 10 min
                    let project_res = conn.query_row(
                        "SELECT p.root_path FROM projects p JOIN nodes n ON n.project_id = p.id WHERE n.id = ?1",
                        rusqlite::params![id],
                        |row| row.get::<_, String>(0),
                    );
                    if let Ok(root_path) = project_res {
                        let ack_dir = std::path::Path::new(&root_path)
                            .join(".codebase-synapse")
                            .join("acks");
                        let _ = std::fs::create_dir_all(&ack_dir);
                        let hash = {
                            let mut h: u64 = 14695981039346656037;
                            for b in result.symbol.file_path.as_bytes() {
                                h = h.wrapping_mul(1099511628211);
                                h ^= *b as u64;
                            }
                            h
                        };
                        let _ = std::fs::write(ack_dir.join(format!("{:x}", hash)), b"");
                    }

                    Ok(json!(result))
                })
            }),
        );
        let c6 = conn.clone();
        self.register("find_path", "Find call path between two symbols",
            json!({"type":"object","properties":{"from_node_id":{"type":"integer"},"to_node_id":{"type":"integer"},"max_depth":{"type":"integer","default":10}},"required":["from_node_id","to_node_id"]}),
            Arc::new(move |params| {
                let from = params["from_node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing from_node_id"))?;
                let to = params["to_node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing to_node_id"))?;
                let depth = params["max_depth"].as_i64().unwrap_or(10);
                Self::conn(&c6, |conn| {
                    let t = GraphTraversal::new(conn);
                    Ok(json!(t.find_path(from, to, depth)?))
                })
            }),
        );
        let c7 = conn.clone();
        self.register("find_dead_code", "Find potentially unused functions and methods",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c7, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let t = GraphTraversal::new(conn);
                    Ok(json!(t.find_dead_code(project.id)?))
                })
            }),
        );
        let c8 = conn.clone();
        self.register("get_file_structure", "Get structural overview of a file",
            json!({"type":"object","properties":{"project":{"type":"string"},"file_path":{"type":"string"}},"required":["project","file_path"]}),
            Arc::new(move |params| {
                let fp = params["file_path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing file_path"))?;
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c8, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let t = GraphTraversal::new(conn);
                    Ok(json!(t.get_file_structure(project.id, fp)?))
                })
            }),
        );
        let c9 = conn.clone();
        self.register("project_overview", "Get high-level statistics of a project",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c9, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let stats = db::queries::get_project_stats(conn, project.id)?;
                    Ok(json!({"project":pname,"stats":stats}))
                })
            }),
        );
        let c10 = conn.clone();
        self.register("query_graph", "Execute an openCypher read-only query on the codebase knowledge graph",
            json!({"type":"object","properties":{"project":{"type":"string"},"query":{"type":"string"}},"required":["project","query"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let query = params["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                Self::conn(&c10, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    crate::cypher::query_graph(conn, project.id, query).map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        );
        let c11 = conn.clone();
        self.register("get_route_map", "List all HTTP routes and their handlers in the project",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c11, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let route_map = db::queries::get_route_map(conn, project.id)?;
                    Ok(json!(route_map.into_iter().map(|(r, h)| {
                        json!({
                            "route": r,
                            "handler": h
                        })
                    }).collect::<Vec<_>>()))
                })
            }),
        );
        let c12 = conn.clone();
        self.register("find_similar", "Find structurally similar functions/methods to a given symbol",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"threshold":{"type":"number","default":0.70}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let thresh = params["threshold"].as_f64().unwrap_or(0.70);
                Self::conn(&c12, |conn| {
                    use crate::db::schema::Node;
                    let mut stmt = conn.prepare(
                        "SELECT e.id, e.project_id, e.source_node_id, e.target_node_id, e.kind, e.metadata,
                                n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                                n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported,
                                n.content_hash, n.source, n.metadata, n.created_at, n.updated_at
                         FROM edges e JOIN nodes n ON n.id = e.target_node_id
                         WHERE e.source_node_id = ?1 AND e.kind = 'similar_to'
                         UNION ALL
                         SELECT e.id, e.project_id, e.source_node_id, e.target_node_id, e.kind, e.metadata,
                                n.id, n.project_id, n.file_path, n.kind, n.name, n.qualified_name, n.signature,
                                n.doc_comment, n.start_line, n.end_line, n.complexity, n.is_exported,
                                n.content_hash, n.source, n.metadata, n.created_at, n.updated_at
                         FROM edges e JOIN nodes n ON n.id = e.source_node_id
                         WHERE e.target_node_id = ?1 AND e.kind = 'similar_to'"
                    )?;

                    let rows = stmt.query_map(rusqlite::params![id], |row| {
                        let edge_metadata: Option<String> = row.get(5)?;
                        let score = edge_metadata
                            .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
                            .and_then(|json| json.get("jaccard_score").and_then(|s| s.as_f64()))
                            .unwrap_or(0.0);

                        let node = Node {
                            id: row.get(6)?,
                            project_id: row.get(7)?,
                            file_path: row.get(8)?,
                            kind: row.get(9)?,
                            name: row.get(10)?,
                            qualified_name: row.get(11)?,
                            signature: row.get(12)?,
                            doc_comment: row.get(13)?,
                            start_line: row.get(14)?,
                            end_line: row.get(15)?,
                            complexity: row.get(16)?,
                            is_exported: row.get(17)?,
                            content_hash: row.get(18)?,
                            source: row.get(19)?,
                            metadata: row.get(20)?,
                            created_at: String::new(),
                            updated_at: String::new(),
                        };

                        Ok((node, score))
                    })?;

                    let mut results = Vec::new();
                    for r in rows {
                        let (node, score) = r?;
                        if score >= thresh {
                            results.push(json!({
                                "node": node,
                                "jaccard_score": score
                            }));
                        }
                    }
                    Ok(json!(results))
                })
            }),
        );
        let c13 = conn.clone();
        self.register("get_architecture", "Get project architecture overview: languages, entry points, packages, hotspots, dead code, and test coverage",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c13, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    crate::mcp::architecture::get_project_architecture(conn, project.id)
                })
            }),
        );
        let c14 = conn.clone();
        self.register("manage_adr", "Manage Architecture Decision Records (ADRs): CRUD operations",
            json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string"},
                    "action": {"type": "string", "enum": ["create", "get", "list", "update", "delete"]},
                    "adr_id": {"type": "integer"},
                    "title": {"type": "string"},
                    "status": {"type": "string", "enum": ["proposed", "accepted", "deprecated", "superseded"]},
                    "context": {"type": "string"},
                    "decision": {"type": "string"},
                    "consequences": {"type": "string"}
                },
                "required": ["project", "action"]
            }),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let action = params["action"].as_str().ok_or_else(|| anyhow::anyhow!("Missing action"))?;
                Self::conn(&c14, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    match action {
                        "create" => {
                            let title = params["title"].as_str().ok_or_else(|| anyhow::anyhow!("Missing title"))?;
                            let status = params["status"].as_str().unwrap_or("proposed");
                            let context = params["context"].as_str().unwrap_or("");
                            let decision = params["decision"].as_str().unwrap_or("");
                            let consequences = params["consequences"].as_str().unwrap_or("");
                            let adr_id = db::queries::insert_adr(conn, project.id, title, status, context, decision, consequences)?;
                            Ok(json!({ "status": "created", "adr_id": adr_id }))
                        }
                        "get" => {
                            let adr_id = params["adr_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing adr_id"))?;
                            let adr = db::queries::get_adr(conn, adr_id)?;
                            Ok(json!(adr))
                        }
                        "list" => {
                            let list = db::queries::list_adrs(conn, project.id)?;
                            Ok(json!(list))
                        }
                        "update" => {
                            let adr_id = params["adr_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing adr_id"))?;
                            let title = params["title"].as_str().ok_or_else(|| anyhow::anyhow!("Missing title"))?;
                            let status = params["status"].as_str().ok_or_else(|| anyhow::anyhow!("Missing status"))?;
                            let context = params["context"].as_str().unwrap_or("");
                            let decision = params["decision"].as_str().unwrap_or("");
                            let consequences = params["consequences"].as_str().unwrap_or("");
                            db::queries::update_adr(conn, adr_id, title, status, context, decision, consequences)?;
                            Ok(json!({ "status": "updated" }))
                        }
                        "delete" => {
                            let adr_id = params["adr_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing adr_id"))?;
                            db::queries::delete_adr(conn, adr_id)?;
                            Ok(json!({ "status": "deleted" }))
                        }
                        _ => Err(anyhow::anyhow!("Invalid action")),
                    }
                })
            }),
        );

        let c_pr = conn.clone();
        self.register(
            "get_pagerank",
            "Get the PageRank authority score for a node. Scores > 0.05 indicate critical architectural hubs that require impact_analysis before modification.",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"project":{"type":"string"}},"required":["node_id","project"]}),
            Arc::new(move |params| {
                let node_id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c_pr, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let rank = db::queries::get_node_pagerank(conn, node_id)?;
                    let top = db::queries::get_top_pagerank_nodes(conn, project.id, 10)?;
                    Ok(json!({
                        "node_id": node_id,
                        "pagerank_score": rank,
                        "is_hub": rank > 0.05,
                        "top_10_hubs": top.iter().map(|(id, r)| json!({"node_id": id, "pagerank": r})).collect::<Vec<_>>()
                    }))
                })
            }),
        );
        let c_cl = conn.clone();
        self.register(
            "get_clusters",
            "Get Leiden cluster assignments for all files. Each cluster is a cohesive architectural module detected from import patterns. Use with suggest_boundaries to enforce module separation.",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c_cl, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let mut stmt = conn.prepare(
                        "SELECT file_path, cluster_id FROM file_clusters WHERE project_id = ?1 ORDER BY cluster_id, file_path"
                    )?;
                    let rows: Vec<(String, i64)> = stmt
                        .query_map(rusqlite::params![project.id], |row| Ok((row.get(0)?, row.get(1)?)))?
                        .filter_map(|r| r.ok()).collect();
                    let cluster_count = rows.iter().map(|(_, c)| c)
                        .collect::<std::collections::HashSet<_>>().len();
                    Ok(json!({
                        "project": pname,
                        "cluster_count": cluster_count,
                        "clusters": rows.iter().map(|(fp, cid)| json!({"file_path": fp, "cluster_id": cid})).collect::<Vec<_>>()
                    }))
                })
            }),
        );
        let c_bd = conn.clone();
        self.register(
            "check_boundaries",
            "Check if the import graph violates architecture rules defined in .codebase-synapse/boundaries.toml. Returns list of violations (from_file, to_file, rule). Run suggest_boundaries first if no config exists.",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c_bd, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let config_path = std::path::Path::new(&project.root_path)
                        .join(".codebase-synapse").join("boundaries.toml");
                    if !config_path.exists() {
                        return Ok(json!({"status":"no_config","message":"Run suggest_boundaries first","violations":[]}));
                    }
                    let config = crate::graph::boundaries::load_config(&config_path)?;
                    let mut stmt = conn.prepare(
                        "SELECT DISTINCT file_path, MIN(id) as id FROM nodes WHERE project_id=?1 AND kind='file' GROUP BY file_path"
                    )?;
                    let files: Vec<(String, i64)> = {
                        let res = stmt.query_map(rusqlite::params![project.id], |row| Ok((row.get(0)?, row.get(1)?)))?
                            .filter_map(|r| r.ok()).collect();
                        res
                    };
                    let edges = db::queries::get_all_import_edges(conn, project.id)?;
                    let violations = crate::graph::check_boundaries(&config, &files, &edges)?;
                    Ok(json!({
                        "project": pname,
                        "violation_count": violations.len(),
                        "violations": violations.iter().map(|v| json!({
                            "from_file": v.from_file, "to_file": v.to_file,
                            "rule_index": v.rule_index, "deny_pattern": v.deny_pattern
                        })).collect::<Vec<_>>()
                    }))
                })
            }),
        );

        let c_sb = conn.clone();
        self.register(
            "suggest_boundaries",
            "Generate a starter .codebase-synapse/boundaries.toml based on Leiden cluster assignments. Freezes current architecture so boundary drift is intentional.",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c_sb, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let mut stmt = conn.prepare(
                        "SELECT n.id, fc.cluster_id, n.file_path
                         FROM file_clusters fc JOIN nodes n ON n.file_path=fc.file_path AND n.project_id=fc.project_id
                         WHERE fc.project_id=?1"
                     )?;
                    let rows: Vec<(i64,i64,String)> = {
                        let res = stmt.query_map(rusqlite::params![project.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?
                            .filter_map(|r| r.ok()).collect();
                        res
                    };
                    let mut assignments = std::collections::HashMap::new();
                    let mut id_to_path = std::collections::HashMap::new();
                    for (nid, cid, fp) in rows { assignments.insert(nid, cid); id_to_path.insert(nid, fp); }
                    let toml_text = crate::graph::suggest_boundaries(&assignments, &id_to_path);
                    Ok(json!({"project": pname, "boundaries_toml": toml_text,
                        "save_to": format!("{}/.codebase-synapse/boundaries.toml", project.root_path)}))
                })
            }),
        );
        let c_wiki = conn.clone();
        self.register(
            "generate_wiki",
            "Generate a Markdown architecture wiki for the project from Leiden clusters. Includes cluster breakdown, file lists, and boundary violations.",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c_wiki, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let cfg = crate::graph::WikiConfig { project_name: project.name.clone(), max_files_per_cluster: 20 };
                    let md = crate::graph::generate_wiki(conn, project.id, &cfg)?;
                    Ok(json!({"project": pname, "wiki_markdown": md}))
                })
            }),
        );
    }

    fn register_memory_tools(&mut self, conn: &SharedConn, session: &Arc<Mutex<SessionMemory>>) {
        let c = conn.clone();
        let _s = session.clone();
        self.register("memory_store", "Store a persistent note, fact, or decision",
            json!({"type":"object","properties":{"project":{"type":"string"},"content":{"type":"string"},"kind":{"type":"string","enum":["note","fact","decision","insight"],"default":"note"},"tags":{"type":"string"},"node_id":{"type":"integer"}},"required":["project","content"]}),
            Arc::new(move |params| {
                let content = params["content"].as_str().ok_or_else(|| anyhow::anyhow!("Missing content"))?;
                let kind = params["kind"].as_str().unwrap_or("note");
                let tags = params["tags"].as_str();
                let node_id = params["node_id"].as_i64();
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let store = MemoryStore::new(conn);
                    let id = store.store(project.id, content, node_id, kind, tags)?;
                    Ok(json!({"id":id,"status":"stored"}))
                })
            }),
        );
        let c2 = conn.clone();
        self.register("memory_search", "Search stored memories",
            json!({"type":"object","properties":{"project":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","default":20}},"required":["project","query"]}),
            Arc::new(move |params| {
                let query = params["query"].as_str().ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                let limit = params["limit"].as_i64().unwrap_or(20);
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c2, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let store = MemoryStore::new(conn);
                    Ok(json!(store.search(project.id, query, limit)?))
                })
            }),
        );
        let c3 = conn.clone();
        self.register("memory_list", "List stored memories by kind",
            json!({"type":"object","properties":{"project":{"type":"string"},"kind":{"type":"string","enum":["note","fact","decision","insight"]}},"required":["project"]}),
            Arc::new(move |params| {
                let kind = params["kind"].as_str();
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c3, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let store = MemoryStore::new(conn);
                    Ok(json!(store.list(project.id, kind)?))
                })
            }),
        );
        let c4 = conn.clone();
        self.register(
            "memory_delete",
            "Delete a memory by ID",
            json!({"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}),
            Arc::new(move |params| {
                let id = params["id"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
                Self::conn(&c4, |conn| {
                    let store = MemoryStore::new(conn);
                    store.delete(id)?;
                    Ok(json!({"status":"deleted"}))
                })
            }),
        );
        let s2 = session.clone();
        self.register("session_remember", "Store a key-value fact in the current session",
            json!({"type":"object","properties":{"key":{"type":"string"},"value":{"type":"string"},"source":{"type":"string","default":"agent"}},"required":["key","value"]}),
            Arc::new(move |params| {
                let key = params["key"].as_str().ok_or_else(|| anyhow::anyhow!("Missing key"))?;
                let value = params["value"].as_str().ok_or_else(|| anyhow::anyhow!("Missing value"))?;
                let source = params["source"].as_str().unwrap_or("agent");
                if let Ok(mut s) = s2.lock() {
                    s.remember(key, value, source);
                }
                Ok(json!({"status":"remembered"}))
            }),
        );
        let s3 = session.clone();
        self.register(
            "session_recall",
            "Recall a fact from the current session",
            json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}),
            Arc::new(move |params| {
                let key = params["key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing key"))?;
                let entry = s3.lock().ok().and_then(|s| s.recall(key).cloned());
                Ok(json!(entry))
            }),
        );
    }

    fn register_context_tools(&mut self, conn: &SharedConn) {
        let c = conn.clone();
        self.register("get_context", "Get a symbol with dependencies and dependents",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"max_depth":{"type":"integer","default":2}},"required":["node_id"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let depth = params["max_depth"].as_i64().unwrap_or(2);
                Self::conn(&c, |conn| {
                    let node = db::queries::get_node_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("Node not found"))?;
                    let _ = db::queries::record_node_access(conn, id, node.project_id);
                    let t = GraphTraversal::new(conn);
                    let callers = t.find_callers(id, depth)?;
                    let callees = t.find_callees(id, depth)?;
                    Ok(json!({"symbol":node,"callers":callers,"callees":callees}))
                })
            }),
        );
        let c2 = conn.clone();
        self.register("get_edit_context", "Everything needed before editing",
            json!({"type":"object","properties":{"node_id":{"type":"integer"},"project":{"type":"string"}},"required":["node_id","project"]}),
            Arc::new(move |params| {
                let id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c2, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let node = db::queries::get_node_by_id(conn, id)?.ok_or_else(|| anyhow::anyhow!("Node not found"))?;
                    let _ = db::queries::record_node_access(conn, id, node.project_id);
                    let t = GraphTraversal::new(conn);
                    let callers = t.find_callers(id, 1)?;
                    let callees = t.find_callees(id, 1)?;
                    let memories = db::queries::search_memory_notes(conn, project.id, &node.name.clone().unwrap_or_default(), 10)?;
                    Ok(json!({"symbol":node,"callers":callers,"callees":callees,"memories":memories}))
                })
            }),
        );
    }

    fn register_utility_tools(&mut self, conn: &SharedConn, config: &Arc<Config>) {
        let _c = conn.clone();
        let cfg = config.clone();
        self.register(
            "get_status",
            "Get server health and index status",
            json!({"type":"object","properties":{}}),
            Arc::new(move |_| {
                let db_size = std::fs::metadata(cfg.db_path())
                    .map(|m| m.len())
                    .unwrap_or(0);
                let data_dir = &cfg.data_dir;
                // Check if git history is indexed
                let git_indexed = data_dir.join("git_history.db").exists()
                    || std::fs::read_dir(data_dir)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .any(|e| e.file_name().to_string_lossy().contains("git"))
                        })
                        .unwrap_or(false);
                // Check if embeddings are available
                let embeddings_file = data_dir.join("embeddings.db");
                let embeddings_available = embeddings_file.exists();
                Ok(json!({
                    "status": "ok",
                    "version": env!("CARGO_PKG_VERSION"),
                    "db_size_bytes": db_size,
                    "data_dir": cfg.data_dir,
                    "git_history_indexed": git_indexed,
                    "embeddings_available": embeddings_available
                }))
            }),
        );
        let c2 = conn.clone();
        self.register("get_stats", "Get detailed statistics about indexed data",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c2, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    Ok(json!(db::queries::get_project_stats(conn, project.id)?))
                })
            }),
        );
    }

    fn register_archaeology_tools(&mut self, conn: &SharedConn, embedder: &Arc<dyn Embedder>) {
        // ─── git_archaeology ───
        let c1 = conn.clone();
        self.register(
            "git_archaeology",
            "Why does this code exist? Returns git history for a symbol: when introduced, who changed it, and why (feat/fix/refactor/perf). Run index_git_history first.",
            json!({"type":"object","properties":{"project":{"type":"string"},"node_id":{"type":"integer"}},"required":["project","node_id"]}),
            Arc::new(move |params| {
                let node_id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c1, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let arch = GitArchaeologist::new(conn, project.id);
                    arch.archaeology_narrative(node_id)
                })
            }),
        );

        // ─── index_git_history ───
        let c2 = conn.clone();
        self.register(
            "index_git_history",
            "Index git commit history for a project. Enables get_hotspots and git_archaeology. Run once after index_repository. max_commits defaults to 500.",
            json!({"type":"object","properties":{"project":{"type":"string"},"repo_path":{"type":"string"},"max_commits":{"type":"integer","default":500}},"required":["project","repo_path"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let repo_path = params["repo_path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing repo_path"))?;
                let max_commits = params["max_commits"].as_i64().unwrap_or(500) as usize;
                Self::conn(&c2, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let arch = GitArchaeologist::new(conn, project.id);
                    let count = arch.index_history(repo_path, max_commits)?;
                    Ok(json!({"status":"indexed","commits_processed":count}))
                })
            }),
        );

        // ─── get_hotspots ───
        let c3 = conn.clone();
        self.register(
            "get_hotspots",
            "Technical debt hotspots: symbols with high complexity × git churn. Score = complexity × ln(1+churn). Requires index_git_history.",
            json!({"type":"object","properties":{"project":{"type":"string"},"limit":{"type":"integer","default":20}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let limit = params["limit"].as_i64().unwrap_or(20);
                Self::conn(&c3, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let analyzer = HotspotAnalyzer::new(conn);
                    Ok(json!({"project":pname,"hotspots":analyzer.get_hotspots(project.id, limit)?}))
                })
            }),
        );

        // ─── technical_debt_map ───
        let c4 = conn.clone();
        self.register(
            "technical_debt_map",
            "File-level technical debt map: which files have the highest combined complexity × churn. Identifies which files to refactor first.",
            json!({"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c4, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let analyzer = HotspotAnalyzer::new(conn);
                    Ok(json!({"project":pname,"debt_map":analyzer.technical_debt_map(project.id)?}))
                })
            }),
        );

        // ─── get_contracts ───
        let c5 = conn.clone();
        self.register(
            "get_contracts",
            "What tests verify this symbol? Returns test contracts (test_of edges). Use BEFORE modifying any symbol to know what tests will break.",
            json!({"type":"object","properties":{"project":{"type":"string"},"node_id":{"type":"integer"}},"required":["project","node_id"]}),
            Arc::new(move |params| {
                let node_id = params["node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing node_id"))?;
                let _pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c5, |conn| {
                    let t = crate::graph::GraphTraversal::new(conn);
                    let tests = t.get_related_by_edge(node_id, "test_of", "incoming")?;
                    Ok(json!({
                        "node_id": node_id,
                        "test_count": tests.len(),
                        "warning": if tests.is_empty() {
                            "No test contracts found — consider adding tests before modifying this symbol."
                        } else {
                            "Modifying this symbol may break the listed test contracts."
                        },
                        "contracts": tests
                    }))
                })
            }),
        );

        // ─── get_recent_semantic_changes ───
        let c6 = conn.clone();
        self.register(
            "get_recent_semantic_changes",
            "What changed semantically in the last N hours? Returns structural changes: symbols added/removed, complexity shifts. More informative than 'which files changed'.",
            json!({"type":"object","properties":{"project":{"type":"string"},"hours":{"type":"integer","default":24}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let hours = params["hours"].as_i64().unwrap_or(24);
                Self::conn(&c6, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let changes = db::queries::get_recent_semantic_changes(conn, project.id, hours)?;
                    Ok(json!({"project":pname,"window_hours":hours,"change_count":changes.len(),"changes":changes}))
                })
            }),
        );

        // ─── get_working_set ───
        let c7 = conn.clone();
        self.register(
            "get_working_set",
            "What is the AI's current working set? Returns most-accessed symbols in the last 24h with temporal decay weighting. Call at session start for efficient context preloading.",
            json!({"type":"object","properties":{"project":{"type":"string"},"limit":{"type":"integer","default":30}},"required":["project"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let limit = params["limit"].as_i64().unwrap_or(30);
                Self::conn(&c7, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let ws = db::queries::get_working_set(conn, project.id, limit)?;
                    Ok(json!({"project":pname,"working_set_size":ws.len(),
                        "nodes": ws.iter().map(|(n, cnt)| json!({"id":n.id,"name":n.name,"kind":n.kind,"file":n.file_path,"access_count":cnt})).collect::<Vec<_>>()
                    }))
                })
            }),
        );

        // ─── find_symbol_everywhere ───
        let c8 = conn.clone();
        self.register(
            "find_symbol_everywhere",
            "Search for a symbol across ALL indexed projects (cross-project linking). Useful for monorepos and shared libraries.",
            json!({"type":"object","properties":{"symbol_name":{"type":"string"}},"required":["symbol_name"]}),
            Arc::new(move |params| {
                let name = params["symbol_name"].as_str().ok_or_else(|| anyhow::anyhow!("Missing symbol_name"))?;
                Self::conn(&c8, |conn| {
                    let nodes = db::queries::find_cross_project_symbol(conn, name)?;
                    Ok(json!({"symbol":name,"found_in_projects":nodes.iter().map(|n|n.project_id).collect::<std::collections::HashSet<_>>().len(),"nodes":nodes}))
                })
            }),
        );

        // ─── prepare_task_context ───
        let c9 = conn.clone();
        let emb = embedder.clone();
        self.register(
            "prepare_task_context",
            "🎯 CALL FIRST: Given a natural-language task description, assembles an optimal context bundle — relevant symbols, their dependencies, impact analysis, related memories — all within a token budget. Eliminates the need for manual exploration.",
            json!({"type":"object","properties":{"project":{"type":"string"},"task":{"type":"string"},"max_tokens":{"type":"integer","default":4000}},"required":["project","task"]}),
            Arc::new(move |params| {
                let pname = params["project"].as_str().unwrap_or("default");
                let task = params["task"].as_str().ok_or_else(|| anyhow::anyhow!("Missing task"))?;
                let max_tokens = params["max_tokens"].as_i64().unwrap_or(4000) as usize;
                let embedding = emb.embed(&[task]).unwrap_or_default();
                let query_vec = embedding.first().cloned();
                Self::conn(&c9, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    let budget = ContextBudget::new(conn, project.id);
                    budget.prepare(task, max_tokens, query_vec.as_deref())
                })
            }),
        );

        // ─── link_cross_project ───
        let c_xp = conn.clone();
        self.register(
            "link_cross_project",
            "Create a cross-project dependency edge between two nodes in different projects.",
            json!({"type":"object","properties":{"source_node_id":{"type":"integer"},"target_node_id":{"type":"integer"},"project":{"type":"string"}},"required":["source_node_id","target_node_id","project"]}),
            Arc::new(move |params| {
                let src = params["source_node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing source_node_id"))?;
                let tgt = params["target_node_id"].as_i64().ok_or_else(|| anyhow::anyhow!("Missing target_node_id"))?;
                let pname = params["project"].as_str().unwrap_or("default");
                Self::conn(&c_xp, |conn| {
                    let project = db::queries::get_project(conn, pname)?.ok_or_else(|| anyhow::anyhow!("Project not found"))?;
                    db::queries::link_cross_project(conn, project.id, src, tgt)?;
                    Ok(json!({"status": "linked", "source": src, "target": tgt}))
                })
            }),
        );
    }

    pub fn get_tool_definitions(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema
                })
            })
            .collect()
    }

    pub fn handle(&self, name: &str, params: Value) -> Result<Value> {
        self.handlers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", name))?(params)
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

fn serialize_search_results(results: Vec<SearchResult>) -> Value {
    json!(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::migrate;

    fn test_registry() -> (
        ToolRegistry,
        SharedConn,
        Arc<Config>,
        Arc<Indexer>,
        Arc<dyn Embedder>,
    ) {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        {
            let c = conn.lock().unwrap();
            migrate(&c).unwrap();
        }
        let config = Arc::new(Config {
            data_dir: std::path::PathBuf::from("/tmp/.codebase-synapse-test"),
            project_root: None,
            graph_only: false,
            log_level: "off".into(),
            watch: false,
        });
        let indexer = Arc::new(Indexer::new(config.clone(), conn.clone()));
        let embedder = Arc::new(crate::embedding::NoopEmbedder);
        let registry = ToolRegistry::new(
            conn.clone(),
            config.clone(),
            indexer.clone(),
            embedder.clone(),
        );
        (registry, conn, config, indexer, embedder)
    }

    #[test]
    fn test_tool_registry_creation() {
        let (registry, _, _, _, _) = test_registry();
        let defs = registry.get_tool_definitions();
        assert!(
            defs.len() >= 20,
            "Expected at least 20 tools, got {}",
            defs.len()
        );
        let names: Vec<&str> = defs.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"index_repository"));
        assert!(names.contains(&"search_symbol"));
        assert!(names.contains(&"memory_store"));
        assert!(names.contains(&"get_status"));
    }

    #[test]
    fn test_tool_not_found() {
        let (registry, _, _, _, _) = test_registry();
        let result = registry.handle("nonexistent_tool", json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_has_tool() {
        let (registry, _, _, _, _) = test_registry();
        assert!(registry.has_tool("get_status"));
        assert!(!registry.has_tool("fake_tool"));
    }

    #[test]
    fn test_get_status_tool() {
        let (registry, _, _, _, _) = test_registry();
        let result = registry.handle("get_status", json!({})).unwrap();
        assert_eq!(result["status"], "ok");
        assert!(result["version"].as_str().is_some());
    }

    #[test]
    fn test_get_stats_tool_missing_project() {
        let (registry, _, _, _, _) = test_registry();
        let result = registry.handle("get_stats", json!({"project": "nonexistent"}));
        assert!(result.is_err());
    }
}
