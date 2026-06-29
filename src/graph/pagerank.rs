//! PageRank over the import/call graph.
//!
//! Supports warm-start from previously stored ranks so incremental
//! re-indexes converge in 1-3 iterations instead of ~15-20 cold.

use crate::db::queries;
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

pub struct PageRankConfig {
    pub damping: f64,
    pub iterations: u32,
    pub epsilon: f64,
}

impl Default for PageRankConfig {
    fn default() -> Self {
        Self {
            damping: 0.85,
            iterations: 30,
            epsilon: 1e-5,
        }
    }
}

/// Pure PageRank computation (no DB access).
///
/// `edges` — (src, dst) where src imports/calls dst (dst is the hub).
/// `prev`  — previous ranks for warm-start (empty = cold start).
///
/// Returns node_id → rank. Ranks approximately sum to 1.0.
pub fn pagerank_inner(
    nodes: &[i64],
    edges: &[(i64, i64)],
    damping: f64,
    max_iter: u32,
    epsilon: f64,
    prev: &HashMap<i64, f64>,
) -> HashMap<i64, f64> {
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }

    // dst receives rank from src (src → dst means src depends on dst, so dst is hub)
    let mut inbound: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut out_degree: HashMap<i64, usize> = HashMap::new();
    for &node in nodes {
        inbound.entry(node).or_default();
        out_degree.entry(node).or_insert(0);
    }
    for &(src, dst) in edges {
        inbound.entry(dst).or_default().push(src);
        *out_degree.entry(src).or_insert(0) += 1;
    }

    let base = 1.0 / n as f64;
    let teleport = (1.0 - damping) / n as f64;

    let mut ranks: HashMap<i64, f64> = nodes
        .iter()
        .map(|&id| (id, *prev.get(&id).unwrap_or(&base)))
        .collect();

    for _ in 0..max_iter {
        let mut new_ranks = HashMap::with_capacity(n);
        let mut delta = 0.0f64;

        // Sum of ranks from sink nodes (out_degree == 0) redistributed to all nodes
        let sink_sum: f64 = nodes
            .iter()
            .filter(|&&node| out_degree[&node] == 0)
            .map(|&node| ranks[&node])
            .sum();
        let sink_teleport = damping * sink_sum / n as f64;

        for &node in nodes {
            let rank_sum: f64 = inbound
                .get(&node)
                .map(|srcs| {
                    srcs.iter()
                        .map(|&src| {
                            let od = out_degree[&src];
                            if od > 0 {
                                ranks[&src] / od as f64
                            } else {
                                0.0
                            }
                        })
                        .sum()
                })
                .unwrap_or(0.0);
            let new_rank = teleport + sink_teleport + damping * rank_sum;
            delta += (new_rank - ranks[&node]).abs();
            new_ranks.insert(node, new_rank);
        }
        ranks = new_ranks;
        if delta < epsilon {
            break;
        }
    }
    ranks
}

/// Compute PageRank for all nodes in a project and persist to `node_pagerank`.
pub fn compute_pagerank(conn: &Connection, project_id: i64, config: &PageRankConfig) -> Result<()> {
    let nodes = queries::get_all_node_ids(conn, project_id)?;
    if nodes.is_empty() {
        return Ok(());
    }
    let edges = queries::get_all_import_edges(conn, project_id)?;
    let prev = queries::get_prev_pageranks(conn, project_id)?;
    let ranks = pagerank_inner(
        &nodes,
        &edges,
        config.damping,
        config.iterations,
        config.epsilon,
        &prev,
    );

    let tx = conn.unchecked_transaction()?;
    for (&node_id, &rank) in &ranks {
        let changed = prev
            .get(&node_id)
            .is_none_or(|&p| (rank - p).abs() >= config.epsilon);
        if changed {
            queries::update_node_pagerank(&tx, node_id, project_id, rank)?;
        }
    }
    tx.commit()?;
    Ok(())
}
