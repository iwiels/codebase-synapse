#![allow(dead_code, unused_imports)]
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tempfile::TempDir;

use codebase_synapse::config::Config;
use codebase_synapse::db;
use codebase_synapse::embedding::Embedder;
use codebase_synapse::indexer::Indexer;
use codebase_synapse::mcp::{McpTransport, ToolRegistry};

pub struct MockEmbedder;

impl Embedder for MockEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(vec![vec![0.1; 384]; texts.len()])
    }
    fn dimensions(&self) -> usize {
        384
    }
}

pub struct TestServer {
    pub conn: Arc<Mutex<Connection>>,
    pub registry: Arc<ToolRegistry>,
    pub transport: McpTransport,
    pub temp_dir: TempDir,
}

pub fn setup_test_server() -> TestServer {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("codebase.db");
    let conn = db::open(&db_path).unwrap();
    let conn = Arc::new(Mutex::new(conn));

    let config = Arc::new(Config {
        data_dir: temp_dir.path().to_path_buf(),
        project_root: Some(temp_dir.path().to_path_buf()),
        graph_only: false,
        log_level: "info".to_string(),
        watch: false,
    });

    let indexer = Arc::new(Indexer::new(config.clone(), conn.clone()));
    let embedder = Arc::new(MockEmbedder);
    let registry = Arc::new(ToolRegistry::new(
        conn.clone(),
        config.clone(),
        indexer.clone(),
        embedder,
    ));
    let transport = McpTransport::new(registry.clone());

    TestServer {
        conn,
        registry,
        transport,
        temp_dir,
    }
}

pub fn tool_call_json(tool_name: &str, args: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": args
        }
    })
    .to_string()
}

pub fn parse_tool_result(response: &Option<String>) -> serde_json::Value {
    let resp = response.as_ref().expect("parse_tool_result: response was None");
    let parsed: serde_json::Value = serde_json::from_str(resp)
        .unwrap_or_else(|e| panic!("parse_tool_result: invalid JSON: {e}\nraw: {resp}"));
    
    if let Some(err) = parsed.get("error") {
        panic!("JSON-RPC error: {:?}", err);
    }
    
    let result = parsed.get("result").expect("parse_tool_result: no result field");
    if result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
        panic!("Tool execution error: {:?}", result.get("content"));
    }
    
    let content = result.get("content").expect("parse_tool_result: no content field");
    let text = content[0]["text"].as_str()
        .unwrap_or_else(|| panic!("parse_tool_result: unexpected response shape: {parsed}"));
    
    serde_json::from_str(text)
        .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
}
