use codebase_synapse::graph::intent::{ChangeIntent, HeuristicIntentAnalyzer, IntentAnalyzer};
use codebase_synapse::graph::risk::{PlanTarget, RiskEvaluator};

fn setup_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    codebase_synapse::db::schema::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (name, root_path) VALUES ('test_project', '/test')",
        [],
    )
    .unwrap();
    conn
}

fn insert_node(
    conn: &rusqlite::Connection,
    file_path: &str,
    kind: &str,
    name: &str,
    complexity: i64,
) -> i64 {
    conn.execute(
        "INSERT INTO nodes (project_id, file_path, kind, name, complexity, is_exported)
         VALUES (1, ?1, ?2, ?3, ?4, 1)",
        rusqlite::params![file_path, kind, name, complexity],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_edge(conn: &rusqlite::Connection, src: i64, dst: i64, kind: &str) {
    conn.execute(
        "INSERT INTO edges (project_id, source_node_id, target_node_id, kind)
         VALUES (1, ?1, ?2, ?3)",
        rusqlite::params![src, dst, kind],
    )
    .unwrap();
}

fn insert_commit(conn: &rusqlite::Connection, node_id: i64, hash: &str) {
    // Insert commit if not exists
    conn.execute(
        "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
         VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 1)",
        rusqlite::params![hash, &hash[..7.min(hash.len())]],
    ).unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
         VALUES (1, ?1, ?2, 'modified')",
        rusqlite::params![hash, node_id],
    )
    .unwrap();
}

fn insert_pagerank(conn: &rusqlite::Connection, node_id: i64, rank: f64) {
    conn.execute(
        "INSERT INTO node_pagerank (node_id, project_id, pagerank) VALUES (?1, 1, ?2)",
        rusqlite::params![node_id, rank],
    )
    .unwrap();
}

// ── Test: basic low-risk scenario ──

#[test]
fn test_risk_evaluator_basic() {
    let conn = setup_db();
    insert_node(&conn, "src/main.rs", "file", "main.rs", 10);

    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/main.rs".to_string(),
        symbol_name: None,
    };

    let report = evaluator.evaluate_plan_risk(1, &[target]).unwrap();

    assert_eq!(report.targets_analyzed.len(), 1);
    assert_eq!(report.targets_analyzed[0].file_path, "src/main.rs");
    assert!(report.overall_risk_score < 0.6);
}

// ── Test: missing_tests actually checks test_of edges ──

#[test]
fn test_missing_tests_checks_edges() {
    let conn = setup_db();

    // Create a function and a caller
    let fn_id = insert_node(&conn, "src/payment.rs", "function", "process_payment", 5);
    let caller_id = insert_node(&conn, "src/api.rs", "function", "handle_checkout", 3);
    insert_edge(&conn, caller_id, fn_id, "calls");

    // No test_of edges — should report caller as untested
    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/payment.rs".to_string(),
        symbol_name: Some("process_payment".to_string()),
    };

    let report = evaluator
        .evaluate_plan_risk(1, std::slice::from_ref(&target))
        .unwrap();
    let factors = &report.targets_analyzed[0].factors;

    // Caller should be in missing_tests
    assert!(!factors.missing_tests.is_empty());
    assert!(factors.missing_tests.iter().any(|f| f.contains("api.rs")));

    // Now add a test_of edge for the caller
    let test_id = insert_node(&conn, "tests/test_api.rs", "test", "test_checkout", 2);
    insert_edge(&conn, test_id, caller_id, "test_of");

    // Re-evaluate — caller should no longer be missing
    let report2 = evaluator
        .evaluate_plan_risk(1, std::slice::from_ref(&target))
        .unwrap();
    let factors2 = &report2.targets_analyzed[0].factors;
    assert!(factors2.missing_tests.is_empty());
}

// ── Test: dynamic normalization baselines ──

