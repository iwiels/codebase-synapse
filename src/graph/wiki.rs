//! Auto-generated architecture wiki. Pure function: same input → same Markdown.

use std::collections::HashMap;
use anyhow::Result;
use rusqlite::Connection;

pub struct WikiConfig {
    pub project_name: String,
    pub max_files_per_cluster: usize,
}

impl Default for WikiConfig {
    fn default() -> Self {
        Self { project_name: "this codebase".into(), max_files_per_cluster: 20 }
    }
}

/// Render a Markdown architecture wiki from cluster data.
pub fn render_wiki(
    config: &WikiConfig,
    clusters: &HashMap<i64, Vec<String>>,
    violations: &[crate::graph::boundaries::Violation],
    modularity: f64,
) -> String {
    let mut md = format!("# Architecture Wiki — {}\n\n", config.project_name);
    md.push_str(&format!("> Modularity score: **{:.4}**\n\n", modularity));

    if clusters.is_empty() {
        md.push_str("_No clusters. Run `index_repository` first._\n");
        return md;
    }

    let mut sorted_ids: Vec<i64> = clusters.keys().cloned().filter(|&c| c != 0).collect();
    sorted_ids.sort_unstable();

    md.push_str("## Summary\n\n| Metric | Value |\n|---|---|\n");
    md.push_str(&format!("| Clusters | {} |\n", sorted_ids.len()));
    md.push_str(&format!("| Modularity | {:.4} |\n", modularity));
    if !violations.is_empty() {
        md.push_str(&format!("| ⚠️ Violations | {} |\n", violations.len()));
    }
    md.push_str("\n---\n\n");

    for cid in &sorted_ids {
        let files = &clusters[cid];
        md.push_str(&format!("## Cluster {}\n\n**{} files**\n\n", cid, files.len()));
        let mut sorted_files = files.clone();
        sorted_files.sort();
        for fp in sorted_files.iter().take(config.max_files_per_cluster) {
            let has_v = violations.iter().any(|v| &v.from_file == fp || &v.to_file == fp);
            if has_v { md.push_str(&format!("- `{}` ⚠️\n", fp)); }
            else { md.push_str(&format!("- `{}`\n", fp)); }
        }
        if files.len() > config.max_files_per_cluster {
            md.push_str(&format!("_...and {} more_\n", files.len() - config.max_files_per_cluster));
        }
        md.push('\n');
    }

    if let Some(misc) = clusters.get(&0) {
        if !misc.is_empty() {
            md.push_str("## Unclustered Files\n\n");
            let mut s = misc.clone(); s.sort();
            for fp in s.iter().take(config.max_files_per_cluster) {
                md.push_str(&format!("- `{}`\n", fp));
            }
        }
    }

    if !violations.is_empty() {
        md.push_str("\n---\n\n## ⚠️ Boundary Violations\n\n| From | To | Rule |\n|---|---|---|\n");
        for v in violations {
            md.push_str(&format!("| `{}` | `{}` | deny `{}` (rule {}) |\n",
                v.from_file, v.to_file, v.deny_pattern, v.rule_index));
        }
    }
    md
}

/// Compute modularity (Q) from cluster assignments and edges.
fn compute_modularity(
    assignments: &HashMap<i64, i64>,
    edges: &[(i64, i64)],
) -> f64 {
    use std::collections::BTreeMap;
    // Build symmetric weighted adjacency
    let mut adj: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
    for &n in assignments.keys() { adj.entry(n).or_default(); }
    for &(s, d) in edges {
        if s == d { continue; }
        *adj.entry(s).or_default().entry(d).or_insert(0.0) += 1.0;
        *adj.entry(d).or_default().entry(s).or_insert(0.0) += 1.0;
    }
    let degrees: HashMap<i64, f64> = assignments.keys()
        .map(|&n| (n, adj.get(&n).map(|a| a.values().sum::<f64>()).unwrap_or(0.0)))
        .collect();
    let m2: f64 = degrees.values().sum();
    if m2 == 0.0 { return 0.0; }
    let mut q = 0.0f64;
    for (&node, nbrs) in &adj {
        let ci = assignments[&node];
        for (&nbr, &w) in nbrs {
            if ci == assignments[&nbr] {
                q += w - degrees[&node] * degrees[&nbr] / m2;
            }
        }
    }
    q / m2
}

/// Generate wiki from DB data for a project.
pub fn generate_wiki(conn: &Connection, project_id: i64, config: &WikiConfig) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT file_path, cluster_id FROM file_clusters WHERE project_id = ?1"
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(rusqlite::params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok()).collect();
    let mut clusters: HashMap<i64, Vec<String>> = HashMap::new();
    let mut file_to_cluster: HashMap<String, i64> = HashMap::new();
    for (fp, cid) in &rows {
        clusters.entry(*cid).or_default().push(fp.clone());
        file_to_cluster.insert(fp.clone(), *cid);
    }
    // Get node IDs mapped to file paths for modularity computation
    let node_ids: Vec<(i64, String)> = {
        let mut stmt2 = conn.prepare(
            "SELECT id, file_path FROM nodes WHERE project_id = ?1"
        )?;
        let rows: Vec<(i64, String)> = stmt2.query_map(rusqlite::params![project_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?.filter_map(|r| r.ok()).collect();
        rows
    };
    let node_to_cluster: HashMap<i64, i64> = node_ids.iter()
        .filter_map(|(id, fp)| file_to_cluster.get(fp).map(|&c| (*id, c)))
        .collect();
    let edges = crate::db::queries::get_all_import_edges(conn, project_id)?;
    let modularity = compute_modularity(&node_to_cluster, &edges);
    Ok(render_wiki(config, &clusters, &[], modularity))
}
