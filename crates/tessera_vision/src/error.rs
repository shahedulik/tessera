use std::fmt;

#[derive(Debug)]
pub enum VisionError {
    Io(std::io::Error),
    Candle(String),
    Tokenizer(String),
    Image(String),
    Pdfium(String),
    HeaderMalformed(String),
    ModelTooLarge { estimated_mb: f64, limit_mb: u32 },
    CudaRequired(String),
    ModelNotLoaded,
}

impl fmt::Display for VisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Candle(m) => write!(f, "candle error: {m}"),
            Self::Tokenizer(m) => write!(f, "tokenizer error: {m}"),
            Self::Image(m) => write!(f, "image error: {m}"),
            Self::Pdfium(m) => write!(f, "pdfium error: {m}"),
            Self::HeaderMalformed(m) => write!(f, "safetensors header malformed: {m}"),
            Self::ModelTooLarge {
                estimated_mb,
                limit_mb,
            } => write!(
                f,
                "MODEL REJECTED: estimated weights {estimated_mb:.1}MB exceed {limit_mb}MB envelope"
            ),
            Self::CudaRequired(m) => write!(f, "CUDA required: {m}"),
            Self::ModelNotLoaded => write!(f, "VLM not loaded (open_vlm first)"),
        }
    }
}

impl std::error::Error for VisionError {}

impl From<std::io::Error> for VisionError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
