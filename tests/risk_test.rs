use codebase_synapse::graph::risk::{PlanTarget, RiskEvaluator};
use tempfile::tempdir;

#[test]
fn test_risk_evaluator_basic() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    
    // Setup minimal tables for testing
    codebase_synapse::db::schema::migrate(&conn).unwrap();
    
    // Insert test project
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES ('test_project', '/test')",
        [],
    ).unwrap();
    
    // Insert test file node
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind, name, start_line, end_line, complexity, is_exported)
         VALUES (1, 'src/main.rs', 'file', 'main.rs', 0, 100, 10, 1)",
         [],
    ).unwrap();

    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/main.rs".to_string(),
        symbol_name: None,
    };
    
    let report = evaluator.evaluate_plan_risk(1, &[target]).unwrap();
    
    assert_eq!(report.targets_analyzed.len(), 1);
    assert_eq!(report.targets_analyzed[0].file_path, "src/main.rs");
    // Baseline risk score should be relatively low since there are no callers or high complexity, but since it misses tests, there will be a small missing tests score.
    assert!(report.overall_risk_score < 0.6);
}
