//! Cohere Transcribe speech-to-text (proof-of-concept, feature-gated)
//!
//! Uses Cohere Labs' Cohere Transcribe model via ONNX Runtime. This is a
//! feature-gated proof-of-concept: the module is opt-in via `--features cohere`
//! and is NOT wired into the CLI, config, factory, or release builds. See the
//! `cohere_poc` test below for usage.
//!
//! Model architecture (per Cohere's release blog):
//! - Encoder: Fast-Conformer over log-mel spectrogram (~90% of 2B params)
//! - Decoder: lightweight autoregressive Transformer with KV cache
//! - Tokenizer: SentencePiece, distributed as `tokens.txt` in the cstr/ONNX export
//!
//! Reference exports surveyed during research:
//! - `cstr/cohere-transcribe-onnx-int8` (used by this PoC, ~2.4 GB)
//!     Layout: `cohere-encoder.int8.onnx` (+ `.onnx.data`),
//!             `cohere-decoder.int8.onnx` (+ `.onnx.data`),
//!             `tokens.txt`
//! - `cstr/cohere-transcribe-onnx-int4` (~1.2 GB, English-friendly)
//! - `onnx-community/cohere-transcribe-03-2026-ONNX` (multiple quantizations,
//!   different file layout)
//!
//! ## Downloading the int8 model for the PoC test
//!
//! The original `CohereLabs/cohere-transcribe-03-2026` weights are gated on
//! HuggingFace (Apache 2.0 licensed but require accepting the model card).
//! The community ONNX export is typically not gated:
//!
//! ```bash
//! mkdir -p models/cohere-transcribe-int8
//! cd models/cohere-transcribe-int8
//! BASE=https://huggingface.co/cstr/cohere-transcribe-onnx-int8/resolve/main
//! for f in cohere-encoder.int8.onnx cohere-encoder.int8.onnx.data \
//!          cohere-decoder.int8.onnx cohere-decoder.int8.onnx.data \
//!          tokens.txt; do
//!     curl -L "$BASE/$f" -o "$f"
//! done
//! ```
//!
//! ## Running the PoC test
//!
//! ```bash
//! cargo test --features cohere transcribe::cohere -- --ignored --nocapture
//! ```
//!
//! The test is `#[ignore]`d so it doesn't run in CI without the model present.
//!
//! ## What's missing before this can ship
//!
//! - Verified ONNX input/output names (this PoC uses Moonshine-style names as a
//!   starting point: `input_features`, `encoder_hidden_states`, `input_ids`,
//!   `past_key_values.*`, `present.*`, `logits`, `use_cache_branch`). Actual
//!   names should be confirmed via `session.inputs()`/`outputs()` once the
//!   model is loaded for the first time.
//! - Confirmed mel feature settings (n_mels, hop, window, sample rate). Cohere's
//!   blog implies a standard 80-dim 16 kHz log-mel front end, but the exact
//!   constants may differ from `FbankExtractor::new_default()`.
//! - Decoder start/EOS token IDs (this PoC reuses Moonshine's `1` / `2` as a
//!   placeholder; the SentencePiece tokens.txt should be inspected to confirm).
//! - Config struct + CLI flags + factory wiring + setup/model.rs download flow.

use super::ctc;
use super::fbank::FbankExtractor;
use super::Transcriber;
use crate::error::TranscribeError;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Placeholder special token IDs for the Cohere decoder.
///
/// These match the Moonshine convention as a starting point. They MUST be
/// confirmed against the actual SentencePiece `tokens.txt` before this PoC
/// is promoted to a shippable backend.
const DECODER_START_TOKEN_ID: i64 = 1;
const EOS_TOKEN_ID: i64 = 2;

/// Maximum tokens to generate (safety limit). Cohere's decoder is lightweight
/// so this is mainly a runaway-loop guard.
const MAX_TOKENS_PER_SECOND: f32 = 8.0;
const ABSOLUTE_MAX_TOKENS: usize = 1024;

/// Sample rate expected by the log-mel front end.
const SAMPLE_RATE: usize = 16_000;

