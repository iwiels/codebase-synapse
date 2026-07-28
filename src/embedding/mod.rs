use std::sync::{Arc, OnceLock};

use anyhow::Result;

#[cfg(feature = "embedding")]
pub mod candle;

#[cfg(feature = "embedding")]
use candle::CandleEmbedder;

pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dimensions(&self) -> usize;
}

pub struct NoopEmbedder;

impl Embedder for NoopEmbedder {
    fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(vec![])
    }
    fn dimensions(&self) -> usize {
        0
    }
}

/// Embedder whose heavy model load (download + BERT build) is deferred
/// until the first `embed()` call. This keeps server startup instant:
/// tools like `list_projects` that never touch embeddings stay fast,
/// and only the first semantic-search call pays the model-load cost.
#[derive(Default)]
pub struct LazyEmbedder {
    inner: OnceLock<Arc<dyn Embedder>>,
}

impl LazyEmbedder {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    fn get(&self) -> &Arc<dyn Embedder> {
        self.inner.get_or_init(|| create_real_embedder())
    }
}

impl Embedder for LazyEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.get().embed(texts)
    }
    fn dimensions(&self) -> usize {
        self.get().dimensions()
    }
}

/// Build the real embedder (Candle BERT, or Noop fallback).
/// Expensive: downloads the model on first run and builds the graph.
#[cfg(feature = "embedding")]
fn create_real_embedder() -> Arc<dyn Embedder> {
    match CandleEmbedder::new() {
        Ok(emb) => Arc::new(emb),
        Err(e) => {
            tracing::warn!(
                "Failed to initialize Candle embedder ({}), falling back to noop",
                e
            );
            Arc::new(NoopEmbedder)
        }
    }
}

#[cfg(not(feature = "embedding"))]
fn create_real_embedder() -> Arc<dyn Embedder> {
    Arc::new(NoopEmbedder)
}

pub fn create_embedder() -> Arc<dyn Embedder> {
    Arc::new(LazyEmbedder::new())
}
