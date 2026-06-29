// tests/leiden_test.rs
use codebase_memory::graph::leiden::{leiden_raw, LeidenConfig};

#[test]
fn test_leiden_single_node() {
    let nodes = vec![1i64];
    let edges: Vec<(i64, i64)> = vec![];
    let cfg = LeidenConfig::default();
    let (assignments, modularity) = leiden_raw(&nodes, &edges, &cfg);
    assert!(assignments.contains_key(&1));
    assert!((modularity).abs() < 1e-9);
}

#[test]
fn test_leiden_two_disconnected_cliques() {
    // Clique A: 1,2,3 fully connected. Clique B: 4,5,6 fully connected.
    let nodes = vec![1i64, 2, 3, 4, 5, 6];
    let edges = vec![
        (1,2),(2,1),(1,3),(3,1),(2,3),(3,2),
        (4,5),(5,4),(4,6),(6,4),(5,6),(6,5),
    ];
    let cfg = LeidenConfig::default();
    let (assignments, _) = leiden_raw(&nodes, &edges, &cfg);
    let ca = assignments[&1];
    let cb = assignments[&4];
    assert_ne!(ca, cb, "disconnected cliques must get different clusters");
    assert_eq!(assignments[&2], ca);
    assert_eq!(assignments[&3], ca);
    assert_eq!(assignments[&5], cb);
    assert_eq!(assignments[&6], cb);
}

#[test]
fn test_leiden_deterministic() {
    let nodes = vec![1i64, 2, 3, 4, 5];
    let edges = vec![(1,2),(2,3),(3,4),(4,5),(5,1),(1,3)];
    let cfg = LeidenConfig::default();
    let (a1, m1) = leiden_raw(&nodes, &edges, &cfg);
    let (a2, m2) = leiden_raw(&nodes, &edges, &cfg);
    assert_eq!(a1, a2, "Leiden must be deterministic");
    assert!((m1 - m2).abs() < 1e-12);
}