#[test]
fn test_dynamic_baselines() {
    let conn = setup_db();

    // Create nodes with varying complexity
    let low_id = insert_node(&conn, "src/low.rs", "function", "low_fn", 2);
    let high_id = insert_node(&conn, "src/high.rs", "function", "high_fn", 100);

    insert_pagerank(&conn, low_id, 0.01);
    insert_pagerank(&conn, high_id, 0.5);

    // High complexity + high pagerank should yield higher risk
    let evaluator = RiskEvaluator::new(&conn);

    let report_low = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/low.rs".to_string(),
                symbol_name: Some("low_fn".to_string()),
            }],
        )
        .unwrap();

    let report_high = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/high.rs".to_string(),
                symbol_name: Some("high_fn".to_string()),
            }],
        )
        .unwrap();

    assert!(
        report_high.overall_risk_score > report_low.overall_risk_score,
        "High complexity/pagerank node should have higher risk: {} > {}",
        report_high.overall_risk_score,
        report_low.overall_risk_score
    );
}

// ── Test: cross-target interaction ──

#[test]
fn test_cross_target_interaction() {
    let conn = setup_db();

    // A calls B calls C — modifying A and C together is riskier
    let a_id = insert_node(&conn, "src/a.rs", "function", "a", 5);
    let b_id = insert_node(&conn, "src/b.rs", "function", "b", 5);
    let c_id = insert_node(&conn, "src/c.rs", "function", "c", 5);

    insert_edge(&conn, a_id, b_id, "calls");
    insert_edge(&conn, b_id, c_id, "calls");

    let evaluator = RiskEvaluator::new(&conn);

    // Evaluate A alone
    let report_a = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/a.rs".to_string(),
                symbol_name: Some("a".to_string()),
            }],
        )
        .unwrap();

    // Evaluate A and C together (in same call chain)
    let report_both = evaluator
        .evaluate_plan_risk(
            1,
            &[
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("a".to_string()),
                },
                PlanTarget {
                    file_path: "src/c.rs".to_string(),
                    symbol_name: Some("c".to_string()),
                },
            ],
        )
        .unwrap();

    assert!(
        report_both.overall_risk_score >= report_a.overall_risk_score,
        "Combined targets in call chain should have >= risk: {} >= {}",
        report_both.overall_risk_score,
        report_a.overall_risk_score
    );
}

// ── Test: counterfactual recommendations are graph-driven ──

#[test]
fn test_counterfactual_recommendations() {
    let conn = setup_db();

    // High-centrality function with callers, no tests, high complexity
    let fn_id = insert_node(&conn, "src/core.rs", "function", "process_order", 20);
    insert_pagerank(&conn, fn_id, 0.3);

    // Add 8 callers across different files
    for i in 0..8 {
        let caller_id = insert_node(
            &conn,
            &format!("src/caller{}.rs", i),
            "function",
            &format!("caller{}", i),
            2,
        );
        insert_edge(&conn, caller_id, fn_id, "calls");
    }

    // Add high churn
    for i in 0..12 {
        insert_commit(&conn, fn_id, &format!("commit_{}", i));
    }

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/core.rs".to_string(),
                symbol_name: Some("process_order".to_string()),
            }],
        )
        .unwrap();

    assert!(report.overall_risk_score >= 0.7, "Should be CRITICAL risk");

    // Should have multiple graph-driven recommendations
    assert!(
        report.counterfactual_recommendations.len() >= 2,
        "Should generate at least 2 recommendations, got {}",
        report.counterfactual_recommendations.len()
    );

    // Check that recommendations mention real file names, not generic templates
    let rec_descriptions: String = report
        .counterfactual_recommendations
        .iter()
        .map(|r| r.description.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // Should mention actual callers or test files
    assert!(
        rec_descriptions.contains("caller") || rec_descriptions.contains("test"),
        "Recommendations should reference actual graph entities: {}",
        rec_descriptions
    );
}

// ── Test: interface segregation recommendation ──