/// Cohere Transcribe transcriber using ONNX Runtime.
///
/// This struct intentionally mirrors `MoonshineTranscriber`: the decoder loop
/// is identical (autoregressive with KV cache, dummy KV on step 0, encoder KV
/// reused after step 0). The only structural change is that the encoder
/// consumes log-mel features (via `FbankExtractor`) rather than raw audio.
pub struct CohereTranscriber {
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    /// SentencePiece tokens loaded from `tokens.txt` (id -> piece string).
    tokens: HashMap<u32, String>,
    /// Cached decoder input names, used to discover KV cache slot names.
    decoder_input_names: Vec<String>,
    /// Cached decoder output names.
    decoder_output_names: Vec<String>,
    /// KV cache num_heads, detected from model metadata.
    num_heads: usize,
    /// KV cache head_dim, detected from model metadata.
    head_dim: usize,
    /// Log-mel feature extractor (shared with SenseVoice/Paraformer/Dolphin).
    fbank_extractor: FbankExtractor,
}

impl CohereTranscriber {
    /// Load the Cohere encoder + decoder + tokens from a model directory.
    ///
    /// Expects the cstr/cohere-transcribe-onnx-int8 layout:
    /// - `cohere-encoder.int8.onnx` (+ `.onnx.data` sidecar)
    /// - `cohere-decoder.int8.onnx` (+ `.onnx.data` sidecar)
    /// - `tokens.txt`
    pub fn from_dir(model_dir: &Path) -> Result<Self, TranscribeError> {
        Self::with_threads(model_dir, num_cpus::get().min(4))
    }

