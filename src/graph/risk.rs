use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::db::queries;
use crate::db::schema::Node;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PlanTarget {
    pub file_path: String,
    pub symbol_name: Option<String>,
}

/// Serializable snapshot of the intent analysis for a single target.
#[derive(Debug, Serialize, Clone)]
pub struct DetectedIntent {
    pub raw: String,
    pub category: String,
    pub confidence: f64,
    pub coupling_risk_multiplier: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TargetFactors {
    pub node_id: i64,
    pub pagerank: f64,
    pub blast_radius_files: usize,
    pub missing_tests: Vec<String>,
    pub git_churn: i64,
    pub complexity: i64,
    pub change_coupling_count: i64,
    pub max_co_changes: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TargetRiskReport {
    pub resolved_node_id: Option<i64>,
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub risk_score: f64,
    pub factors: TargetFactors,
    pub detected_intent: Option<DetectedIntent>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CounterfactualStrategy {
    pub strategy: String,
    pub description: String,
    pub estimated_risk_delta: f64,
    pub priority: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChangeOrderEntry {
    pub order: usize,
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub risk_score: f64,
    pub rationale: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PlanRiskReport {
    pub overall_risk_score: f64,
    pub risk_level: String,
    pub targets_analyzed: Vec<TargetRiskReport>,
    pub boundary_violations: Vec<String>,
    pub counterfactual_recommendations: Vec<CounterfactualStrategy>,
    pub safe_change_order: Vec<ChangeOrderEntry>,
    pub pruned_recommendations: Vec<CounterfactualStrategy>,
    pub confidence_score: f64,
}

/// Repo-wide normalization baselines computed from actual data.
struct RepoBaselines {
    max_pagerank: f64,
    pagerank_percentile_80: f64,
    max_complexity: f64,
    total_files: i64,
}

pub struct RiskEvaluator<'a> {
    conn: &'a Connection,
}

impl<'a> RiskEvaluator<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    fn map_priority(delta: f64) -> String {
        if delta >= 0.10 {
            "HIGH".to_string()
        } else if delta >= 0.05 {
            "MEDIUM".to_string()
        } else {
            "LOW".to_string()
        }
    }

    /// Public entry point. No change intent provided -> full impact set (recall mode).
    pub fn evaluate_plan_risk(
        &self,
        project_id: i64,
        targets: &[PlanTarget],
    ) -> Result<PlanRiskReport> {
        let intents: Vec<Option<String>> = vec![None; targets.len()];
        self.evaluate_plan_risk_with(
            project_id,
            targets,
            &intents,
            &crate::graph::intent::HeuristicIntentAnalyzer,
        )
    }

    /// Convenience: apply a per-target natural-language intent (commit/PR description),
    /// enabling the intent-aware precision pruning layer.
    pub fn evaluate_plan_risk_with_intent(
        &self,
        project_id: i64,
        targets: &[PlanTarget],
        intents: &[Option<String>],
    ) -> Result<PlanRiskReport> {
        self.evaluate_plan_risk_with(
            project_id,
            targets,
            intents,
            &crate::graph::intent::HeuristicIntentAnalyzer,
        )
    }

    /// Full control: custom intent analyzer (e.g. LLM-backed) + per-target intents.
    pub fn evaluate_plan_risk_with(
        &self,
        project_id: i64,
        targets: &[PlanTarget],
        intents: &[Option<String>],
        analyzer: &dyn crate::graph::intent::IntentAnalyzer,
    ) -> Result<PlanRiskReport> {
        let baselines = self.compute_baselines(project_id)?;

        let mut reports = Vec::new();
        let mut overall_sum = 0.0;
        let mut recommendations = Vec::new();
        let mut boundary_violations = Vec::new();
        let mut all_target_node_ids = Vec::new();
        let mut assessments: Vec<Option<crate::graph::intent::IntentAssessment>> = Vec::new();

        for (ti, target) in targets.iter().enumerate() {
            // Intent-aware precision layer (RIPPLE plan-then-predict phase):
            // classify the stated intent and derive a coupling-risk multiplier.
            let assessment = intents
                .get(ti)
                .and_then(|o| o.as_ref())
                .map(|intent| analyzer.analyze(intent));
            let coupling_mult = assessment
                .as_ref()
                .map(|a| a.coupling_risk_multiplier)
                .unwrap_or(1.0);
            let detected_intent = assessment.as_ref().map(|a| DetectedIntent {
                raw: intents.get(ti).and_then(|o| o.clone()).unwrap_or_default(),
                category: a.intent.as_str().to_string(),
                confidence: (a.confidence * 100.0).round() / 100.0,
                coupling_risk_multiplier: a.coupling_risk_multiplier,
            });

            let node = self.resolve_target(project_id, target)?;
            if let Some(n) = node {
                all_target_node_ids.push(n.id);
                let factors = self.analyze_factors(project_id, &n, coupling_mult)?;
                let risk_score = self.calculate_risk_score(&factors, &baselines);
                overall_sum += risk_score;

                reports.push(TargetRiskReport {
                    resolved_node_id: Some(n.id),
                    file_path: n.file_path.clone(),
                    symbol_name: n.name.clone(),
                    risk_score,
                    factors,
                    detected_intent: detected_intent.clone(),
                });
            } else {
                reports.push(TargetRiskReport {
                    resolved_node_id: None,
                    file_path: target.file_path.clone(),
                    symbol_name: target.symbol_name.clone(),
                    risk_score: 0.05,
                    factors: TargetFactors {
                        node_id: -1,
                        pagerank: 0.0,
                        blast_radius_files: 0,
                        missing_tests: vec![],
                        git_churn: 0,
                        complexity: 1,
                        change_coupling_count: 0,
                        max_co_changes: 1,
                    },
                    detected_intent,
                });
                overall_sum += 0.05;
            }
            assessments.push(assessment);
        }

        if let Ok(violations) = self.check_boundary_violations(project_id, &all_target_node_ids) {
            for v in violations {
                boundary_violations.push(format!(
                    "Import boundary drift: '{}' -> '{}' violates deny pattern '{}'",
                    v.from_file, v.to_file, v.deny_pattern
                ));
            }
        }

        let count = targets.len() as f64;
        let mut overall_risk_score = if count > 0.0 {
            let base = overall_sum / count;
            if !boundary_violations.is_empty() {
                (base + 0.15).min(1.0)
            } else {
                base
            }
        } else {
            0.0
        };

        // Cross-target interaction: if two targets are in the same call chain, elevate risk
        let interaction_bonus = self.check_cross_target_interactions(&reports);
        if interaction_bonus > 0.0 {
            overall_risk_score = (overall_risk_score + interaction_bonus).min(1.0);
        }

        let risk_level = if overall_risk_score >= 0.7 {
            "CRITICAL"
        } else if overall_risk_score >= 0.4 {
            "MEDIUM"
        } else {
            "LOW"
        };

        // Generate graph-driven counterfactual recommendations
        let mut pruned_recommendations = Vec::new();
        for (idx, r) in reports.iter().enumerate() {
            if let Some(node_id) = r.resolved_node_id {
                let mut recs = self
                    .generate_counterfactual_recommendations(project_id, node_id, r, &baselines);
                // Intent-aware pruning (RIPPLE precision phase): drop strategies that
                // the stated intent has rendered false-positive for this target.
                if let Some(assessment) = assessments.get(idx).and_then(|a| a.as_ref()) {
                    recs.retain(|rec| {
                        if assessment.prune_strategies.contains(rec.strategy.as_str()) {
                            pruned_recommendations.push(rec.clone());
                            false
                        } else {
                            true
                        }
                    });
                }
                recommendations.append(&mut recs);
            }
        }

        // Deduplicate recommendations by strategy+description
        let mut unique_recs = Vec::new();
        let mut seen = HashSet::new();
        for rec in recommendations {
            let key = format!("{}:{}", rec.strategy, rec.description);
            if seen.insert(key) {
                unique_recs.push(rec);
            }
        }

        // Sort by estimated_risk_delta descending (highest impact first)
        unique_recs.sort_by(|a, b| {
            b.estimated_risk_delta
                .partial_cmp(&a.estimated_risk_delta)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Compute safe change order
        let safe_change_order = self.compute_safe_change_order(&reports);

        // Calculate confidence score based on index completeness
        let git_commits_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM git_commits WHERE project_id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let test_files_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE project_id = ?1 AND kind = 'test'",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let mut confidence_score: f64 = 1.0;
        if git_commits_count == 0 {
            confidence_score -= 0.3; // no git history = churn score is incomplete
        }
        if test_files_count == 0 {
            confidence_score -= 0.2; // no tests indexed = missing test contracts
        }
        let confidence_score = (confidence_score * 100.0).round() / 100.0;
        let confidence_score = confidence_score.clamp(0.0, 1.0);

        Ok(PlanRiskReport {
            overall_risk_score,
            risk_level: risk_level.to_string(),
            targets_analyzed: reports,
            boundary_violations,
            counterfactual_recommendations: unique_recs,
            safe_change_order,
            pruned_recommendations,
            confidence_score,
        })
    }

    // ── Baselines ──

    fn compute_baselines(&self, project_id: i64) -> Result<RepoBaselines> {
        let max_pagerank: f64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(pagerank), 0.0) FROM node_pagerank WHERE project_id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let total: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM node_pagerank WHERE project_id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let pagerank_percentile_80: f64 = if total > 0 {
            let offset = (total as f64 * 0.8).floor() as i64;
            self.conn
                .query_row(
                    "SELECT COALESCE(pagerank, 0.05) FROM node_pagerank
                 WHERE project_id = ?1
                 ORDER BY pagerank DESC
                 LIMIT 1 OFFSET ?2",
                    rusqlite::params![project_id, offset],
                    |row| row.get(0),
                )
                .unwrap_or(0.05)
        } else {
            0.05
        };

        let max_complexity: f64 = self.conn.query_row(
            "SELECT COALESCE(MAX(complexity), 1) FROM nodes WHERE project_id = ?1 AND complexity IS NOT NULL",
            rusqlite::params![project_id],
            |row| row.get(0),
        ).unwrap_or(1.0);

        let total_files: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT file_path) FROM nodes WHERE project_id = ?1 AND kind = 'file'",
            rusqlite::params![project_id],
            |row| row.get(0),
        ).unwrap_or(1);

        Ok(RepoBaselines {
            max_pagerank: max_pagerank.max(0.001),
            pagerank_percentile_80: pagerank_percentile_80.max(0.001),
            max_complexity: max_complexity.max(1.0),
            total_files: total_files.max(1),
        })
    }

    // ── Target resolution ──

    fn resolve_target(&self, project_id: i64, target: &PlanTarget) -> Result<Option<Node>> {
        let normalized_path = target.file_path.replace('\\', "/");
        let path_pattern = format!("%{}", normalized_path);

        if let Some(symbol) = &target.symbol_name {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
                        doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
                        created_at, updated_at
                 FROM nodes
                 WHERE project_id = ?1
                   AND REPLACE(file_path, '\\', '/') LIKE ?2
                   AND name = ?3
                 LIMIT 1"
            )?;
            let mut rows = stmt.query(rusqlite::params![project_id, path_pattern, symbol])?;
            if let Some(row) = rows.next()? {
                return Ok(Some(queries::row_to_node(row)?));
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
                        doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
                        created_at, updated_at
                 FROM nodes
                 WHERE project_id = ?1
                   AND REPLACE(file_path, '\\', '/') LIKE ?2
                   AND kind = 'file'
                 LIMIT 1"
            )?;
            let mut rows = stmt.query(rusqlite::params![project_id, path_pattern])?;
            if let Some(row) = rows.next()? {
                return Ok(Some(queries::row_to_node(row)?));
            }
        }
        Ok(None)
    }

