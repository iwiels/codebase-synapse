//! Search layer: BM25 full-text (FTS5), vector cosine similarity, hybrid RRF fusion.

pub mod bm25;
pub mod hybrid;
pub mod vector;

pub use bm25::Bm25Search;
pub use hybrid::HybridSearch;
pub use vector::VectorSearch;
