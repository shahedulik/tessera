pub const VRAM_LIMIT_MB: u32 = 8192;
pub const WEIGHTS_ALLOC_MB: u32 = 4608;
pub const KV_CACHE_ALLOC_MB: u32 = 2560;
pub const OVERHEAD_ALLOC_MB: u32 = 1024;

pub mod envelope;
pub mod error;

#[cfg(any(feature = "pdf", feature = "model"))]
pub mod candidates;

#[cfg(any(feature = "pdf", feature = "model"))]
pub mod pdf_vein;

#[cfg(feature = "model")]
pub mod anomaly;

#[cfg(feature = "model")]
pub mod engine;
