mod common;

use serde_json::json;
use common::setup_test_server;

#[test]
fn test_initialize_handshake() {
    let server = setup_test_server();
    
    let handshake_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    }).to_string();
    
    let res_str = server.transport.handle_message(&handshake_req).unwrap().unwrap();
    let res: serde_json::Value = serde_json::from_str(&res_str).unwrap();
    
    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(res["id"], 1);
    assert_eq!(res["result"]["serverInfo"]["name"], "codebase-synapse");
    assert_eq!(res["result"]["protocolVersion"], "2025-03-26");
}

#[test]
fn test_list_tools() {
    let server = setup_test_server();
    
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "tools/list",
        "params": {}
    }).to_string();
    
    let res_str = server.transport.handle_message(&list_req).unwrap().unwrap();
    let res: serde_json::Value = serde_json::from_str(&res_str).unwrap();
    
    assert_eq!(res["id"], 42);
    let tools = res["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty(), "Tool registry should expose tools");
    
    let has_index_tool = tools.iter().any(|t| t["name"] == "index_repository");
    assert!(has_index_tool, "Exposes index_repository tool");
}

#[test]
fn test_unknown_tool() {
    let server = setup_test_server();
    
    let unknown_req = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": "not_a_tool",
            "arguments": {}
        }
    }).to_string();
    
    let res_str = server.transport.handle_message(&unknown_req).unwrap().unwrap();
    let res: serde_json::Value = serde_json::from_str(&res_str).unwrap();
    
    assert!(res.get("error").is_some());
    assert_eq!(res["error"]["code"], -32601); // Method not found
}
