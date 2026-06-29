use anyhow::Result;
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use hf_hub::api::sync::Api;
use tokenizers::Tokenizer;
use tracing::info;

use super::Embedder;

pub struct CandleEmbedder {
    device: Device,
    model: BertModel,
    tokenizer: Tokenizer,
    dim: usize,
}

impl CandleEmbedder {
    pub fn new() -> Result<Self> {
        let api = Api::new()
            .map_err(|e| anyhow::anyhow!("Failed to initialize HuggingFace API: {}", e))?;
        let model_id = std::env::var("EMBEDDING_MODEL_ID")
            .unwrap_or_else(|_| "sentence-transformers/all-MiniLM-L6-v2".to_string());
        let repo = api.model(model_id.to_string());

        info!("Downloading embedding model: {} (first run only)", model_id);

        let config_path = repo.get("config.json").map_err(|e| {
            anyhow::anyhow!("Failed to download config.json for {}: {}", model_id, e)
        })?;
        let tokenizer_path = repo.get("tokenizer.json").map_err(|e| {
            anyhow::anyhow!("Failed to download tokenizer.json for {}: {}", model_id, e)
        })?;
        let weights_path = repo.get("model.safetensors").map_err(|e| {
            anyhow::anyhow!(
                "Failed to download model.safetensors for {}: {}",
                model_id,
                e
            )
        })?;

        let config: Config = serde_json::from_str(&std::fs::read_to_string(&config_path)?)
            .map_err(|e| anyhow::anyhow!("Failed to parse model config.json: {}", e))?;
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Tokenizer error: {}", e))?;

        let device = Device::Cpu;
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)
                .map_err(|e| anyhow::anyhow!("Failed to load model weights: {}", e))?
        };

        let model = BertModel::load(vb, &config)
            .map_err(|e| anyhow::anyhow!("Failed to build BERT model: {}", e))?;
        let dim = config.hidden_size;

        info!(
            "Embedding model loaded ({} dimensions, device: {:?})",
            dim, device
        );

        Ok(Self {
            device,
            model,
            tokenizer,
            dim,
        })
    }
}

impl Embedder for CandleEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            let tokens = self
                .tokenizer
                .encode(*text, true)
                .map_err(|e| anyhow::anyhow!("Tokenization error: {}", e))?;

            let token_ids = tokens.get_ids();
            let token_type_ids = tokens.get_type_ids();
            let attention_mask = tokens.get_attention_mask();

            let token_ids = Tensor::new(token_ids, &self.device)?.unsqueeze(0)?;
            let token_type_ids = Tensor::new(token_type_ids, &self.device)?.unsqueeze(0)?;
            let attention_mask = Tensor::new(attention_mask, &self.device)?.unsqueeze(0)?;

            let output = self
                .model
                .forward(&token_ids, &token_type_ids, Some(&attention_mask))?;

            let (_batch_size, _seq_len, _hidden) = output.dims3().unwrap_or((1, 0, self.dim));
            let sum_hidden = output.sum_keepdim(1)?;
            let count = attention_mask.sum_keepdim(1)?.unsqueeze(2)?;

            let pooled = (sum_hidden / count)?.squeeze(1)?;
            let embedding = pooled.to_vec1::<f32>()?;
            results.push(embedding);
        }

        Ok(results)
    }

    fn dimensions(&self) -> usize {
        self.dim
    }
}
