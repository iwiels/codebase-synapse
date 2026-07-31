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
        // BERT's maximum sequence length for all-MiniLM-L6-v2.
        const MAX_LEN: usize = 512;
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Tokenize every text once, keeping raw ids + attention mask.
        let mut encoded = Vec::with_capacity(texts.len());
        let mut max_len = 0usize;
        for text in texts {
            let tokens = self
                .tokenizer
                .encode(*text, true)
                .map_err(|e| anyhow::anyhow!("Tokenization error: {}", e))?;
            let ids: Vec<u32> = tokens.get_ids().to_vec();
            let mask: Vec<u32> = tokens.get_attention_mask().to_vec();
            let len = ids.len().min(MAX_LEN);
            max_len = max_len.max(len);
            encoded.push((ids, mask, len));
        }
        let max_len = max_len.max(1);

        // Pad to a single batch and stack [B, L] tensors.
        let batch = encoded.len();
        let mut token_ids = vec![0u32; batch * max_len];
        let token_type_ids = vec![0u32; batch * max_len];
        let mut attention_mask = vec![0u32; batch * max_len];
        for (i, (ids, mask, len)) in encoded.iter().enumerate() {
            let base = i * max_len;
            token_ids[base..base + *len].copy_from_slice(&ids[..*len]);
            attention_mask[base..base + *len].copy_from_slice(&mask[..*len]);
        }

        let token_ids = Tensor::new(token_ids, &self.device)?.reshape((batch, max_len))?;
        let token_type_ids =
            Tensor::new(token_type_ids, &self.device)?.reshape((batch, max_len))?;
        let attention_mask =
            Tensor::new(attention_mask, &self.device)?.reshape((batch, max_len))?;

        // One forward pass for the whole batch.
        let output = self
            .model
            .forward(&token_ids, &token_type_ids, Some(&attention_mask))?;

        // Mean pooling: sum hidden states over seq, divide by real token count.
        let sum_hidden = output.sum_keepdim(1)?; // [B, 1, H]
        let count = attention_mask
            .to_dtype(candle_core::DType::F32)?
            .sum_keepdim(1)?
            .unsqueeze(2)?; // [B, 1, 1]
        let count = count.broadcast_as(sum_hidden.shape())?;
        let pooled = (sum_hidden / count)?.squeeze(1)?; // [B, H]

        Ok(pooled.to_vec2::<f32>()?)
    }

    fn dimensions(&self) -> usize {
        self.dim
    }
}