#[test]
fn test_interface_segregation_recommendation() {
    let conn = setup_db();

    // A function that implements a trait
    let fn_id = insert_node(&conn, "src/payment.rs", "function", "process_payment", 5);
    let trait_id = insert_node(&conn, "src/payment.rs", "trait", "PaymentProcessor", 0);
    insert_edge(&conn, fn_id, trait_id, "implements");

    // Give it high blast radius
    for i in 0..10 {
        let caller_id = insert_node(
            &conn,
            &format!("src/{}.rs", i),
            "function",
            &format!("c{}", i),
            2,
        );
        insert_edge(&conn, caller_id, fn_id, "calls");
    }
    insert_pagerank(&conn, fn_id, 0.1);

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/payment.rs".to_string(),
                symbol_name: Some("process_payment".to_string()),
            }],
        )
        .unwrap();

    let strategies: Vec<&str> = report
        .counterfactual_recommendations
        .iter()
        .map(|r| r.strategy.as_str())
        .collect();

    assert!(
        strategies.contains(&"interface_segregation"),
        "Should recommend interface segregation for trait implementors with high blast radius. Got: {:?}",
        strategies
    );

    // The description should mention the actual trait name
    let desc = report
        .counterfactual_recommendations
        .iter()
        .find(|r| r.strategy == "interface_segregation")
        .map(|r| r.description.clone())
        .unwrap_or_default();

    assert!(
        desc.contains("PaymentProcessor"),
        "Should mention the actual trait name: {}",
        desc
    );
}

// ── Test: extraction candidates for high complexity ──

#[test]
fn test_extraction_candidates() {
    let conn = setup_db();

    // High complexity function with sub-calls
    let fn_id = insert_node(&conn, "src/engine.rs", "function", "run_engine", 50);
    insert_pagerank(&conn, fn_id, 0.01);

    let sub1 = insert_node(&conn, "src/engine.rs", "function", "init_phase", 3);
    let sub2 = insert_node(&conn, "src/engine.rs", "function", "exec_phase", 8);
    insert_edge(&conn, fn_id, sub1, "calls");
    insert_edge(&conn, fn_id, sub2, "calls");

    // Add churn
    for i in 0..5 {
        insert_commit(&conn, fn_id, &format!("commit_{}", i));
    }

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/engine.rs".to_string(),
                symbol_name: Some("run_engine".to_string()),
            }],
        )
        .unwrap();

    let descs: Vec<&str> = report
        .counterfactual_recommendations
        .iter()
        .map(|r| r.description.as_str())
        .collect();

    assert!(
        descs
            .iter()
            .any(|d| d.contains("init_phase") || d.contains("exec_phase")),
        "Should suggest extraction of sub-calls. Got: {:?}",
        descs
    );
}

// ── Test: target not found ──

#[test]
fn test_target_not_found() {
    let conn = setup_db();

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/nonexistent.rs".to_string(),
                symbol_name: None,
            }],
        )
        .unwrap();

    assert_eq!(report.targets_analyzed.len(), 1);
    assert!(report.targets_analyzed[0].resolved_node_id.is_none());
    assert!(report.targets_analyzed[0].risk_score <= 0.1);
}

// ── Test: risk level thresholds ──

#[test]
fn test_risk_level_thresholds() {
    let conn = setup_db();

    // Low risk: simple file, no callers
    insert_node(&conn, "src/simple.rs", "function", "simple_fn", 2);

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/simple.rs".to_string(),
                symbol_name: Some("simple_fn".to_string()),
            }],
        )
        .unwrap();

    assert_eq!(report.risk_level, "LOW");

    // High risk: central hub with churn, no tests, high complexity
    let high_id = insert_node(&conn, "src/hub.rs", "function", "hub_fn", 50);
    insert_pagerank(&conn, high_id, 0.5);
    for i in 0..20 {
        let c = insert_node(
            &conn,
            &format!("src/c{}.rs", i),
            "function",
            &format!("c{}", i),
            1,
        );
        insert_edge(&conn, c, high_id, "calls");
    }
    for i in 0..15 {
        insert_commit(&conn, high_id, &format!("commit_{}", i));
    }

    let report2 = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/hub.rs".to_string(),
                symbol_name: Some("hub_fn".to_string()),
            }],
        )
        .unwrap();

    assert_eq!(report2.risk_level, "CRITICAL");
}

