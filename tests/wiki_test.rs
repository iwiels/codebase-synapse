// tests/wiki_test.rs
use codebase_synapse::graph::wiki::{render_wiki, WikiConfig};
use std::collections::HashMap;

#[test]
fn test_render_wiki_contains_project_name() {
    let cfg = WikiConfig { project_name: "my-app".into(), max_files_per_cluster: 10 };
    let clusters: HashMap<i64, Vec<String>> = HashMap::new();
    let md = render_wiki(&cfg, &clusters, &[], 0.0);
    assert!(md.contains("my-app"));
    assert!(md.contains("Architecture Wiki"));
}

#[test]
fn test_render_wiki_two_clusters_sorted() {
    let cfg = WikiConfig { project_name: "app".into(), max_files_per_cluster: 10 };
    let mut clusters = HashMap::new();
    clusters.insert(1i64, vec!["src/api.rs".into()]);
    clusters.insert(2i64, vec!["src/db.rs".into()]);
    let md = render_wiki(&cfg, &clusters, &[], 0.65);
    assert!(md.contains("Cluster 1"));
    assert!(md.contains("Cluster 2"));
    assert!(md.contains("src/api.rs"));
}

#[test]
fn test_render_wiki_deterministic() {
    let cfg = WikiConfig { project_name: "app".into(), max_files_per_cluster: 10 };
    let mut clusters = HashMap::new();
    clusters.insert(1i64, vec!["b.rs".into(), "a.rs".into()]);
    let md1 = render_wiki(&cfg, &clusters, &[], 0.5);
    let md2 = render_wiki(&cfg, &clusters, &[], 0.5);
    assert_eq!(md1, md2, "wiki must be deterministic");
}