    /// Load with an explicit thread count for ONNX intra-op parallelism.
    pub fn with_threads(model_dir: &Path, threads: usize) -> Result<Self, TranscribeError> {
        tracing::info!("Loading Cohere Transcribe model from {:?}", model_dir);
        let start = std::time::Instant::now();

        let encoder_file = model_dir.join("cohere-encoder.int8.onnx");
        let decoder_file = model_dir.join("cohere-decoder.int8.onnx");
        let tokens_file = model_dir.join("tokens.txt");

        if !encoder_file.exists() {
            return Err(TranscribeError::ModelNotFound(format!(
                "Cohere encoder not found: {}\n  \
                 Download from https://huggingface.co/cstr/cohere-transcribe-onnx-int8",
                encoder_file.display()
            )));
        }
        if !decoder_file.exists() {
            return Err(TranscribeError::ModelNotFound(format!(
                "Cohere decoder not found: {}\n  \
                 Download from https://huggingface.co/cstr/cohere-transcribe-onnx-int8",
                decoder_file.display()
            )));
        }
        if !tokens_file.exists() {
            return Err(TranscribeError::ModelNotFound(format!(
                "Cohere tokens.txt not found: {}",
                tokens_file.display()
            )));
        }

        let tokens = ctc::load_tokens(&tokens_file)?;
        tracing::debug!("Loaded {} Cohere SentencePiece tokens", tokens.len());

        let encoder = Session::builder()
            .map_err(|e| {
                TranscribeError::InitFailed(format!("ONNX encoder session builder failed: {}", e))
            })?
            .with_intra_threads(threads)
            .map_err(|e| {
                TranscribeError::InitFailed(format!("Failed to set encoder threads: {}", e))
            })?
            .commit_from_file(&encoder_file)
            .map_err(|e| {
                TranscribeError::InitFailed(format!(
                    "Failed to load Cohere encoder from {:?}: {}",
                    encoder_file, e
                ))
            })?;

        let decoder = Session::builder()
            .map_err(|e| {
                TranscribeError::InitFailed(format!("ONNX decoder session builder failed: {}", e))
            })?
            .with_intra_threads(threads)
            .map_err(|e| {
                TranscribeError::InitFailed(format!("Failed to set decoder threads: {}", e))
            })?
            .commit_from_file(&decoder_file)
            .map_err(|e| {
                TranscribeError::InitFailed(format!(
                    "Failed to load Cohere decoder from {:?}: {}",
                    decoder_file, e
                ))
            })?;

        let decoder_input_names: Vec<String> = decoder
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let decoder_output_names: Vec<String> = decoder
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();

        tracing::debug!(
            "Cohere encoder inputs:  {:?}",
            encoder
                .inputs()
                .iter()
                .map(|i| i.name())
                .collect::<Vec<_>>()
        );
        tracing::debug!(
            "Cohere encoder outputs: {:?}",
            encoder
                .outputs()
                .iter()
                .map(|o| o.name())
                .collect::<Vec<_>>()
        );
        tracing::debug!("Cohere decoder inputs:  {:?}", decoder_input_names);
        tracing::debug!("Cohere decoder outputs: {:?}", decoder_output_names);

        // Detect num_heads/head_dim from the first KV cache input's [B, H, T, D]
        // shape, same trick Moonshine uses to stay agnostic across quantizations.
        let (num_heads, head_dim) = decoder
            .inputs()
            .iter()
            .find(|i| i.name().starts_with("past_key_values"))
            .and_then(|input| {
                if let ort::value::ValueType::Tensor { ref shape, .. } = *input.dtype() {
                    let dims: &[i64] = shape;
                    if dims.len() == 4 && dims[1] > 0 && dims[3] > 0 {
                        Some((dims[1] as usize, dims[3] as usize))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Could not detect KV cache dimensions from Cohere decoder metadata, \
                     falling back to (num_heads=16, head_dim=64). Verify against the \
                     actual model before relying on this."
                );
                (16, 64)
            });

        let fbank_extractor = FbankExtractor::new_default();

        tracing::info!(
            "Cohere model loaded in {:.2}s (num_heads={}, head_dim={})",
            start.elapsed().as_secs_f32(),
            num_heads,
            head_dim,
        );

        Ok(Self {
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            tokens,
            decoder_input_names,
            decoder_output_names,
            num_heads,
            head_dim,
            fbank_extractor,
        })
    }

    /// Run encoder + autoregressive decoder, return generated token ids.
    fn run_inference(&self, samples: &[f32]) -> Result<Vec<u32>, TranscribeError> {
        let duration_secs = samples.len() as f32 / SAMPLE_RATE as f32;

        // --- Encoder ---
        let encoder_start = std::time::Instant::now();

        // Log-mel features: shape [num_frames, n_mels]. Cohere ingests log-mel
        // rather than raw audio, which is the main structural difference vs.
        // Moonshine.
        let fbank = self.fbank_extractor.extract(samples);
        if fbank.nrows() == 0 {
            return Err(TranscribeError::AudioFormat(
                "Audio too short for log-mel feature extraction".to_string(),
            ));
        }

        let num_frames = fbank.nrows();
        let n_mels = fbank.ncols();
        let (mel_data, _offset) = fbank.into_raw_vec_and_offset();

        // Encoder input shape [1, num_frames, n_mels]. The actual Cohere export
        // may expect [1, n_mels, num_frames] (transposed); confirm at integration
        // time and transpose here if needed.
        let input_tensor = Tensor::<f32>::from_array(([1usize, num_frames, n_mels], mel_data))
            .map_err(|e| {
                TranscribeError::InferenceFailed(format!(
                    "Failed to create encoder input tensor: {}",
                    e
                ))
            })?;

        let mut encoder = self.encoder.lock().map_err(|e| {
            TranscribeError::InferenceFailed(format!("Failed to lock encoder: {}", e))
        })?;

        let encoder_input_name = encoder
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .unwrap_or_else(|| "input_features".to_string());
        let encoder_output_name = encoder
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .unwrap_or_else(|| "last_hidden_state".to_string());

        let encoder_inputs: Vec<(std::borrow::Cow<str>, ort::session::SessionInputValue)> = vec![(
            std::borrow::Cow::Owned(encoder_input_name),
            input_tensor.into(),
        )];

        let mut encoder_outputs = encoder.run(encoder_inputs).map_err(|e| {
            TranscribeError::InferenceFailed(format!("Cohere encoder inference failed: {}", e))
        })?;

        tracing::debug!(
            "Cohere encoder completed in {:.2}s",
            encoder_start.elapsed().as_secs_f32()
        );

        let encoder_hidden = encoder_outputs
            .remove(&encoder_output_name)
            .ok_or_else(|| {
                TranscribeError::InferenceFailed(format!(
                    "Cohere encoder produced no output named '{}'",
                    encoder_output_name
                ))
            })?;
        drop(encoder_outputs);
        drop(encoder);

        // --- Decoder (autoregressive loop) ---
        let decoder_start = std::time::Instant::now();
        let max_tokens =
            ((duration_secs * MAX_TOKENS_PER_SECOND) as usize).clamp(16, ABSOLUTE_MAX_TOKENS);

        let mut generated_tokens: Vec<i64> = vec![DECODER_START_TOKEN_ID];

        let mut decoder = self.decoder.lock().map_err(|e| {
            TranscribeError::InferenceFailed(format!("Failed to lock decoder: {}", e))
        })?;

        let mut kv_input_names: Vec<&str> = self
            .decoder_input_names
            .iter()
            .filter(|n| n.starts_with("past_key_values"))
            .map(|n| n.as_str())
            .collect();
        kv_input_names.sort();

        let mut kv_output_names: Vec<&str> = self
            .decoder_output_names
            .iter()
            .filter(|n| n.starts_with("present"))
            .map(|n| n.as_str())
            .collect();
        kv_output_names.sort();

        let mut decoder_kv_input_names: Vec<&str> = kv_input_names
            .iter()
            .filter(|n| n.contains(".decoder."))
            .copied()
            .collect();
        decoder_kv_input_names.sort();

        let mut encoder_kv_input_names: Vec<&str> = kv_input_names
            .iter()
            .filter(|n| n.contains(".encoder."))
            .copied()
            .collect();
        encoder_kv_input_names.sort();

        let mut decoder_kv_output_names: Vec<&str> = kv_output_names
            .iter()
            .filter(|n| n.contains(".decoder."))
            .copied()
            .collect();
        decoder_kv_output_names.sort();

        let mut encoder_kv_output_names: Vec<&str> = kv_output_names
            .iter()
            .filter(|n| n.contains(".encoder."))
            .copied()
            .collect();
        encoder_kv_output_names.sort();

        let num_heads = self.num_heads;
        let head_dim = self.head_dim;

        let mut decoder_kv_cache: Vec<ort::value::DynValue> = Vec::new();
        let mut encoder_kv_cache: Vec<ort::value::DynValue> = Vec::new();

        for step in 0..max_tokens {
            let input_ids = if step == 0 {
                Tensor::<i64>::from_array((
                    [1usize, generated_tokens.len()],
                    generated_tokens.clone(),
                ))
            } else {
                Tensor::<i64>::from_array((
                    [1usize, 1usize],
                    vec![*generated_tokens.last().unwrap()],
                ))
            }
            .map_err(|e| {
                TranscribeError::InferenceFailed(format!(
                    "Failed to create input_ids tensor: {}",
                    e
                ))
            })?;

            let mut inputs: Vec<(std::borrow::Cow<str>, ort::session::SessionInputValue)> =
                Vec::new();

            inputs.push((std::borrow::Cow::Borrowed("input_ids"), input_ids.into()));
            inputs.push((
                std::borrow::Cow::Borrowed("encoder_hidden_states"),
                ort::session::SessionInputValue::from(&encoder_hidden),
            ));

            if step == 0 {
                let dummy_size = num_heads * head_dim;
                for kv_name in &decoder_kv_input_names {
                    let dummy_kv = Tensor::<f32>::from_array((
                        [1usize, num_heads, 1usize, head_dim],
                        vec![0.0f32; dummy_size],
                    ))
                    .map_err(|e| {
                        TranscribeError::InferenceFailed(format!(
                            "Failed to create dummy decoder KV tensor: {}",
                            e
                        ))
                    })?;
                    inputs.push((std::borrow::Cow::Borrowed(kv_name), dummy_kv.into()));
                }
            } else {
                for (i, kv_name) in decoder_kv_input_names.iter().enumerate() {
                    inputs.push((
                        std::borrow::Cow::Borrowed(kv_name),
                        ort::session::SessionInputValue::from(&decoder_kv_cache[i]),
                    ));
                }
            }

            if step == 0 {
                let dummy_size = num_heads * head_dim;
                for kv_name in &encoder_kv_input_names {
                    let dummy_kv = Tensor::<f32>::from_array((
                        [1usize, num_heads, 1usize, head_dim],
                        vec![0.0f32; dummy_size],
                    ))
                    .map_err(|e| {
                        TranscribeError::InferenceFailed(format!(
                            "Failed to create dummy encoder KV tensor: {}",
                            e
                        ))
                    })?;
                    inputs.push((std::borrow::Cow::Borrowed(kv_name), dummy_kv.into()));
                }
            } else {
                for (i, kv_name) in encoder_kv_input_names.iter().enumerate() {
                    inputs.push((
                        std::borrow::Cow::Borrowed(kv_name),
                        ort::session::SessionInputValue::from(&encoder_kv_cache[i]),
                    ));
                }
            }

            let use_cache = Tensor::<bool>::from_array(([1], vec![step > 0])).map_err(|e| {
                TranscribeError::InferenceFailed(format!(
                    "Failed to create use_cache tensor: {}",
                    e
                ))
            })?;
            inputs.push((
                std::borrow::Cow::Borrowed("use_cache_branch"),
                use_cache.into(),
            ));

            let mut outputs = decoder.run(inputs).map_err(|e| {
                TranscribeError::InferenceFailed(format!(
                    "Cohere decoder inference failed at step {}: {}",
                    step, e
                ))
            })?;

            let logits_val = &outputs["logits"];
            let (shape, logits_data) = logits_val.try_extract_tensor::<f32>().map_err(|e| {
                TranscribeError::InferenceFailed(format!("Failed to extract logits: {}", e))
            })?;

            let shape_dims: &[i64] = shape;
            let vocab_logits: &[f32] = if shape_dims.len() == 3 {
                let vocab_size = shape_dims[2] as usize;
                let seq_len = shape_dims[1] as usize;
                let offset = (seq_len - 1) * vocab_size;
                &logits_data[offset..offset + vocab_size]
            } else if shape_dims.len() == 2 {
                let vocab_size = shape_dims[1] as usize;
                &logits_data[..vocab_size]
            } else {
                return Err(TranscribeError::InferenceFailed(format!(
                    "Unexpected Cohere logits shape: {:?}",
                    shape_dims
                )));
            };

            let next_token = vocab_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx as i64)
                .ok_or_else(|| {
                    TranscribeError::InferenceFailed("Empty Cohere logits vector".to_string())
                })?;

            if next_token == EOS_TOKEN_ID {
                tracing::debug!("Cohere decoder reached EOS at step {}", step);
                break;
            }

            generated_tokens.push(next_token);

            let mut new_decoder_cache = Vec::new();
            for kv_out_name in &decoder_kv_output_names {
                if let Some(value) = outputs.remove(kv_out_name) {
                    new_decoder_cache.push(value);
                }
            }
            decoder_kv_cache = new_decoder_cache;

            if step == 0 {
                for kv_out_name in &encoder_kv_output_names {
                    if let Some(value) = outputs.remove(kv_out_name) {
                        encoder_kv_cache.push(value);
                    }
                }
            }
        }

        tracing::debug!(
            "Cohere decoder completed in {:.2}s ({} tokens)",
            decoder_start.elapsed().as_secs_f32(),
            generated_tokens.len() - 1
        );

        let token_ids: Vec<u32> = generated_tokens.iter().skip(1).map(|&t| t as u32).collect();
        Ok(token_ids)
    }

