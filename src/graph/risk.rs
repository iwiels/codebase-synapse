use std::collections::HashSet;
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::schema::Node;
use crate::db::queries;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PlanTarget {
    pub file_path: String,
    pub symbol_name: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TargetFactors {
    pub pagerank: f64,
    pub blast_radius_files: usize,
    pub missing_tests: Vec<String>,
    pub git_churn: i64,
    pub complexity: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TargetRiskReport {
    pub resolved_node_id: Option<i64>,
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub risk_score: f64,
    pub factors: TargetFactors,
}

#[derive(Debug, Serialize, Clone)]
pub struct CounterfactualStrategy {
    pub strategy: String,
    pub description: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PlanRiskReport {
    pub overall_risk_score: f64,
    pub risk_level: String,
    pub targets_analyzed: Vec<TargetRiskReport>,
    pub boundary_violations: Vec<String>,
    pub counterfactual_recommendations: Vec<CounterfactualStrategy>,
}

pub struct RiskEvaluator<'a> {
    conn: &'a Connection,
}

impl<'a> RiskEvaluator<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn evaluate_plan_risk(&self, project_id: i64, targets: &[PlanTarget]) -> Result<PlanRiskReport> {
        let mut reports = Vec::new();
        let mut overall_sum = 0.0;
        let mut recommendations = Vec::new();
        let mut boundary_violations = Vec::new();
        
        let mut all_target_node_ids = Vec::new();

        for target in targets {
            let node = self.resolve_target(project_id, target)?;
            if let Some(n) = node {
                all_target_node_ids.push(n.id);
                let factors = self.analyze_factors(project_id, &n)?;
                let risk_score = self.calculate_risk_score(&factors);
                overall_sum += risk_score;

                reports.push(TargetRiskReport {
                    resolved_node_id: Some(n.id),
                    file_path: n.file_path.clone(),
                    symbol_name: n.name.clone(),
                    risk_score,
                    factors,
                });
            } else {
                reports.push(TargetRiskReport {
                    resolved_node_id: None,
                    file_path: target.file_path.clone(),
                    symbol_name: target.symbol_name.clone(),
                    risk_score: 0.1, // Default low risk if target not indexed/found
                    factors: TargetFactors {
                        pagerank: 0.0,
                        blast_radius_files: 0,
                        missing_tests: vec![],
                        git_churn: 0,
                        complexity: 1,
                    },
                });
                overall_sum += 0.1;
            }
        }

        // Check if there are boundary violations
        if let Ok(violations) = self.check_boundary_violations(project_id, &all_target_node_ids) {
            for v in violations {
                boundary_violations.push(format!("Import boundary drift: '{}' -> '{}' violates deny pattern '{}'", v.from_file, v.to_file, v.deny_pattern));
            }
        }

        let count = targets.len() as f64;
        let overall_risk_score = if count > 0.0 {
            let base = overall_sum / count;
            // Elevate overall risk if there are boundary violations
            if !boundary_violations.is_empty() {
                (base + 0.15).min(1.0)
            } else {
                base
            }
        } else {
            0.0
        };

        let risk_level = if overall_risk_score >= 0.7 {
            "CRITICAL"
        } else if overall_risk_score >= 0.4 {
            "MEDIUM"
        } else {
            "LOW"
        };

        // Generate recommendations based on the reports
        for r in &reports {
            if r.risk_score >= 0.4 {
                if let Some(name) = &r.symbol_name {
                    if r.factors.pagerank > 0.05 || r.factors.blast_radius_files > 5 {
                        recommendations.push(CounterfactualStrategy {
                            strategy: "interface_segregation".to_string(),
                            description: format!(
                                "Symbol '{}' under '{}' is a central hub (PageRank: {:.4}, Blast Radius: {} files). Instead of modifying its signature/internals directly, consider introducing an interface/trait extension or wrapper.",
                                name, r.file_path, r.factors.pagerank, r.factors.blast_radius_files
                            ),
                        });
                    }
                    if !r.factors.missing_tests.is_empty() {
                        recommendations.push(CounterfactualStrategy {
                            strategy: "add_test_contract".to_string(),
                            description: format!(
                                "Symbol '{}' lacks test contracts, and affects downstream files: {:?}. Write test contracts (test_of edges) verifying this function before modifying it.",
                                name, r.factors.missing_tests
                            ),
                        });
                    }
                }
                if r.factors.git_churn > 8 {
                    recommendations.push(CounterfactualStrategy {
                        strategy: "refactor_hotspot".to_string(),
                        description: format!(
                            "File '{}' has high Git churn ({} commits). Ensure changes are partitioned into small, isolated PRs to avoid conflicts and regression.",
                            r.file_path, r.factors.git_churn
                        ),
                    });
                }
                if r.factors.complexity > 12 {
                    recommendations.push(CounterfactualStrategy {
                        strategy: "divide_and_conquer".to_string(),
                        description: format!(
                            "The node at '{}' has high cyclomatic complexity ({}). Consider refactoring or extracting helpers in a separate step before applying functional changes.",
                            r.file_path, r.factors.complexity
                        ),
                    });
                }
            }
        }

        // Deduplicate recommendations
        let mut unique_recs = Vec::new();
        let mut seen = HashSet::new();
        for rec in recommendations {
            let key = format!("{}:{}", rec.strategy, rec.description);
            if seen.insert(key) {
                unique_recs.push(rec);
            }
        }

        Ok(PlanRiskReport {
            overall_risk_score,
            risk_level: risk_level.to_string(),
            targets_analyzed: reports,
            boundary_violations,
            counterfactual_recommendations: unique_recs,
        })
    }

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
            // Find file node
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

    fn analyze_factors(&self, project_id: i64, node: &Node) -> Result<TargetFactors> {
        let pagerank = queries::get_node_pagerank(self.conn, node.id).unwrap_or(0.0);
        
        // Blast radius: count direct and transitive dependents (up to depth 2)
        let dependents = queries::get_call_graph(self.conn, node.id, "callers", 2)?;
        let mut affected_files = HashSet::new();
        for dep in &dependents {
            affected_files.insert(dep.file_path.clone());
        }
        let blast_radius_files = affected_files.len();

        // Git churn: count commits touching this node
        let git_churn: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT cnl.commit_hash)
             FROM commit_node_links cnl
             WHERE cnl.node_id = ?1 AND cnl.project_id = ?2",
            rusqlite::params![node.id, project_id],
            |row| row.get(0),
        ).unwrap_or(0);

        // Missing tests: get test_of edges (incoming)
        let mut missing_tests = Vec::new();
        let test_edges = queries::get_edges_by_target(self.conn, node.id, Some("test_of"))?;
        if test_edges.is_empty() {
            // If no direct tests, list some downstream callers that lack verification
            for dep in dependents.iter().take(3) {
                missing_tests.push(dep.file_path.clone());
            }
        }

        let complexity = node.complexity.unwrap_or(1);

        Ok(TargetFactors {
            pagerank,
            blast_radius_files,
            missing_tests,
            git_churn,
            complexity,
        })
    }

    fn calculate_risk_score(&self, f: &TargetFactors) -> f64 {
        // Normalise inputs
        let pr_norm = (f.pagerank / 0.15).min(1.0);
        let blast_norm = (f.blast_radius_files as f64 / 10.0).min(1.0);
        let churn_norm = (f.git_churn as f64 / 10.0).min(1.0);
        let test_norm = if f.missing_tests.is_empty() { 0.0 } else { 1.0 };
        let comp_norm = (f.complexity as f64 / 15.0).min(1.0);

        // Weighted sum (total = 1.0)
        let w_pr = 0.3;
        let w_blast = 0.25;
        let w_churn = 0.15;
        let w_test = 0.2;
        let w_comp = 0.1;

        (pr_norm * w_pr) + (blast_norm * w_blast) + (churn_norm * w_churn) + (test_norm * w_test) + (comp_norm * w_comp)
    }

    fn check_boundary_violations(&self, project_id: i64, node_ids: &[i64]) -> Result<Vec<crate::graph::boundaries::Violation>> {
        let project_res = self.conn.query_row(
            "SELECT root_path FROM projects WHERE id = ?1",
            rusqlite::params![project_id],
            |row| row.get::<_, String>(0),
        );
        let root_path = match project_res {
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
        
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT file_path, MIN(id) as id FROM nodes WHERE project_id=?1 AND kind='file' GROUP BY file_path"
        )?;
        let files: Vec<(String, i64)> = stmt.query_map(rusqlite::params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok()).collect();
            
        let edges = queries::get_all_import_edges(self.conn, project_id)?;
        let all_violations = crate::graph::boundaries::check_boundaries(&config, &files, &edges)?;

        // Filter violations where the source matches one of our modified files
        let mut relevant_violations = Vec::new();
        let mut target_files = HashSet::new();
        for &nid in node_ids {
            if let Ok(Some(n)) = queries::get_node_by_id(self.conn, nid) {
                target_files.insert(n.file_path);
            }
        }
        for v in all_violations {
            if target_files.contains(&v.from_file) {
                relevant_violations.push(v);
            }
        }
        
        Ok(relevant_violations)
    }
}