// ── Test: empty targets list ──

#[test]
fn test_empty_targets() {
    let conn = setup_db();
    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator.evaluate_plan_risk(1, &[]).unwrap();

    assert_eq!(report.overall_risk_score, 0.0);
    assert_eq!(report.targets_analyzed.len(), 0);
    assert_eq!(report.counterfactual_recommendations.len(), 0);
}

// ── Test: deduplication of recommendations ──

#[test]
fn test_dedup_recommendations() {
    let conn = setup_db();

    // Two functions in the same file with same characteristics
    insert_node(&conn, "src/a.rs", "function", "fn1", 20);
    insert_node(&conn, "src/a.rs", "function", "fn2", 20);

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("fn1".to_string()),
                },
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("fn2".to_string()),
                },
            ],
        )
        .unwrap();

    // All recommendations should be unique
    let mut seen = std::collections::HashSet::new();
    for rec in &report.counterfactual_recommendations {
        let key = format!("{}:{}", rec.strategy, rec.description);
        assert!(seen.insert(key), "Duplicate recommendation found");
    }
}

// ── Test: confidence score calculation ──

#[test]
fn test_confidence_score_calculation() {
    let conn = setup_db();
    // Insert a file node
    let file_id = insert_node(&conn, "src/main.rs", "file", "main.rs", 10);

    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/main.rs".to_string(),
        symbol_name: None,
    };

    let report = evaluator
        .evaluate_plan_risk(1, std::slice::from_ref(&target))
        .unwrap();
    // Since there are no git commits and no test files indexed, confidence should be 1.0 - 0.3 - 0.2 = 0.5
    assert_eq!(report.confidence_score, 0.5);

    // Let's add a commit and check confidence again
    insert_commit(&conn, file_id, "hash123");
    let report2 = evaluator
        .evaluate_plan_risk(1, std::slice::from_ref(&target))
        .unwrap();
    // With 1 commit, confidence should be 1.0 - 0.2 = 0.8
    assert_eq!(report2.confidence_score, 0.8);

    // Let's add a test node (kind = 'test')
    insert_node(&conn, "tests/unit_test.rs", "test", "my_test", 1);
    let report3 = evaluator
        .evaluate_plan_risk(1, std::slice::from_ref(&target))
        .unwrap();
    // With commits and test files, confidence should be 1.0
    assert_eq!(report3.confidence_score, 1.0);
}

// ── Test: recommendation delta prediction and sorting ──

#[test]
fn test_recommendation_sorting_and_priority() {
    let conn = setup_db();

    // Create a node with high complexity, high churn, and callers (to generate multiple suggestions)
    let fn_id = insert_node(&conn, "src/core.rs", "function", "process_payments", 40);
    insert_pagerank(&conn, fn_id, 0.4);

    // Add 10 callers -> high blast radius + missing tests
    for i in 0..10 {
        let caller_id = insert_node(
            &conn,
            &format!("src/c{}.rs", i),
            "function",
            &format!("c{}", i),
            2,
        );
        insert_edge(&conn, caller_id, fn_id, "calls");
    }

    // Churn of 12 commits
    for i in 0..12 {
        insert_commit(&conn, fn_id, &format!("hash_{}", i));
    }

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/core.rs".to_string(),
                symbol_name: Some("process_payments".to_string()),
            }],
        )
        .unwrap();

    let recs = &report.counterfactual_recommendations;
    assert!(recs.len() >= 2);

    // Verify sorting is descending by estimated_risk_delta
    for idx in 0..recs.len() - 1 {
        assert!(
            recs[idx].estimated_risk_delta >= recs[idx + 1].estimated_risk_delta,
            "Recommendations not sorted descending by delta: {} at {} vs {} at {}",
            recs[idx].estimated_risk_delta,
            idx,
            recs[idx + 1].estimated_risk_delta,
            idx + 1
        );
    }

    // Verify priority mapping is correct based on deltas
    for rec in recs {
        let expected_priority = if rec.estimated_risk_delta >= 0.10 {
            "HIGH"
        } else if rec.estimated_risk_delta >= 0.05 {
            "MEDIUM"
        } else {
            "LOW"
        };
        assert_eq!(rec.priority, expected_priority);
    }
}

