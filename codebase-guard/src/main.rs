//! codebase-guard — PreToolUse hook para Claude Code.
//!
//! Lee un JSON event de stdin con { tool_name, tool_input },
//! consulta el DB de codebase-synapse y devuelve
//! {"behavior":"block","message":"..."} o {"behavior":"allow"}.

use std::io::{self, BufRead};
use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_PR_THRESHOLD: f64 = 0.05;
const DEFAULT_BLAST_THRESHOLD: i64 = 10;
const ACK_TTL_SECS: u64 = 600;

#[derive(Debug, Deserialize)]
struct HookEvent {
    tool_name: Option<String>,
    tool_input: Option<Value>,
}

#[derive(Debug, Serialize)]
struct HookResponse {
    behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn allow() -> String {
    serde_json::to_string(&HookResponse { behavior: "allow".into(), message: None }).unwrap()
}

fn block(msg: String) -> String {
    serde_json::to_string(&HookResponse { behavior: "block".into(), message: Some(msg) }).unwrap()
}

fn get_db_path() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--db" { return args.next().map(PathBuf::from); }
    }
    let d = PathBuf::from(".codebase-synapse/index.db");
    if d.exists() { Some(d) } else { None }
}

fn extract_file(tool_name: &str, inp: &Value) -> Option<String> {
    match tool_name {
        "Write" | "Edit" | "MultiEdit" | "write_to_file" | "replace_file_content" | "multi_replace_file_content" => {
            inp.get("file_path").or_else(|| inp.get("TargetFile"))
                .and_then(|v| v.as_str()).map(|s| s.to_string())
        }
        _ => None,
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.as_bytes() { h = h.wrapping_mul(1099511628211); h ^= *b as u64; }
    h
}

fn check_ack(db_dir: &std::path::Path, file_path: &str) -> bool {
    use std::time::SystemTime;
    let ack = db_dir.join("acks").join(format!("{:x}", fnv1a(file_path)));
    if let Ok(meta) = std::fs::metadata(&ack) {
        if let Ok(modified) = meta.modified() {
            if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                return elapsed.as_secs() < ACK_TTL_SECS;
            }
        }
    }
    false
}

fn run() -> Result<()> {
    let db_path = match get_db_path() {
        Some(p) => p,
        None => { println!("{}", allow()); return Ok(()); }
    };

    let mut input = String::new();
    for line in io::stdin().lock().lines() { input.push_str(&line?); input.push('\n'); }

    let event: HookEvent = match serde_json::from_str(&input) {
        Ok(e) => e,
        Err(_) => { println!("{}", allow()); return Ok(()); }
    };

    let tool_name = match event.tool_name.as_deref() {
        Some(n) => n.to_string(),
        None => { println!("{}", allow()); return Ok(()); }
    };

    let file_path = match event.tool_input.as_ref()
        .and_then(|inp| extract_file(&tool_name, inp))
    {
        Some(fp) => fp,
        None => { println!("{}", allow()); return Ok(()); }
    };

    let db_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
    if check_ack(db_dir, &file_path) { println!("{}", allow()); return Ok(()); }

    let conn = rusqlite::Connection::open(&db_path)?;
    let pr_threshold = std::env::var("GUARD_PAGERANK_MIN")
        .ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(DEFAULT_PR_THRESHOLD);
    let blast_threshold = std::env::var("GUARD_BLAST_MIN")
        .ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(DEFAULT_BLAST_THRESHOLD);

    let result: rusqlite::Result<(f64, i64)> = conn.query_row(
        "SELECT COALESCE(np.pagerank, 0.0),
                (SELECT COUNT(DISTINCT e.source_node_id) FROM edges e WHERE e.target_node_id = n.id)
         FROM nodes n LEFT JOIN node_pagerank np ON np.node_id = n.id
         WHERE n.file_path LIKE ?1 AND n.kind = 'file' LIMIT 1",
        rusqlite::params![format!("%{}", file_path)],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match result {
        Ok((pagerank, blast_radius)) if pagerank >= pr_threshold || blast_radius >= blast_threshold => {
            println!("{}", block(format!(
                "⚠️ HIGH-IMPACT FILE: `{}` (PageRank: {:.4}, Blast Radius: {} files)\n\n\
                 Before editing this architectural hub:\n\
                 1. Call `impact_analysis` with this node's ID\n\
                 2. Review the affected files listed\n\
                 3. This block lifts automatically after impact_analysis runs\n\n\
                 Override: set GUARD_PAGERANK_MIN=1.0 in your environment.",
                file_path, pagerank, blast_radius
            )));
        }
        _ => println!("{}", allow()),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("codebase-guard error: {}", e);
        println!("{}", serde_json::json!({"behavior":"allow"}).to_string());
    }
}
