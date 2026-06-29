// tests/boundaries_test.rs
use codebase_synapse::graph::boundaries::{check_boundaries, BoundaryConfig, BoundaryRule};

fn make_config(from: &str, deny: &[&str]) -> BoundaryConfig {
    BoundaryConfig {
        boundary: vec![BoundaryRule {
            from: from.to_string(),
            deny: deny.iter().map(|s| s.to_string()).collect(),
            allow: vec![],
        }],
    }
}

#[test]
fn test_no_violation_when_no_matching_edges() {
    let config = make_config("src/api/**", &["src/db/**"]);
    let files = vec![
        ("src/api/users.rs".to_string(), 1i64),
        ("src/services/auth.rs".to_string(), 2i64),
    ];
    // Edge from api to services — NOT in deny list
    let edges = vec![(1i64, 2i64)];
    let violations = check_boundaries(&config, &files, &edges).unwrap();
    assert!(violations.is_empty(), "no violation expected");
}

#[test]
fn test_violation_detected() {
    let config = make_config("src/api/**", &["src/db/**"]);
    let files = vec![
        ("src/api/users.rs".to_string(), 1i64),
        ("src/db/user_repo.rs".to_string(), 2i64),
    ];
    let edges = vec![(1i64, 2i64)]; // api → db: violation!
    let violations = check_boundaries(&config, &files, &edges).unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].from_file, "src/api/users.rs");
    assert_eq!(violations[0].to_file, "src/db/user_repo.rs");
    assert_eq!(violations[0].rule_index, 0);
}

#[test]
fn test_allow_overrides_deny() {
    let config = BoundaryConfig {
        boundary: vec![BoundaryRule {
            from: "src/api/**".to_string(),
            deny: vec!["src/db/**".to_string()],
            allow: vec!["src/db/shared.rs".to_string()],
        }],
    };
    let files = vec![
        ("src/api/users.rs".to_string(), 1i64),
        ("src/db/shared.rs".to_string(), 2i64),
    ];
    let edges = vec![(1, 2)];
    let violations = check_boundaries(&config, &files, &edges).unwrap();
    assert!(
        violations.is_empty(),
        "allow should override deny for shared.rs"
    );
}