// ── Test: safe change order ──

#[test]
fn test_safe_change_order_empty_targets() {
    let conn = setup_db();
    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator.evaluate_plan_risk(1, &[]).unwrap();
    assert!(report.safe_change_order.is_empty());
}

#[test]
fn test_safe_change_order_single_target() {
    let conn = setup_db();
    let fn_id = insert_node(&conn, "src/a.rs", "function", "foo", 5);
    insert_pagerank(&conn, fn_id, 0.1);

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/a.rs".to_string(),
                symbol_name: Some("foo".to_string()),
            }],
        )
        .unwrap();

    assert_eq!(report.safe_change_order.len(), 1);
    assert_eq!(report.safe_change_order[0].order, 1);
    assert_eq!(
        report.safe_change_order[0].symbol_name,
        Some("foo".to_string())
    );
    assert_eq!(report.safe_change_order[0].rationale, "only target");
}

#[test]
fn test_safe_change_order_independent_targets() {
    let conn = setup_db();
    // Two independent targets — no call edges between them
    let a_id = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, a_id, 0.1);
    let b_id = insert_node(&conn, "src/b.rs", "function", "beta", 8);
    insert_pagerank(&conn, b_id, 0.3);

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("alpha".to_string()),
                },
                PlanTarget {
                    file_path: "src/b.rs".to_string(),
                    symbol_name: Some("beta".to_string()),
                },
            ],
        )
        .unwrap();

    assert_eq!(report.safe_change_order.len(), 2);
    // Both should have order 1,2 — independent, but lower risk first
    let names: Vec<Option<&str>> = report
        .safe_change_order
        .iter()
        .map(|e| e.symbol_name.as_deref())
        .collect();
    // alpha has lower complexity → lower risk → should come first
    assert_eq!(names, vec![Some("alpha"), Some("beta")]);
}

#[test]
fn test_safe_change_order_dependency_chain() {
    let conn = setup_db();
    // a → b means a calls b, so b must be modified first
    let a_id = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, a_id, 0.1);
    let b_id = insert_node(&conn, "src/b.rs", "function", "beta", 2);
    insert_pagerank(&conn, b_id, 0.1);
    insert_edge(&conn, a_id, b_id, "calls"); // alpha calls beta

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("alpha".to_string()),
                },
                PlanTarget {
                    file_path: "src/b.rs".to_string(),
                    symbol_name: Some("beta".to_string()),
                },
            ],
        )
        .unwrap();

    assert_eq!(report.safe_change_order.len(), 2);
    // beta should come first (alpha depends on it)
    assert_eq!(
        report.safe_change_order[0].symbol_name,
        Some("beta".to_string())
    );
    assert_eq!(
        report.safe_change_order[1].symbol_name,
        Some("alpha".to_string())
    );
    // beta should have no deps rationale
    assert!(
        report.safe_change_order[0]
            .rationale
            .contains("no dependencies")
            || report.safe_change_order[0].rationale.contains("satisfied")
    );
}

