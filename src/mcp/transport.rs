use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use anyhow::Result;
use serde_json::json;
use serde_json::Value;
use tracing::{info, warn};

static SHUTDOWN: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

use super::protocol::*;
use super::tools::ToolRegistry;

pub struct McpTransport {
    registry: Arc<ToolRegistry>,
}

impl McpTransport {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }

    pub fn run(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();

        for line in stdin.lock().lines() {
            if SHUTDOWN.load(Ordering::Relaxed) {
                break;
            }
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    warn!("Error reading stdin: {}", e);
                    break;
                }
            };

            if let Some(output) = self.handle_message(&line)? {
                if writeln!(writer, "{}", output).is_err() || writer.flush().is_err() {
                    warn!("MCP client disconnected, shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn handle_message(&self, line: &str) -> Result<Option<String>> {
        if line.trim().is_empty() {
            return Ok(None);
        }

        let message: McpMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to parse message: {} (raw: {})", e, &line[..line.len().min(200)]);
                let err_msg = McpMessage::error(0, McpError::parse_error());
                return Ok(Some(serde_json::to_string(&err_msg)?));
            }
        };

        match message {
            McpMessage::Request { jsonrpc: _, id, method, params } => {
                let response = self.handle_request(id, &method, params);
                let output = serde_json::to_string(&response)?;
                Ok(Some(output))
            }
            McpMessage::Notification { method, params, .. } => {
                self.handle_notification(&method, params);
                Ok(None)
            }
            McpMessage::Response { .. } => Ok(None),
        }
    }

    fn handle_request(&self, id: u64, method: &str, params: Value) -> McpMessage {
        match method {
            "initialize" => {
                info!("MCP client initialized");
                McpMessage::success(
                    id,
                    json!(InitializeResult {
                        protocol_version: "2025-03-26".into(),
                        capabilities: ServerCapabilities {
                            tools: {
                                let mut m = serde_json::Map::new();
                                m.insert("listChanged".into(), json!(false));
                                m
                            },
                            resources: None,
                            prompts: None,
                        },
                        server_info: ServerInfo {
                            name: "codebase-synapse".into(),
                            version: env!("CARGO_PKG_VERSION").into(),
                        },
                    }),
                )
            }

            "notified" | "initialized" => McpMessage::success(id, json!({})),

            "tools/list" => {
                let tools = self.registry.get_tool_definitions();
                McpMessage::success(id, json!({ "tools": tools }))
            }

            "tools/call" => {
                let tool_name = params["name"].as_str().unwrap_or("");
                let tool_params = params.get("arguments").cloned().unwrap_or(json!({}));

                if !self.registry.has_tool(tool_name) {
                    return McpMessage::error(id, McpError::method_not_found());
                }

                match self.registry.handle(tool_name, tool_params) {
                    Ok(result) => McpMessage::success(id, json!({ "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                    }]})),
                    Err(e) => {
                        warn!("Tool '{}' error: {}", tool_name, e);
                        McpMessage::success(id, json!({ "content": [{
                            "type": "text",
                            "text": format!("Error: {}", e)
                        }], "isError": true }))
                    }
                }
            }

            _ => McpMessage::success(id, json!({})),
        }
    }

    fn handle_notification(&self, method: &str, _params: Value) {
        if method == "exit" {
            info!("Received exit notification, shutting down");
            SHUTDOWN.store(true, Ordering::Relaxed);
        }
    }
}
