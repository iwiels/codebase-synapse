use std::sync::Arc;

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

pub fn create_embedder() -> Arc<dyn Embedder> {
    #[cfg(feature = "embedding")]
    {
        match CandleEmbedder::new() {
            Ok(emb) => return Arc::new(emb),
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize Candle embedder ({}), falling back to noop",
                    e
                );
            }
        }
    }
    Arc::new(NoopEmbedder)
}