#[test]
fn test_safe_change_order_three_node_chain() {
    let conn = setup_db();
    // a → b → c means c must come first, then b, then a
    let a_id = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, a_id, 0.1);
    let b_id = insert_node(&conn, "src/b.rs", "function", "beta", 2);
    insert_pagerank(&conn, b_id, 0.1);
    let c_id = insert_node(&conn, "src/c.rs", "function", "gamma", 2);
    insert_pagerank(&conn, c_id, 0.1);
    insert_edge(&conn, a_id, b_id, "calls"); // alpha calls beta
    insert_edge(&conn, b_id, c_id, "calls"); // beta calls gamma

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("alpha".to_string()),
                },
                PlanTarget {
                    file_path: "src/b.rs".to_string(),
                    symbol_name: Some("beta".to_string()),
                },
                PlanTarget {
                    file_path: "src/c.rs".to_string(),
                    symbol_name: Some("gamma".to_string()),
                },
            ],
        )
        .unwrap();

    assert_eq!(report.safe_change_order.len(), 3);
    let names: Vec<Option<&str>> = report
        .safe_change_order
        .iter()
        .map(|e| e.symbol_name.as_deref())
        .collect();
    // gamma first, beta second, alpha last
    assert_eq!(names, vec![Some("gamma"), Some("beta"), Some("alpha")]);
}

#[test]
fn test_safe_change_order_cycle_detection() {
    let conn = setup_db();
    // a → b → a (cycle)
    let a_id = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, a_id, 0.1);
    let b_id = insert_node(&conn, "src/b.rs", "function", "beta", 2);
    insert_pagerank(&conn, b_id, 0.1);
    insert_edge(&conn, a_id, b_id, "calls");
    insert_edge(&conn, b_id, a_id, "calls");

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[
                PlanTarget {
                    file_path: "src/a.rs".to_string(),
                    symbol_name: Some("alpha".to_string()),
                },
                PlanTarget {
                    file_path: "src/b.rs".to_string(),
                    symbol_name: Some("beta".to_string()),
                },
            ],
        )
        .unwrap();

    // Cycle should still return all targets
    assert_eq!(report.safe_change_order.len(), 2);
    // At least one should have cycle rationale
    let has_cycle_rationale = report
        .safe_change_order
        .iter()
        .any(|e| e.rationale.contains("cycle"));
    assert!(has_cycle_rationale);
}

// ── Test: change coupling detection ──

use codebase_synapse::graph::change_coupling;

#[test]
fn test_change_coupling_detect_and_store() {
    let conn = setup_db();

    // Create two file nodes
    let a_id = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let b_id = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // Create 4 commits that touch both files (co-change)
    for i in 0..4 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, a_id],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, b_id],
        ).unwrap();
    }

    let couplings = change_coupling::detect_and_store(&conn, 1, 3).unwrap();
    assert_eq!(couplings.len(), 1);
    assert_eq!(couplings[0].co_change_count, 4);
    assert_eq!(couplings[0].source_node_id, a_id);
    assert_eq!(couplings[0].target_node_id, b_id);
}

#[test]
fn test_change_coupling_below_threshold() {
    let conn = setup_db();

    let a_id = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let b_id = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // Only 2 co-changes — below threshold of 3
    for i in 0..2 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, a_id],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, b_id],
        ).unwrap();
    }

    let couplings = change_coupling::detect_and_store(&conn, 1, 3).unwrap();
    assert!(couplings.is_empty());
}

#[test]
fn test_change_coupling_get_couplings_for_node() {
    let conn = setup_db();

    let a_id = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let b_id = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // Create 5 co-changes
    for i in 0..5 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, a_id],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, b_id],
        ).unwrap();
    }

    change_coupling::detect_and_store(&conn, 1, 3).unwrap();

    let couplings = change_coupling::get_couplings_for_node(&conn, a_id).unwrap();
    assert_eq!(couplings.len(), 1);
    assert_eq!(couplings[0].coupled_node_id, b_id);
    assert_eq!(couplings[0].co_change_count, 5);
}

