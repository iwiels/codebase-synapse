//! Search layer: BM25 full-text (FTS5), vector cosine similarity, hybrid RRF fusion.

pub mod bm25;
pub mod vector;
pub mod hybrid;

pub use bm25::Bm25Search;
pub use vector::VectorSearch;
pub use hybrid::HybridSearch;
