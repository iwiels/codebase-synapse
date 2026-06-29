mod common;

use std::fs;
use serde_json::json;
use common::{setup_test_server, tool_call_json, parse_tool_result};

#[test]
fn test_e2e_indexing_and_searching() {
    let server = setup_test_server();
    let root = server.temp_dir.path();

    // Create test files
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let rust_file = src_dir.join("lib.rs");
    fs::write(&rust_file, r#"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn greet() {
    println!("hello");
}
"#).unwrap();

    let ts_file = src_dir.join("main.ts");
    fs::write(&ts_file, r#"
export function handleRequest() {
    console.log("handling");
}
"#).unwrap();

    // 1. Index Repository
    let index_req = tool_call_json("index_repository", json!({ "repo_path": root.to_str().unwrap() }));
    let index_res_str = server.transport.handle_message(&index_req).unwrap();
    let index_res = parse_tool_result(&index_res_str);
    assert_eq!(index_res["status"], "indexed");

    // 2. List Projects
    let list_req = tool_call_json("list_projects", json!({}));
    let list_res_str = server.transport.handle_message(&list_req).unwrap();
    let list_res = parse_tool_result(&list_res_str);
    let projects = list_res["projects"].as_array().unwrap();
    assert!(!projects.is_empty(), "projects should not be empty");

    let proj_name = projects[0]["name"].as_str().unwrap();

    // 3. Search Symbol
    let search_sym_req = tool_call_json("search_symbol", json!({
        "project": proj_name,
        "query": "add"
    }));
    let search_sym_res_str = server.transport.handle_message(&search_sym_req).unwrap();
    let search_sym_res = parse_tool_result(&search_sym_res_str);
    let results = search_sym_res.as_array().unwrap();
    assert!(!results.is_empty(), "search_symbol should return matches");
    assert_eq!(results[0]["name"].as_str().unwrap(), "add");

    // 4. Search Code
    let search_code_req = tool_call_json("search_code", json!({
        "project": proj_name,
        "query": "greet"
    }));
    let search_code_res_str = server.transport.handle_message(&search_code_req).unwrap();
    let search_code_res = parse_tool_result(&search_code_res_str);
    let code_results = search_code_res.as_array().unwrap();
    assert!(!code_results.is_empty(), "search_code should return matches");

    // 5. Get File Structure
    let rust_file_str = rust_file.to_string_lossy().to_string();
    let file_struct_req = tool_call_json("get_file_structure", json!({
        "project": proj_name,
        "file_path": rust_file_str
    }));
    let file_struct_res_str = server.transport.handle_message(&file_struct_req).unwrap();
    let file_struct_res = parse_tool_result(&file_struct_res_str);
    let structures = file_struct_res.as_array().unwrap();
    assert!(!structures.is_empty(), "get_file_structure should return symbols");

    // 6. Memory Store & Search
    let mem_store_req = tool_call_json("memory_store", json!({
        "project": proj_name,
        "content": "This project implements additions and greetings",
        "kind": "insight"
    }));
    let mem_store_res_str = server.transport.handle_message(&mem_store_req).unwrap();
    let mem_store_res = parse_tool_result(&mem_store_res_str);
    assert_eq!(mem_store_res["status"], "stored");

    let mem_search_req = tool_call_json("memory_search", json!({
        "project": proj_name,
        "query": "additions"
    }));
    let mem_search_res_str = server.transport.handle_message(&mem_search_req).unwrap();
    let mem_search_res = parse_tool_result(&mem_search_res_str);
    let mem_results = mem_search_res.as_array().unwrap();
    assert!(!mem_results.is_empty(), "memory_search should return the note");

    // 7. Get Project Overview
    let overview_req = tool_call_json("project_overview", json!({
        "project": proj_name
    }));
    let overview_res_str = server.transport.handle_message(&overview_req).unwrap();
    let overview_res = parse_tool_result(&overview_res_str);
    assert_eq!(overview_res["project"], proj_name);

    // 8. Prepare Task Context
    let task_context_req = tool_call_json("prepare_task_context", json!({
        "project": proj_name,
        "task": "Find how numbers are added"
    }));
    let task_context_res_str = server.transport.handle_message(&task_context_req).unwrap();
    let task_context_res = parse_tool_result(&task_context_res_str);
    assert!(task_context_res.get("relevant_symbols").is_some());
}
