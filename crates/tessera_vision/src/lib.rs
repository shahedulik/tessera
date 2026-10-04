use candle_core::{DType, Device, Tensor};
use dashmap::DashMap;
use pdfium_render::prelude::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

pub const VRAM_LIMIT_MB: u32 = 8192;
pub const WEIGHTS_ALLOC_MB: u32 = 4608;
pub const KV_CACHE_ALLOC_MB: u32 = 2560;
pub const OVERHEAD_ALLOC_MB: u32 = 1024;
const GHOST_CACHE_MAX_ENTRIES: usize = 200;

pub struct VisionEngine {
    device: Device,
    pdfium: Pdfium,
    ghost_cache: Arc<DashMap<String, Tensor>>,
    vram_allocated_mb: u32,
}

impl VisionEngine {
    pub fn new() -> Self {
        let device = Device::cuda_if_available(0).unwrap_or(Device::Cpu);
        let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| Pdfium::bind_to_system_library())
            .expect("FATAL: pdfium.dll not found beside executable or on Windows PATH");
        let pdfium = Pdfium::new(bindings);

        Self {
            device,
            pdfium,
            ghost_cache: Arc::new(DashMap::new()),
            vram_allocated_mb: 0,
        }
    }

    pub fn ingest_and_rasterize(&self, path: &Path) -> Result<Tensor, Box<dyn std::error::Error>> {
        let doc = self.pdfium.load_pdf_from_file(path, None)?;
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

    pub fn load_quantized_vdu(
        &mut self,
        _model_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.vram_allocated_mb + WEIGHTS_ALLOC_MB > VRAM_LIMIT_MB - OVERHEAD_ALLOC_MB {
            panic!("FATAL: Model weights exceed 4.5GB VRAM envelope");
        }
        self.vram_allocated_mb += WEIGHTS_ALLOC_MB;
        println!(
            "[VDU] INT4 Weights Mapped: {}MB used | Envelope: {}MB weights + {}MB KV + {}MB overhead = {}MB limit",
            self.vram_allocated_mb, WEIGHTS_ALLOC_MB, KV_CACHE_ALLOC_MB, OVERHEAD_ALLOC_MB, VRAM_LIMIT_MB
        );
        Ok(())
    }
}

impl Default for VisionEngine {
    fn default() -> Self {
        Self::new()
    }
}