    /// Decode token ids to text. The Cohere tokenizer is SentencePiece, so we
    /// concatenate pieces and convert the U+2581 word-boundary marker to a
    /// regular space.
    fn decode_tokens(&self, token_ids: &[u32]) -> String {
        let mut result = String::new();
        for &id in token_ids {
            if let Some(piece) = self.tokens.get(&id) {
                result.push_str(&piece.replace('\u{2581}', " "));
            }
        }
        result.trim().to_string()
    }
}

impl Transcriber for CohereTranscriber {
    fn transcribe(&self, samples: &[f32]) -> Result<String, TranscribeError> {
        if samples.is_empty() {
            return Err(TranscribeError::AudioFormat(
                "Empty audio buffer".to_string(),
            ));
        }

        let duration_secs = samples.len() as f32 / SAMPLE_RATE as f32;
        tracing::debug!(
            "Transcribing {:.2}s of audio ({} samples) with Cohere",
            duration_secs,
            samples.len(),
        );

        let start = std::time::Instant::now();
        let token_ids = self.run_inference(samples)?;
        let text = self.decode_tokens(&token_ids).trim().to_string();

        tracing::info!(
            "Cohere transcription completed in {:.2}s: {:?}",
            start.elapsed().as_secs_f32(),
            if text.chars().count() > 50 {
                format!("{}...", text.chars().take(50).collect::<String>())
            } else {
                text.clone()
            }
        );

        Ok(text)
    }
}

