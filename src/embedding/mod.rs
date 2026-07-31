use std::sync::{Arc, LazyLock, OnceLock};

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
        if let Some(emb) = self.inner.get() {
            return emb;
        }
        // Only real embedders are cached. A failed init (e.g. interrupted
        // model download) returns None, so the next call retries instead of
        // permanently degrading to a noop until a server restart.
        if let Some(emb) = create_real_embedder() {
            let _ = self.inner.set(emb);
        }
        self.inner.get().unwrap_or(&NOOP_FALLBACK)
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

/// Shared noop used when a real embedder is unavailable (feature disabled
/// or a transient init failure). Never cached by `LazyEmbedder`.
static NOOP_FALLBACK: LazyLock<Arc<dyn Embedder>> =
    LazyLock::new(|| Arc::new(NoopEmbedder));

/// Build the real embedder (Candle BERT).
/// Expensive: downloads the model on first run and builds the graph.
/// `None` means "not available right now" (feature disabled or failed
/// init) and is retried on the next call.
#[cfg(feature = "embedding")]
fn create_real_embedder() -> Option<Arc<dyn Embedder>> {
    match CandleEmbedder::new() {
        Ok(emb) => Some(Arc::new(emb)),
        Err(e) => {
            tracing::warn!("Failed to initialize Candle embedder ({}), retrying on next use", e);
            None
        }
    }
}

#[cfg(not(feature = "embedding"))]
fn create_real_embedder() -> Option<Arc<dyn Embedder>> {
    None
}

pub fn create_embedder() -> Arc<dyn Embedder> {
    Arc::new(LazyEmbedder::new())
}