#[test]
fn test_change_coupling_increases_risk_score() {
    let conn = setup_db();

    let fn_a = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, fn_a, 0.1);
    let fn_b = insert_node(&conn, "src/b.rs", "function", "beta", 2);
    insert_pagerank(&conn, fn_b, 0.1);

    // Create file nodes for coupling
    let file_a = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let file_b = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // Create 5 co-changes between file nodes
    for i in 0..5 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_a],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_b],
        ).unwrap();
    }

    // Detect and store coupling
    change_coupling::detect_and_store(&conn, 1, 3).unwrap();

    let evaluator = RiskEvaluator::new(&conn);

    // Risk without coupling (before detection — but edges exist now, so this includes coupling)
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/a.rs".to_string(),
                symbol_name: Some("alpha".to_string()),
            }],
        )
        .unwrap();

    // Should have change_coupling_count > 0
    assert!(report.targets_analyzed[0].factors.change_coupling_count > 0);
}

#[test]
fn test_change_coupling_recommends_review() {
    let conn = setup_db();

    let fn_a = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, fn_a, 0.1);
    let file_a = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let file_b = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // Create 5 co-changes
    for i in 0..5 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_a],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_b],
        ).unwrap();
    }

    change_coupling::detect_and_store(&conn, 1, 3).unwrap();

    let evaluator = RiskEvaluator::new(&conn);
    let report = evaluator
        .evaluate_plan_risk(
            1,
            &[PlanTarget {
                file_path: "src/a.rs".to_string(),
                symbol_name: Some("alpha".to_string()),
            }],
        )
        .unwrap();

    // Should have a change_coupling recommendation
    let has_coupling_rec = report
        .counterfactual_recommendations
        .iter()
        .any(|r| r.strategy == "account_change_coupling");
    assert!(
        has_coupling_rec,
        "Expected account_change_coupling recommendation"
    );
}

// ── Test: intent-aware precision layer (RIPPLE plan-then-predict) ──

#[test]
fn test_intent_unit_documentation() {
    let a = HeuristicIntentAnalyzer.analyze("fix typo in documentation comment");
    assert_eq!(a.intent, ChangeIntent::Documentation);
    assert!(a.prune_strategies.contains("account_change_coupling"));
    assert!(a.prune_strategies.contains("add_test_contract"));
    assert!(a.coupling_risk_multiplier < 0.3);
}

#[test]
fn test_intent_unit_security_keeps_impact() {
    let a = HeuristicIntentAnalyzer.analyze("fix SQL injection vulnerability in auth handler");
    assert_eq!(a.intent, ChangeIntent::Security);
    assert_eq!(a.coupling_risk_multiplier, 1.0);
    assert!(a.prune_strategies.is_empty());
}

#[test]
fn test_intent_aware_prunes_recommendations() {
    let conn = setup_db();

    let fn_a = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, fn_a, 0.1);
    let file_a = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let file_b = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // Create 5 co-changes so coupling is detected
    for i in 0..5 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_a],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_b],
        ).unwrap();
    }
    change_coupling::detect_and_store(&conn, 1, 3).unwrap();

    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/a.rs".to_string(),
        symbol_name: Some("alpha".to_string()),
    };

    // Without intent: coupling recommendation present
    let report_no_intent = evaluator
        .evaluate_plan_risk_with_intent(1, std::slice::from_ref(&target), &[None])
        .unwrap();
    assert!(report_no_intent
        .counterfactual_recommendations
        .iter()
        .any(|r| r.strategy == "account_change_coupling"));
    assert!(report_no_intent.pruned_recommendations.is_empty());

    // With documentation intent: coupling recommendation pruned
    let report_doc = evaluator
        .evaluate_plan_risk_with_intent(
            1,
            std::slice::from_ref(&target),
            &[Some("fix typo in documentation comment".to_string())],
        )
        .unwrap();
    assert!(
        !report_doc
            .counterfactual_recommendations
            .iter()
            .any(|r| r.strategy == "account_change_coupling"),
        "coupling rec should be pruned for doc intent"
    );
    assert!(
        report_doc
            .pruned_recommendations
            .iter()
            .any(|r| r.strategy == "account_change_coupling"),
        "pruned rec should be recorded for audit"
    );
    assert_eq!(
        report_doc.targets_analyzed[0]
            .detected_intent
            .as_ref()
            .unwrap()
            .category,
        "documentation"
    );
}

