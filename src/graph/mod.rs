//! Code knowledge graph: builder, traversal, impact analysis, PageRank, Leiden clustering, boundary enforcement, wiki generation.

pub mod boundaries;
pub mod builder;
pub mod change_coupling;
pub mod impact;
pub mod intent;
pub mod leiden;
pub mod pagerank;
pub mod traversal;
pub mod wiki;

pub use boundaries::{check_boundaries, suggest_boundaries, BoundaryConfig, Violation};
pub use builder::GraphBuilder;
pub use change_coupling as coupling;
pub use impact::ImpactAnalysis;
pub use leiden::{compute_clusters, ClusterReport, LeidenConfig};
pub use pagerank::{compute_pagerank, PageRankConfig};
pub use traversal::GraphTraversal;
pub use wiki::{generate_wiki, render_wiki, WikiConfig};
pub mod risk;
pub use risk::{
    CounterfactualStrategy, PlanRiskReport, PlanTarget, RiskEvaluator, TargetRiskReport,
};