    // ── Factor analysis ──

    fn analyze_factors(
        &self,
        project_id: i64,
        node: &Node,
        coupling_risk_multiplier: f64,
    ) -> Result<TargetFactors> {
        let pagerank = queries::get_node_pagerank(self.conn, node.id).unwrap_or(0.0);

        // Blast radius: count distinct files among direct+transitive dependents (depth 2)
        let dependents = queries::get_call_graph(self.conn, node.id, "callers", 2)?;
        let mut affected_files = HashSet::new();
        for dep in &dependents {
            affected_files.insert(dep.file_path.clone());
        }
        let blast_radius_files = affected_files.len();

        // Git churn: count distinct commits touching this node
        let git_churn: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(DISTINCT cnl.commit_hash)
             FROM commit_node_links cnl
             WHERE cnl.node_id = ?1 AND cnl.project_id = ?2",
                rusqlite::params![node.id, project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Missing tests: check each dependent for incoming test_of edges
        let missing_tests = self.find_untested_dependents(&dependents)?;

        let complexity = node.complexity.unwrap_or(1);

        // Change coupling: count how many files frequently co-change with this node
        // If the target is a function/method, check the file node's coupling instead
        let coupling_node_id = if node.kind == "function" || node.kind == "method" {
            // Find the file node for this file_path
            self.conn.query_row(
                "SELECT id FROM nodes WHERE project_id = ?1 AND kind = 'file' AND file_path = ?2 LIMIT 1",
                rusqlite::params![project_id, node.file_path],
                |row| row.get::<_, i64>(0),
            ).unwrap_or(node.id)
        } else {
            node.id
        };
        // Intent-aware scaling: low-impact intents (docs, logs) shrink the coupling signal.
        let raw_coupling =
            crate::graph::change_coupling::get_coupling_strength(self.conn, coupling_node_id)
                .unwrap_or(0);
        let change_coupling_count =
            ((raw_coupling as f64) * coupling_risk_multiplier.clamp(0.0, 1.0)).round() as i64;
        let max_co_changes =
            crate::graph::change_coupling::get_max_co_changes(self.conn, project_id).unwrap_or(1);

        Ok(TargetFactors {
            node_id: node.id,
            pagerank,
            blast_radius_files,
            missing_tests,
            git_churn,
            complexity,
            change_coupling_count,
            max_co_changes,
        })
    }

    /// Check each dependent to see if it has incoming test_of edges (actual test coverage).
    fn find_untested_dependents(&self, dependents: &[Node]) -> Result<Vec<String>> {
        let mut untested = Vec::new();
        for dep in dependents {
            let has_test = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM edges WHERE target_node_id = ?1 AND kind = 'test_of'",
                    rusqlite::params![dep.id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;

            if !has_test {
                untested.push(dep.file_path.clone());
            }
        }
        Ok(untested)
    }

    // ── Risk scoring ──

    fn calculate_risk_score(&self, f: &TargetFactors, baselines: &RepoBaselines) -> f64 {
        // Logarithmic scaling for PageRank to suppress outlier dilution
        let pr_norm = if baselines.max_pagerank > 0.0 {
            ((1.0 + f.pagerank * 100.0).ln() / (1.0 + baselines.max_pagerank * 100.0).ln()).min(1.0)
        } else {
            0.0
        };

        // Normalize using logical ceilings (dynamic blast radius ceiling / 15 commits)
        let blast_ceiling = (baselines.total_files as f64 * 0.3).clamp(5.0, 20.0);
        let blast_norm = (f.blast_radius_files as f64 / blast_ceiling).min(1.0);
        let churn_norm = (f.git_churn as f64 / 15.0).min(1.0);

        // Compute test coverage normalization
        let test_norm = if f.blast_radius_files > 0 {
            f.missing_tests.len() as f64 / f.blast_radius_files as f64
        } else {
            // Leaf node: check if the node itself has a test_of contract
            let has_self_test = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM edges WHERE target_node_id = ?1 AND kind = 'test_of'",
                    rusqlite::params![f.node_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;

            if has_self_test {
                0.0
            } else {
                0.5 // untested leaf node penalty
            }
        };

        let comp_norm = (f.complexity as f64 / baselines.max_complexity).min(1.0);

        // Change coupling normalization: how many files co-change with this node
        // Higher coupling = more implicit dependencies = higher risk
        let coupling_norm = if f.max_co_changes > 0 {
            (f.change_coupling_count as f64 / (f.max_co_changes as f64).min(20.0)).min(1.0)
        } else {
            0.0
        };

        let w_pr = 0.25;
        let w_blast = 0.20;
        let w_churn = 0.15;
        let w_test = 0.15;
        let w_comp = 0.10;
        let w_coupling = 0.15;

        (pr_norm * w_pr)
            + (blast_norm * w_blast)
            + (churn_norm * w_churn)
            + (test_norm * w_test)
            + (comp_norm * w_comp)
            + (coupling_norm * w_coupling)
    }

    // ── Cross-target interaction ──

    fn check_cross_target_interactions(&self, reports: &[TargetRiskReport]) -> f64 {
        let node_ids: Vec<i64> = reports.iter().filter_map(|r| r.resolved_node_id).collect();

        if node_ids.len() < 2 {
            return 0.0;
        }

        // Check if any pair of targets is in the same call chain
        let mut interaction_count = 0i64;
        for i in 0..node_ids.len() {
            for j in (i + 1)..node_ids.len() {
                if let Ok(path) = queries::find_path(self.conn, node_ids[i], node_ids[j], 5) {
                    if !path.is_empty() {
                        interaction_count += 1;
                    }
                }
            }
        }

        // Each interaction adds up to 0.05 to the overall risk, capped at 0.2
        (interaction_count as f64 * 0.05).min(0.2)
    }

    // ── Safe change ordering ──

    fn compute_safe_change_order(&self, reports: &[TargetRiskReport]) -> Vec<ChangeOrderEntry> {
        let n = reports.len();
        if n == 0 {
            return vec![];
        }
        if n == 1 {
            let r = &reports[0];
            return vec![ChangeOrderEntry {
                order: 1,
                file_path: r.file_path.clone(),
                symbol_name: r.symbol_name.clone(),
                risk_score: r.risk_score,
                rationale: "only target".to_string(),
            }];
        }

        // Build index: node_id → report index
        let mut id_to_idx: HashMap<i64, usize> = HashMap::new();
        for (i, r) in reports.iter().enumerate() {
            if let Some(nid) = r.resolved_node_id {
                id_to_idx.insert(nid, i);
            }
        }

        // Build dependency edges between targets
        // If A calls B, then B must come before A (B is a dependency of A)
        // adj[u] contains v means v must come before u
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut in_degree: Vec<usize> = vec![0; n];

        for (i, r) in reports.iter().enumerate() {
            if let Some(nid) = r.resolved_node_id {
                // Get callees of this target that are also targets
                if let Ok(callees) = queries::get_call_graph(self.conn, nid, "callees", 2) {
                    for callee in callees {
                        if let Some(&j) = id_to_idx.get(&callee.id) {
                            if i != j {
                                // i calls j → j must come before i
                                // So j → i in topological order
                                adj[j].push(i);
                                in_degree[i] += 1;
                            }
                        }
                    }
                }
            }
        }

        // Kahn's algorithm with risk-score tiebreaking (ascending)
        // Start with nodes that have no incoming edges (no deps among targets)
        let mut queue: VecDeque<usize> = VecDeque::new();
        for (i, deg) in in_degree.iter().enumerate().take(n) {
            if *deg == 0 {
                queue.push_back(i);
            }
        }

        // Sort initial queue by risk score ascending
        let mut initial: Vec<usize> = queue.drain(..).collect();
        initial.sort_by(|a, b| {
            reports[*a]
                .risk_score
                .partial_cmp(&reports[*b].risk_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &idx in &initial {
            queue.push_back(idx);
        }

        let mut order = Vec::with_capacity(n);
        let mut visited = 0usize;

        while let Some(current) = queue.pop_front() {
            visited += 1;
            let r = &reports[current];

            let rationale = if adj[current].is_empty() && in_degree[current] == 0 {
                "no dependencies among targets".to_string()
            } else if in_degree[current] == 0 {
                "all dependencies satisfied".to_string()
            } else {
                "no dependencies among targets".to_string()
            };

            order.push(ChangeOrderEntry {
                order: visited,
                file_path: r.file_path.clone(),
                symbol_name: r.symbol_name.clone(),
                risk_score: r.risk_score,
                rationale,
            });

            // For each neighbor that this node unlocks
            // We reversed the edges: adj[j] contains i means j must come before i
            // So we need to find all nodes that depend on current and decrement their in_degree
            // But we stored it as adj[j] → i, so we need the reverse: for each i where current ∈ adj[i]
            // Actually let me re-check: adj[j].push(i) means j→i edge, so in_degree[i]++
            // So to process neighbors: we need to find all i where current has an edge TO i
            // That's the reverse of what we stored. Let me fix this.

            // Actually the edges are: adj[j].push(i) meaning j→i (j before i)
            // So when j is processed, we decrement in_degree[i]
            // That means we need: for each i in adj[current], decrement in_degree[i]
            for &next in &adj[current] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    // Insert into queue maintaining risk-score order
                    let pos = queue
                        .iter()
                        .position(|&idx| {
                            reports[idx]
                                .risk_score
                                .partial_cmp(&reports[next].risk_score)
                                .unwrap_or(std::cmp::Ordering::Equal)
                                == std::cmp::Ordering::Greater
                        })
                        .unwrap_or(queue.len());
                    queue.insert(pos, next);
                }
            }
        }

        // If we didn't visit all nodes, there's a cycle — append remaining by risk score
        if visited < n {
            let mut remaining: Vec<usize> = (0..n)
                .filter(|i| {
                    !order.iter().any(|e| {
                        e.file_path == reports[*i].file_path
                            && e.symbol_name == reports[*i].symbol_name
                    })
                })
                .collect();
            remaining.sort_by(|a, b| {
                reports[*a]
                    .risk_score
                    .partial_cmp(&reports[*b].risk_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for idx in remaining {
                let r = &reports[idx];
                visited += 1;
                order.push(ChangeOrderEntry {
                    order: visited,
                    file_path: r.file_path.clone(),
                    symbol_name: r.symbol_name.clone(),
                    risk_score: r.risk_score,
                    rationale: "cycle detected — ordered by risk score".to_string(),
                });
            }
        }

        order
    }

    // ── Counterfactual recommendation engine ──

    fn generate_counterfactual_recommendations(
        &self,
        project_id: i64,
        node_id: i64,
        report: &TargetRiskReport,
        baselines: &RepoBaselines,
    ) -> Vec<CounterfactualStrategy> {
        let mut recs = Vec::new();

        let _w_pr = 0.3;
        let _w_blast = 0.25;
        let w_churn = 0.15;
        let w_test = 0.2;
        let w_comp = 0.1;

        // 1. Interface segregation: find traits/interfaces this node implements or could implement
        if report.factors.pagerank / baselines.max_pagerank > 0.3
            || report.factors.blast_radius_files > 5
        {
            if let Some(name) = &report.symbol_name {
                if let Ok(interfaces) = self.find_related_interfaces(node_id) {
                    if !interfaces.is_empty() {
                        let iface_names: Vec<String> =
                            interfaces.iter().filter_map(|n| n.name.clone()).collect();
                        let delta = 0.08;
                        recs.push(CounterfactualStrategy {
                            strategy: "interface_segregation".to_string(),
                            description: format!(
                                "'{}' implements traits/interfaces {:?}. Instead of modifying its signature directly, extend the trait to add a new method variant. This preserves backward compatibility for all {} callers.",
                                name, iface_names, report.factors.blast_radius_files
                            ),
                            estimated_risk_delta: delta,
                            priority: Self::map_priority(delta),
                        });
                    } else {
                        // No interfaces found — suggest creating one
                        let delta = 0.06;
                        recs.push(CounterfactualStrategy {
                            strategy: "extract_interface".to_string(),
                            description: format!(
                                "'{}' has no associated interface/trait but has a blast radius of {} files. Extract its public API into a trait and program against it. This decouples callers from implementation changes.",
                                name, report.factors.blast_radius_files
                            ),
                            estimated_risk_delta: delta,
                            priority: Self::map_priority(delta),
                        });
                    }
                }
            }
        }

        // 2. Add test contracts: identify specific untested dependents
        if !report.factors.missing_tests.is_empty() {
            let untested_count = report.factors.missing_tests.len();
            let sample: Vec<&str> = report
                .factors
                .missing_tests
                .iter()
                .take(3)
                .map(|s| s.as_str())
                .collect();
            let raw_delta = if report.factors.blast_radius_files > 0 {
                w_test * (untested_count as f64 / report.factors.blast_radius_files as f64)
            } else {
                0.0
            };
            let delta = (raw_delta * 1000.0).round() / 1000.0;
            recs.push(CounterfactualStrategy {
                strategy: "add_test_contract".to_string(),
                description: format!(
                    "{} dependents lack test coverage: {:?}. Before modifying this node, add test_of edges (test contracts) for these files. This reduces regression risk from {}% of dependents untested to a verifiable baseline.",
                    untested_count, sample,
                    untested_count.checked_mul(100).and_then(|val| val.checked_div(report.factors.blast_radius_files)).unwrap_or(0)
                ),
                estimated_risk_delta: delta,
                priority: Self::map_priority(delta),
            });
        }

        // 3. Check if the node itself lacks direct tests
        let has_direct_test = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE target_node_id = ?1 AND kind = 'test_of'",
                rusqlite::params![node_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_direct_test && report.risk_score >= 0.4 {
            let delta = w_test * 0.5; // removes untested leaf penalty
            recs.push(CounterfactualStrategy {
                strategy: "add_self_test".to_string(),
                description: format!(
                    "Node '{}' has no direct test contract (test_of edge). Create a test that exercises its public API before modifying it. This establishes a regression baseline.",
                    report.symbol_name.as_deref().unwrap_or(&report.file_path)
                ),
                estimated_risk_delta: delta,
                priority: Self::map_priority(delta),
            });
        }

        // 4. Refactor hotspot: find similar functions that could absorb or share responsibility
        if report.factors.git_churn > 7 {
            let git_churn_norm = (report.factors.git_churn as f64 / 15.0).min(1.0);
            let delta = (w_churn * git_churn_norm * 0.4 * 1000.0).round() / 1000.0;
            if let Ok(alternatives) = self.find_alternative_implementations(node_id) {
                if !alternatives.is_empty() {
                    let alt_names: Vec<String> = alternatives
                        .iter()
                        .filter_map(|n| n.name.clone())
                        .take(3)
                        .collect();
                    recs.push(CounterfactualStrategy {
                        strategy: "refactor_hotspot".to_string(),
                        description: format!(
                            "'{}' has high churn ({} commits). Similar functions exist: {:?}. Consider extracting the frequently-changed logic into a dedicated module or merging with an existing implementation to reduce churn surface.",
                            report.symbol_name.as_deref().unwrap_or(""),
                            report.factors.git_churn,
                            alt_names
                        ),
                        estimated_risk_delta: delta,
                        priority: Self::map_priority(delta),
                    });
                } else {
                    recs.push(CounterfactualStrategy {
                        strategy: "partition_changes".to_string(),
                        description: format!(
                            "File '{}' has high Git churn ({} commits) with no similar alternatives found. Partition changes into small, isolated PRs. Each PR should modify one concern to minimize conflict and regression surface.",
                            report.file_path, report.factors.git_churn
                        ),
                        estimated_risk_delta: delta,
                        priority: Self::map_priority(delta),
                    });
                }
            }
        }

        // 5. Divide and conquer: find extraction candidates from high-complexity nodes
        if report.factors.complexity as f64 / baselines.max_complexity > 0.5 {
            let comp_norm = (report.factors.complexity as f64 / baselines.max_complexity).min(1.0);
            let delta = (w_comp * (comp_norm / 2.0) * 1000.0).round() / 1000.0;
            if let Ok(extraction_candidates) = self.find_extraction_candidates(node_id) {
                if !extraction_candidates.is_empty() {
                    let cand_names: Vec<String> = extraction_candidates
                        .iter()
                        .filter_map(|n| n.name.clone())
                        .take(3)
                        .collect();
                    recs.push(CounterfactualStrategy {
                        strategy: "divide_and_conquer".to_string(),
                        description: format!(
                            "Node '{}' has high complexity ({}). Sub-calls that could be extracted: {:?}. Extract these into separate functions before applying functional changes to reduce the blast radius of each modification.",
                            report.symbol_name.as_deref().unwrap_or(""),
                            report.factors.complexity,
                            cand_names
                        ),
                        estimated_risk_delta: delta,
                        priority: Self::map_priority(delta),
                    });
                } else {
                    recs.push(CounterfactualStrategy {
                        strategy: "divide_and_conquer".to_string(),
                        description: format!(
                            "Node '{}' has high complexity ({}). Decompose into smaller functions before modifying logic. Each extraction reduces the cyclomatic complexity and makes regression testing more targeted.",
                            report.symbol_name.as_deref().unwrap_or(""),
                            report.factors.complexity
                        ),
                        estimated_risk_delta: delta,
                        priority: Self::map_priority(delta),
                    });
                }
            }
        }

        // 6. Call chain isolation: if the node is called by high-pagerank nodes, suggest wrapper
        if report.factors.pagerank / baselines.max_pagerank > 0.2 {
            let delta = 0.07;
            if let Ok(caller_nodes) =
                self.find_high_pagerank_callers(node_id, baselines.pagerank_percentile_80)
            {
                if !caller_nodes.is_empty() {
                    let caller_names: Vec<String> = caller_nodes
                        .iter()
                        .filter_map(|n| n.name.clone())
                        .take(3)
                        .collect();
                    recs.push(CounterfactualStrategy {
                        strategy: "isolate_call_chain".to_string(),
                        description: format!(
                            "High-centrality callers depend on this node: {:?}. Introduce an adapter or wrapper function to isolate your changes from these critical paths. This prevents cascading regressions into high-PageRank hubs.",
                            caller_names
                        ),
                        estimated_risk_delta: delta,
                        priority: Self::map_priority(delta),
                    });
                }
            }
        }

        // 7. Change coupling: files that frequently co-change with this node
        if report.factors.change_coupling_count > 0 {
            // Use file node for coupling lookup (coupling edges are between file nodes)
            let coupling_node_id = if let Some(name) = &report.symbol_name {
                if !name.is_empty() {
                    self.conn.query_row(
                        "SELECT id FROM nodes WHERE project_id = ?1 AND kind = 'file' AND file_path = ?2 LIMIT 1",
                        rusqlite::params![project_id, report.file_path],
                        |row| row.get::<_, i64>(0),
                    ).unwrap_or(node_id)
                } else {
                    node_id
                }
            } else {
                node_id
            };
            let coupled =
                crate::graph::change_coupling::get_coupled_nodes(self.conn, coupling_node_id)
                    .unwrap_or_default();
            if !coupled.is_empty() {
                let coupled_info: Vec<(String, i64)> = coupled
                    .iter()
                    .take(5)
                    .map(|(n, count)| (n.file_path.clone(), *count))
                    .collect();
                let coupling_norm = if report.factors.max_co_changes > 0 {
                    (report.factors.change_coupling_count as f64
                        / (report.factors.max_co_changes as f64).min(20.0))
                    .min(1.0)
                } else {
                    0.0
                };
                let w_coupling_rec = 0.15;
                let delta = (w_coupling_rec * coupling_norm * 1000.0).round() / 1000.0;
                if delta > 0.0 {
                    let descriptions: Vec<String> = coupled_info
                        .iter()
                        .map(|(path, count)| format!("{} ({} co-changes)", path, count))
                        .collect();
                    recs.push(CounterfactualStrategy {
                        strategy: "account_change_coupling".to_string(),
                        description: format!(
                            "'{}' has evolutionary coupling with {} files that frequently change together: {:?}. When modifying this node, proactively review and test these co-changed files. They share implicit dependencies not visible in the code graph.",
                            report.symbol_name.as_deref().unwrap_or(""),
                            coupled_info.len(),
                            descriptions
                        ),
                        estimated_risk_delta: delta,
                        priority: Self::map_priority(delta),
                    });
                }
            }
        }

        recs
    }

    // ── Graph inspection helpers for counterfactual reasoning ──

    /// Find traits/interfaces that this node implements (via 'implements' edges).
    fn find_related_interfaces(&self, node_id: i64) -> Result<Vec<Node>> {
        // Check if the node itself implements something
        let implements = queries::get_edges_by_source(self.conn, node_id, Some("implements"))?;
        let mut results: Vec<Node> = implements.into_iter().map(|(_, n)| n).collect();

        // Also check the parent container (file/module) for trait definitions
        let container = self.conn.query_row(
            "SELECT n2.id FROM edges e
             JOIN nodes n1 ON n1.id = e.source_node_id
             JOIN nodes n2 ON n2.id = e.target_node_id
             WHERE e.source_node_id = ?1 AND e.kind = 'contained_by'
             LIMIT 1",
            rusqlite::params![node_id],
            |row| row.get::<_, i64>(0),
        );

        if let Ok(container_id) = container {
            let container_implements =
                queries::get_edges_by_source(self.conn, container_id, Some("implements"))?;
            for (_, n) in container_implements {
                if !results.iter().any(|r| r.id == n.id) {
                    results.push(n);
                }
            }
        }

        // Also look for 'extends' relationships
        let extends = queries::get_edges_by_source(self.conn, node_id, Some("extends"))?;
        for (_, n) in extends {
            if !results.iter().any(|r| r.id == n.id) {
                results.push(n);
            }
        }

        Ok(results)
    }

    /// Find functions with similar names or signatures (potential alternatives).
    fn find_alternative_implementations(&self, node_id: i64) -> Result<Vec<Node>> {
        // Get the node's name
        let name = match queries::get_node_by_id(self.conn, node_id)? {
            Some(n) => n.name.unwrap_or_default(),
            None => return Ok(vec![]),
        };

        if name.is_empty() || name.len() < 4 {
            return Ok(vec![]);
        }

        // Search for nodes with similar names (same name in different files, or _alt/_v2 variants)
        let base_name = name
            .trim_end_matches("_alt")
            .trim_end_matches("_v2")
            .trim_end_matches("_old");
        let pattern1 = base_name.to_string();
        let pattern2 = format!("{}_alt", base_name);
        let pattern3 = format!("{}_v2", base_name);

        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, file_path, kind, name, qualified_name, signature,
                    doc_comment, start_line, end_line, complexity, is_exported, content_hash, source, metadata,
                    created_at, updated_at
             FROM nodes
             WHERE kind IN ('function', 'method')
               AND (name = ?1 OR name = ?2 OR name = ?3)
               AND id != ?4
             LIMIT 5"
        )?;
        let rows = stmt.query_map(
            rusqlite::params![pattern1, pattern2, pattern3, node_id],
            queries::row_to_node,
        )?;
        let results: Vec<Node> = rows.filter_map(|r| r.ok()).collect();
        Ok(results)
    }

    /// Find sub-calls of a high-complexity node that could be extraction candidates.
    fn find_extraction_candidates(&self, node_id: i64) -> Result<Vec<Node>> {
        // Get direct callees (depth 1 only) that are functions/methods
        let callees = queries::get_call_graph(self.conn, node_id, "callees", 1)?;
        // Filter to only functions/methods (not external calls)
        let candidates: Vec<Node> = callees
            .into_iter()
            .filter(|n| n.kind == "function" || n.kind == "method")
            .collect();
        Ok(candidates)
    }

    /// Find callers with high PageRank (critical hubs that depend on this node).
    fn find_high_pagerank_callers(&self, node_id: i64, threshold: f64) -> Result<Vec<Node>> {
        let callers = queries::get_call_graph(self.conn, node_id, "callers", 1)?;

        let mut high_pr_callers = Vec::new();
        for caller in callers {
            let pr = queries::get_node_pagerank(self.conn, caller.id).unwrap_or(0.0);
            if pr >= threshold {
                high_pr_callers.push(caller);
            }
        }
        Ok(high_pr_callers)
    }

    // ── Boundary violations (optimized) ──

    fn check_boundary_violations(
        &self,
        project_id: i64,
        node_ids: &[i64],
    ) -> Result<Vec<crate::graph::boundaries::Violation>> {
        if node_ids.is_empty() {
            return Ok(vec![]);
        }

        let root_path = match self.conn.query_row(
            "SELECT root_path FROM projects WHERE id = ?1",
            rusqlite::params![project_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(p) => p,
            Err(_) => return Ok(vec![]),
        };

        let config_path = std::path::Path::new(&root_path)
            .join(".codebase-synapse")
            .join("boundaries.toml");
        if !config_path.exists() {
            return Ok(vec![]);
        }
        let config = crate::graph::boundaries::load_config(&config_path)?;

        // Collect target file paths
        let mut target_files = HashSet::new();
        for &nid in node_ids {
            if let Ok(Some(n)) = queries::get_node_by_id(self.conn, nid) {
                target_files.insert(n.file_path);
            }
        }

        if target_files.is_empty() {
            return Ok(vec![]);
        }

        // Get only the relevant file nodes (not all project files)
        let mut file_map: Vec<(String, i64)> = Vec::new();
        for fp in &target_files {
            if let Ok(id) = self.conn.query_row(
                "SELECT MIN(id) FROM nodes WHERE project_id=?1 AND kind='file' AND file_path=?2",
                rusqlite::params![project_id, fp],
                |row| row.get::<_, i64>(0),
            ) {
                file_map.push((fp.clone(), id));
            }
        }

        // Get import edges only from target nodes (much smaller set)
        let target_node_ids: Vec<i64> = file_map.iter().map(|(_, id)| *id).collect();
        let placeholders: Vec<String> = target_node_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "SELECT source_node_id, target_node_id FROM edges
             WHERE project_id = ?1 AND kind IN ('imports', 'calls', 'references', 'depends_on')
             AND source_node_id IN ({})",
            placeholders.join(",")
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(project_id)];
        for &id in &target_node_ids {
            params.push(Box::new(id));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let edges: Vec<(i64, i64)> = if let Ok(mut stmt) = self.conn.prepare(&query) {
            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        } else {
            vec![]
        };

        let all_violations =
            crate::graph::boundaries::check_boundaries(&config, &file_map, &edges)?;

        // Filter to only violations from target files
        let relevant: Vec<_> = all_violations
            .into_iter()
            .filter(|v| target_files.contains(&v.from_file))
            .collect();

        Ok(relevant)
    }
}