#[test]
fn test_intent_reduces_coupling_risk_score() {
    let conn = setup_db();

    let fn_a = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, fn_a, 0.1);
    let file_a = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let file_b = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    // 5 co-changes
    for i in 0..5 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_a],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_b],
        ).unwrap();
    }
    change_coupling::detect_and_store(&conn, 1, 3).unwrap();

    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/a.rs".to_string(),
        symbol_name: Some("alpha".to_string()),
    };

    let no_intent = evaluator
        .evaluate_plan_risk_with_intent(1, std::slice::from_ref(&target), &[None])
        .unwrap();

    let doc_intent = evaluator
        .evaluate_plan_risk_with_intent(
            1,
            std::slice::from_ref(&target),
            &[Some("fix typo in docs".to_string())],
        )
        .unwrap();

    let score_no = no_intent.targets_analyzed[0].risk_score;
    let score_doc = doc_intent.targets_analyzed[0].risk_score;

    // Documentation intent shrinks the coupling contribution -> lower risk score
    assert!(
        score_doc < score_no,
        "doc intent should reduce risk: {} vs {}",
        score_doc,
        score_no
    );
    // And the scaled coupling factor should be smaller
    let coupling_no = no_intent.targets_analyzed[0].factors.change_coupling_count;
    let coupling_doc = doc_intent.targets_analyzed[0].factors.change_coupling_count;
    assert!(
        coupling_doc < coupling_no,
        "doc intent should scale coupling count: {} vs {}",
        coupling_doc,
        coupling_no
    );
}

#[test]
fn test_intent_security_keeps_full_impact() {
    let conn = setup_db();

    let fn_a = insert_node(&conn, "src/a.rs", "function", "alpha", 2);
    insert_pagerank(&conn, fn_a, 0.1);
    let file_a = insert_node(&conn, "src/a.rs", "file", "a.rs", 1);
    let file_b = insert_node(&conn, "src/b.rs", "file", "b.rs", 1);

    for i in 0..5 {
        let hash = format!("commit_{}", i);
        conn.execute(
            "INSERT OR IGNORE INTO git_commits (project_id, hash, short_hash, message, author, timestamp, intent_kind, files_changed)
             VALUES (1, ?1, ?2, 'test', 'test', '2025-01-01', 'other', 2)",
            rusqlite::params![hash, &hash[..7.min(hash.len())]],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_a],
        ).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO commit_node_links (project_id, commit_hash, node_id, change_type)
             VALUES (1, ?1, ?2, 'modified')",
            rusqlite::params![hash, file_b],
        ).unwrap();
    }
    change_coupling::detect_and_store(&conn, 1, 3).unwrap();

    let evaluator = RiskEvaluator::new(&conn);
    let target = PlanTarget {
        file_path: "src/a.rs".to_string(),
        symbol_name: Some("alpha".to_string()),
    };
    let report = evaluator
        .evaluate_plan_risk_with_intent(
            1,
            &[target],
            &[Some("fix SQL injection vulnerability".to_string())],
        )
        .unwrap();

    // Security intent keeps the coupling recommendation and full impact set
    assert!(report
        .counterfactual_recommendations
        .iter()
        .any(|r| r.strategy == "account_change_coupling"));
    assert!(report.pruned_recommendations.is_empty());
    assert_eq!(
        report.targets_analyzed[0]
            .detected_intent
            .as_ref()
            .unwrap()
            .category,
        "security"
    );
}
