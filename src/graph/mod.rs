//! Code knowledge graph: builder, traversal, impact analysis, PageRank, Leiden clustering, boundary enforcement, wiki generation.

pub mod builder;
pub mod traversal;
pub mod impact;
pub mod pagerank;
pub mod leiden;
pub mod boundaries;
pub mod wiki;

pub use builder::GraphBuilder;
pub use traversal::GraphTraversal;
pub use impact::ImpactAnalysis;
pub use pagerank::{PageRankConfig, compute_pagerank};
pub use leiden::{LeidenConfig, ClusterReport, compute_clusters};
pub use boundaries::{BoundaryConfig, Violation, check_boundaries, suggest_boundaries};
pub use wiki::{WikiConfig, render_wiki, generate_wiki};
