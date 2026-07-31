//! Deterministic community detection: Louvain + Leiden refinement.
//!
//! Determinism: all iteration in sorted node order, ties break on
//! smallest community id. Same input → identical output.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub struct LeidenConfig {
    pub resolution: f64,
    pub min_cluster_size: usize,
    pub max_iterations: u32,
    pub epsilon: f64,
}

impl Default for LeidenConfig {
    fn default() -> Self {
        Self {
            resolution: 1.0,
            min_cluster_size: 2,
            max_iterations: 50,
            epsilon: 1e-6,
        }
    }
}

pub struct ClusterReport {
    pub assignments: HashMap<i64, i64>,
    pub modularity: f64,
    pub cluster_count: usize,
}

/// Pure Leiden/Louvain. Returns (node → cluster_id, modularity).
pub fn leiden_raw(
    nodes: &[i64],
    edges: &[(i64, i64)],
    config: &LeidenConfig,
) -> (HashMap<i64, i64>, f64) {
    if nodes.is_empty() {
        return (HashMap::new(), 0.0);
    }

    // Build symmetric weighted adjacency
    let mut adj: BTreeMap<i64, BTreeMap<i64, f64>> = BTreeMap::new();
    for &n in nodes {
        adj.entry(n).or_default();
    }
    for &(s, d) in edges {
        if s == d {
            continue;
        }
        *adj.entry(s).or_default().entry(d).or_insert(0.0) += 1.0;
        *adj.entry(d).or_default().entry(s).or_insert(0.0) += 1.0;
    }

    let degrees: HashMap<i64, f64> = nodes
        .iter()
        .map(|&n| (n, adj[&n].values().sum::<f64>()))
        .collect();

    let m2: f64 = degrees.values().sum();

    let mut sorted_nodes: Vec<i64> = nodes.to_vec();
    sorted_nodes.sort_unstable();

    // Init: each node is its own community
    let mut community: HashMap<i64, i64> = sorted_nodes
        .iter()
        .enumerate()
        .map(|(i, &n)| (n, i as i64 + 1))
        .collect();

    if m2 == 0.0 {
        return (community, 0.0);
    }

    // Community total degree, maintained incrementally so a node move is
    // O(degree) instead of O(N). This turns the whole local-move phase
    // from O(iterations * N^2 * communities) into O(iterations * edges).
    let mut comm_degree: HashMap<i64, f64> = HashMap::new();
    for &n in &sorted_nodes {
        *comm_degree.entry(community[&n]).or_insert(0.0) += degrees[&n];
    }

    // Louvain local move phase
    for _ in 0..config.max_iterations {
        let mut moved = false;
        for &node in &sorted_nodes {
            let ki = degrees[&node];
            let current_c = community[&node];
            let mut neighbor_comms: BTreeMap<i64, f64> = BTreeMap::new();
            if let Some(nbrs) = adj.get(&node) {
                for (&nbr, &w) in nbrs {
                    *neighbor_comms.entry(community[&nbr]).or_insert(0.0) += w;
                }
            }

            let ki_in_old = neighbor_comms.get(&current_c).copied().unwrap_or(0.0);
            let k_old = comm_degree[&current_c];

            let mut best_c = current_c;
            let mut best_delta = 0.0f64;

            for (&cand_c, &ki_in_new) in &neighbor_comms {
                if cand_c == current_c {
                    continue;
                }
                let k_cand = comm_degree[&cand_c];

                // ΔQ = (ki_in_new - ki_in_old) / M - resolution * ki * (k_cand - k_old + ki) / M^2
                let delta = (ki_in_new - ki_in_old) / m2
                    - config.resolution * ki * (k_cand - k_old + ki) / (m2 * m2);

                if delta > best_delta || (delta == best_delta && cand_c < best_c) {
                    best_delta = delta;
                    best_c = cand_c;
                }
            }
            if best_c != current_c {
                community.insert(node, best_c);
                *comm_degree.entry(current_c).or_insert(0.0) -= ki;
                *comm_degree.entry(best_c).or_insert(0.0) += ki;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }

    // Leiden refinement: split disconnected sub-communities
    let mut final_comm = community.clone();
    let mut next_id: i64 = *final_comm.values().max().unwrap_or(&0) + 1;
    let mut members_by_comm: HashMap<i64, Vec<i64>> = HashMap::new();
    for &n in &sorted_nodes {
        members_by_comm.entry(final_comm[&n]).or_default().push(n);
    }
    let mut comm_ids: Vec<i64> = members_by_comm.keys().cloned().collect();
    comm_ids.sort_unstable();
    for comm_id in comm_ids {
        let members = &members_by_comm[&comm_id];
        if members.len() <= 1 {
            continue;
        }
        let member_set: HashSet<i64> = members.iter().cloned().collect();
        let mut visited: HashSet<i64> = HashSet::new();
        let mut components: Vec<Vec<i64>> = Vec::new();
        for &start in members {
            if visited.contains(&start) {
                continue;
            }
            let mut comp = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(start);
            visited.insert(start);
            while let Some(node) = queue.pop_front() {
                comp.push(node);
                if let Some(nbrs) = adj.get(&node) {
                    for &nbr in nbrs.keys() {
                        if member_set.contains(&nbr) && !visited.contains(&nbr) {
                            visited.insert(nbr);
                            queue.push_back(nbr);
                        }
                    }
                }
            }
            components.push(comp);
        }
        for component in components.iter().skip(1) {
            let new_id = next_id;
            next_id += 1;
            for &n in component {
                final_comm.insert(n, new_id);
            }
        }
    }

    // Fold tiny clusters into cluster 0 (MISC)
    let mut sizes: HashMap<i64, usize> = HashMap::new();
    for &c in final_comm.values() {
        *sizes.entry(c).or_insert(0) += 1;
    }
    for c in final_comm.values_mut() {
        if *sizes.get(c).unwrap_or(&0) < config.min_cluster_size {
            *c = 0;
        }
    }

    let modularity = {
        let mut q = 0.0f64;
        for (&node, nbrs) in &adj {
            let ci = final_comm[&node];
            for (&nbr, &w) in nbrs {
                if ci == final_comm[&nbr] {
                    q += w - config.resolution * degrees[&node] * degrees[&nbr] / m2;
                }
            }
        }
        q / m2
    };

    (final_comm, modularity)
}

/// Compute clusters for a project and persist to `file_clusters`.
pub fn compute_clusters(
    conn: &Connection,
    project_id: i64,
    config: &LeidenConfig,
) -> Result<ClusterReport> {
    let nodes = crate::db::queries::get_all_node_ids(conn, project_id)?;
    if nodes.is_empty() {
        return Ok(ClusterReport {
            assignments: HashMap::new(),
            modularity: 0.0,
            cluster_count: 0,
        });
    }
    let edges = crate::db::queries::get_all_import_edges(conn, project_id)?;
    let (assignments, modularity) = leiden_raw(&nodes, &edges, config);

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM file_clusters WHERE project_id = ?1",
        rusqlite::params![project_id],
    )?;
    let id_path: Vec<(i64, String)> = {
        let mut stmt = tx.prepare("SELECT id, file_path FROM nodes WHERE project_id = ?1")?;
        let res = stmt
            .query_map(rusqlite::params![project_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        res
    };
    for (node_id, file_path) in id_path {
        let cid = *assignments.get(&node_id).unwrap_or(&0);
        tx.execute(
            "INSERT INTO file_clusters (project_id, file_path, cluster_id, computed_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(project_id, file_path) DO UPDATE SET cluster_id=?3, computed_at=datetime('now')",
            rusqlite::params![project_id, file_path, cid],
        )?;
    }
    tx.commit()?;

    let cluster_count = {
        let mut ids: Vec<i64> = assignments.values().cloned().collect();
        ids.sort_unstable();
        ids.dedup();
        ids.iter().filter(|&&c| c != 0).count()
    };
    Ok(ClusterReport {
        assignments,
        modularity,
        cluster_count,
    })
}