/// Resolve a model name or path to a directory containing the Cohere ONNX files.
#[allow(dead_code)]
fn resolve_model_path(model: &str) -> Result<PathBuf, TranscribeError> {
    let path = PathBuf::from(model);
    if path.is_absolute() && path.exists() {
        return Ok(path);
    }

    let models_dir = crate::config::Config::models_dir();
    let candidate = models_dir.join(model);
    if candidate.exists() {
        return Ok(candidate);
    }

    let local = PathBuf::from("models").join(model);
    if local.exists() {
        return Ok(local);
    }

    Err(TranscribeError::ModelNotFound(format!(
        "Cohere model '{}' not found. Looked in:\n  - {}\n  - {}\n  - {}",
        model,
        path.display(),
        candidate.display(),
        local.display(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolve fixtures dir (matches the convention used by other integration tests).
    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// Load a 16 kHz mono WAV into f32 samples in [-1, 1].
    fn load_wav(path: &Path) -> Vec<f32> {
        let reader = hound::WavReader::open(path)
            .unwrap_or_else(|e| panic!("Failed to open {}: {}", path.display(), e));
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000, "Expected 16 kHz audio");
        assert_eq!(spec.channels, 1, "Expected mono audio");

        let max_val = (1i64 << (spec.bits_per_sample - 1)) as f32;
        reader
            .into_samples::<i32>()
            .filter_map(|s| s.ok())
            .map(|s| s as f32 / max_val)
            .collect()
    }

    /// End-to-end PoC: load the int8 Cohere model and transcribe a fixture WAV.
    ///
    /// This test is `#[ignore]`d by default because it requires a ~2.4 GB model
    /// download. To run it:
    ///
    /// 1. Download the model into `models/cohere-transcribe-int8/` (see the
    ///    module-level docs at the top of this file for `curl` commands).
    /// 2. `cargo test --features cohere transcribe::cohere::tests::cohere_poc \
    ///                -- --ignored --nocapture`
    ///
    /// The test passes if loading + inference completes without panicking. The
    /// transcribed text is printed for manual inspection; assertion of exact
    /// content is left for a future, model-verified shippable PR.
    #[test]
    #[ignore]
    fn cohere_poc() {
        // Honor a env override so devs can point at a local checkout.
        let model_dir = std::env::var("VOXTYPE_COHERE_MODEL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("models")
                    .join("cohere-transcribe-int8")
            });

        assert!(
            model_dir.exists(),
            "Cohere model dir not found at {}. See module docs for download instructions.",
            model_dir.display()
        );

        let transcriber =
            CohereTranscriber::from_dir(&model_dir).expect("Failed to load Cohere transcriber");

        let wav_path = fixtures_dir().join("vad").join("speech_hello.wav");
        let samples = load_wav(&wav_path);
        assert!(
            !samples.is_empty(),
            "Loaded zero samples from {}",
            wav_path.display()
        );

        let text = transcriber
            .transcribe(&samples)
            .expect("Cohere transcription failed");
        eprintln!("Cohere PoC transcription: {:?}", text);
    }

    #[test]
    fn resolve_model_path_not_found() {
        let result = resolve_model_path("/nonexistent/cohere/path");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TranscribeError::ModelNotFound(_)
        ));
    }
}
