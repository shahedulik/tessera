use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::moondream::{Config as MoonDreamConfig, Model as MoonDreamModel};
use dashmap::DashMap;
use image::GenericImageView;
use pdfium_render::prelude::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokenizers::Tokenizer;

use crate::candidates::{build_candidates, PageAnalysis};
use crate::envelope::{envelope_check, format_envelope};
use crate::error::VisionError;
use crate::pdf_vein::TextSpan;

const GHOST_CACHE_MAX_ENTRIES: usize = 200;
const MAX_ANSWER_TOKENS: usize = 96;
const END_MARKER_TOKENS: [u32; 3] = [27, 10619, 29];

pub const DEFAULT_MODEL_DIR: &str = "D:\\tessera\\models\\moondream2";
pub const DEFAULT_SEED: u64 = 20260916;

pub fn cuda_available() -> bool {
    Device::cuda_if_available(0)
        .map(|device| device.is_cuda())
        .unwrap_or(false)
}

fn preprocess_page_image(
    path: &Path,
    device: &Device,
    dtype: DType,
) -> Result<(Tensor, u32, u32), VisionError> {
    let img = image::ImageReader::open(path)
        .map_err(|e| VisionError::Image(e.to_string()))?
        .decode()
        .map_err(|e| VisionError::Image(e.to_string()))?;
    let (src_w, src_h) = img.dimensions();
    let img = img
        .resize_to_fill(378, 378, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let data = img.into_raw();
    let tensor = Tensor::from_vec(data, (378, 378, 3), &Device::Cpu)
        .map_err(|e| VisionError::Candle(e.to_string()))?
        .permute((2, 0, 1))
        .map_err(|e| VisionError::Candle(e.to_string()))?;
    let mean = Tensor::new(&[0.5f32, 0.5, 0.5], &Device::Cpu)
        .map_err(|e| VisionError::Candle(e.to_string()))?
        .reshape((3, 1, 1))
        .map_err(|e| VisionError::Candle(e.to_string()))?;
    let std = Tensor::new(&[0.5f32, 0.5, 0.5], &Device::Cpu)
        .map_err(|e| VisionError::Candle(e.to_string()))?
        .reshape((3, 1, 1))
        .map_err(|e| VisionError::Candle(e.to_string()))?;
    let normalized = (tensor
        .to_dtype(DType::F32)
        .map_err(|e| VisionError::Candle(e.to_string()))?
        / 255.)
    .map_err(|e| VisionError::Candle(e.to_string()))?
    .broadcast_sub(&mean)
    .map_err(|e| VisionError::Candle(e.to_string()))?
    .broadcast_div(&std)
    .map_err(|e| VisionError::Candle(e.to_string()))?;
    Ok((
        normalized
            .to_device(device)
            .map_err(|e| VisionError::Candle(e.to_string()))?
            .to_dtype(dtype)
            .map_err(|e| VisionError::Candle(e.to_string()))?,
        src_w,
        src_h,
    ))
}

pub struct VisionEngine {
    device: Device,
    pdfium: Option<Pdfium>,
    model: Option<MoonDreamModel>,
    tokenizer: Option<Tokenizer>,
    ghost_cache: Arc<DashMap<String, Tensor>>,
    vram_allocated_mb: u32,
    seed: u64,
}

impl VisionEngine {
    pub fn new() -> Self {
        let device = Device::cuda_if_available(0).unwrap_or(Device::Cpu);
        let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| Pdfium::bind_to_system_library())
            .expect("FATAL: pdfium.dll not found beside executable or on Windows PATH");
        Self {
            device,
            pdfium: Some(Pdfium::new(bindings)),
            model: None,
            tokenizer: None,
            ghost_cache: Arc::new(DashMap::new()),
            vram_allocated_mb: 0,
            seed: DEFAULT_SEED,
        }
    }

    pub fn open_vlm(model_dir: &Path, seed: u64, require_cuda: bool) -> Result<Self, VisionError> {
        let device = Device::cuda_if_available(0).map_err(|e| VisionError::Candle(e.to_string()))?;
        if require_cuda && !device.is_cuda() {
            return Err(VisionError::CudaRequired(
                "no CUDA device visible to candle; VLM inference is GPU-gated".into(),
            ));
        }
        let weights = model_dir.join("model.safetensors");
        let tokenizer_path = model_dir.join("tokenizer.json");
        for required in [&weights, &tokenizer_path] {
            if !required.exists() {
                return Err(VisionError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("missing {}", required.display()),
                )));
            }
        }
        let target_f16 = device.is_cuda();
        let report = envelope_check(&weights, target_f16)?;
        println!("[VDU] VRAM envelope: {}", format_envelope(&report));
        let dtype = if target_f16 { DType::F16 } else { DType::F32 };
        // SAFETY: from_mmaped_safetensors is candle's canonical read-only mmap loader
        // (official candle moondream example pattern). The file is hash-gated before use
        // (scripts/download_model.ps1 + PHASE1_DNA_MANIFEST.sha256); no mutation occurs.
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], dtype, &device) }
            .map_err(|e| VisionError::Candle(e.to_string()))?;
        let model = MoonDreamModel::new(&MoonDreamConfig::v2(), vb)
            .map_err(|e| VisionError::Candle(e.to_string()))?;
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| VisionError::Tokenizer(e.to_string()))?;
        println!(
            "[VDU] Moondream2 loaded | dtype={} | device={} | seed={seed}",
            if target_f16 { "f16" } else { "f32" },
            if device.is_cuda() { "cuda:0" } else { "cpu" }
        );
        Ok(Self {
            device,
            pdfium: None,
            model: Some(model),
            tokenizer: Some(tokenizer),
            ghost_cache: Arc::new(DashMap::new()),
            vram_allocated_mb: report.weights_mb.round() as u32,
            seed,
        })
    }

    pub fn weights_mb(&self) -> u32 {
        self.vram_allocated_mb
    }

    pub fn ingest_and_rasterize(&self, path: &Path) -> Result<Tensor, Box<dyn std::error::Error>> {
        let pdfium = self.pdfium.as_ref().ok_or_else(|| {
            VisionError::Pdfium("engine opened in VLM mode without pdfium".into())
        })?;
        let doc = pdfium.load_pdf_from_file(path, None)?;
        let page = doc.pages().first()?;
        let config = PdfRenderConfig::new()
            .set_target_width(1024)
            .set_maximum_height(1024)
            .render_form_data(true)
            .render_annotations(true);
        let bitmap = page.render_with_config(&config)?;
        let rgb_image = bitmap.as_image().to_rgb8();
        let (width, height) = rgb_image.dimensions();
        let raw_pixels = rgb_image.into_raw();
        let cpu_tensor =
            Tensor::from_vec(raw_pixels, (height as usize, width as usize, 3), &Device::Cpu)?;
        let normalized = (cpu_tensor.to_dtype(DType::F32)? / 255.0)?;
        let gpu_tensor = normalized.to_device(&self.device)?;
        println!(
            "[VDU] Rasterized: {:?} | Shape: {:?}",
            path.file_name(),
            gpu_tensor.shape()
        );
        Ok(gpu_tensor)
    }

    pub fn compute_layout_hash(&self, raw_bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(raw_bytes);
        hex::encode(hasher.finalize())
    }

    pub fn check_ghost_cache(&self, raw_bytes: &[u8]) -> Option<Tensor> {
        let hash = self.compute_layout_hash(raw_bytes);
        self.ghost_cache.get(&hash).map(|e| e.value().clone())
    }

    pub fn enforce_cache_bounds(&self) {
        if self.ghost_cache.len() > GHOST_CACHE_MAX_ENTRIES
            && let Some(key) = self.ghost_cache.iter().next().map(|e| e.key().clone())
        {
            self.ghost_cache.remove(&key);
        }
    }

    pub fn analyze_page(
        &mut self,
        png: &Path,
        spans: &[TextSpan],
        question: &str,
        page_w_pts: f32,
        page_h_pts: f32,
    ) -> Result<PageAnalysis, VisionError> {
        let dtype = if self.device.is_cuda() {
            DType::F16
        } else {
            DType::F32
        };
        let (image_tensor, src_w, src_h) = preprocess_page_image(png, &self.device, dtype)?;
        let image_embeds = {
            let model = self.model.as_ref().ok_or(VisionError::ModelNotLoaded)?;
            image_tensor
                .unsqueeze(0)
                .map_err(|e| VisionError::Candle(e.to_string()))?
                .apply(model.vision_encoder())
                .map_err(|e| VisionError::Candle(e.to_string()))?
        };
        let prompt = format!("\n\nQuestion: {question}\n\nAnswer:");
        let answer = self.generate_answer(&prompt, &image_embeds)?;
        let candidates = build_candidates(&answer, spans, page_w_pts, page_h_pts);
        Ok(PageAnalysis {
            png_file: png.to_string_lossy().into_owned(),
            src_width_px: src_w,
            src_height_px: src_h,
            answer,
            candidates,
        })
    }

    fn generate_answer(
        &mut self,
        prompt: &str,
        image_embeds: &Tensor,
    ) -> Result<String, VisionError> {
        let candle_err = |e: candle_core::Error| VisionError::Candle(e.to_string());
        let tokenizer = self
            .tokenizer
            .as_ref()
            .ok_or(VisionError::ModelNotLoaded)?
            .clone();
        let encoding = tokenizer
            .encode(prompt, true)
            .map_err(|e| VisionError::Tokenizer(e.to_string()))?;
        let mut tokens = encoding.get_ids().to_vec();
        if tokens.is_empty() {
            return Err(VisionError::Tokenizer("empty prompt encoding".into()));
        }
        let special = *tokenizer
            .get_vocab(true)
            .get("<|file_sep|>")
            .ok_or_else(|| VisionError::Tokenizer("missing <|file_sep|> in vocab".into()))?;
        let (bos, eos) = (special, special);
        let mut logits_processor = LogitsProcessor::new(self.seed, None, None);
        let mut output_ids: Vec<u32> = Vec::new();
        let model = self.model.as_mut().ok_or(VisionError::ModelNotLoaded)?;
        model.text_model.clear_kv_cache();
        for index in 0..MAX_ANSWER_TOKENS {
            let context_size = if index > 0 { 1 } else { tokens.len() };
            let ctxt = &tokens[tokens.len().saturating_sub(context_size)..];
            let input = Tensor::new(ctxt, &self.device)
                .map_err(candle_err)?
                .unsqueeze(0)
                .map_err(candle_err)?;
            let logits = if index > 0 {
                model.text_model.forward(&input).map_err(candle_err)?
            } else {
                let bos_tensor = Tensor::new(&[bos], &self.device)
                    .map_err(candle_err)?
                    .unsqueeze(0)
                    .map_err(candle_err)?;
                model
                    .text_model
                    .forward_with_img(&bos_tensor, &input, image_embeds)
                    .map_err(candle_err)?
            };
            let logits = logits
                .squeeze(0)
                .map_err(candle_err)?
                .to_dtype(DType::F32)
                .map_err(candle_err)?;
            let next = logits_processor.sample(&logits).map_err(candle_err)?;
            tokens.push(next);
            output_ids.push(next);
            if next == eos || tokens.ends_with(&END_MARKER_TOKENS) {
                break;
            }
        }
        let answer = tokenizer
            .decode(&output_ids, true)
            .map_err(|e| VisionError::Tokenizer(e.to_string()))?;
        Ok(answer.replace("<END>", " ").trim().to_string())
    }
}

impl Default for VisionEngine {
    fn default() -> Self {
        Self::new()
    }
}
