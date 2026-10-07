use crate::error::VisionError;
use crate::{KV_CACHE_ALLOC_MB, OVERHEAD_ALLOC_MB, VRAM_LIMIT_MB, WEIGHTS_ALLOC_MB};
use std::fs;
use std::io::Read;
use std::path::Path;

const MAX_HEADER_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct EnvelopeReport {
    pub params: u64,
    pub weights_mb: f64,
    pub kv_mb: f64,
    pub overhead_mb: f64,
    pub limit_mb: f64,
    pub target_f16: bool,
}

pub fn read_safetensors_header(path: &Path) -> Result<String, VisionError> {
    let mut file = fs::File::open(path)?;
    let mut len_bytes = [0u8; 8];
    file.read_exact(&mut len_bytes)?;
    let len = u64::from_le_bytes(len_bytes);
    if len == 0 || len > MAX_HEADER_BYTES {
        return Err(VisionError::HeaderMalformed(format!(
            "header length {len} outside (0, {MAX_HEADER_BYTES}]"
        )));
    }
    let mut buf = vec![0u8; len as usize];
    file.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| VisionError::HeaderMalformed("header is not UTF-8".into()))
}

pub fn estimate_from_header(header: &str, target_f16: bool) -> Result<(u64, f64), VisionError> {
    let mut params: u64 = 0;
    let mut bytes: f64 = 0.0;
    let fragments = header.split("\"dtype\"");
    for fragment in fragments.skip(1) {
        let dtype = fragment
            .trim_start_matches(|c: char| c != '"')
            .trim_start_matches('"');
        let dtype = dtype
            .split('"')
            .next()
            .ok_or_else(|| VisionError::HeaderMalformed("unterminated dtype".into()))?;
        let per_param: f64 = match dtype {
            "F64" | "F32" | "F16" | "BF16" => {
                if target_f16 {
                    2.0
                } else {
                    4.0
                }
            }
            "I64" => 8.0,
            "I32" => 4.0,
            "I16" => 2.0,
            "I8" | "U8" | "BOOL" | "F8_E4M3" => 1.0,
            other => {
                return Err(VisionError::HeaderMalformed(format!(
                    "unknown dtype {other}"
                )))
            }
        };
        let shape_at = fragment
            .find("\"shape\"")
            .ok_or_else(|| VisionError::HeaderMalformed("dtype without shape".into()))?;
        let after = &fragment[shape_at..];
        let open = after
            .find('[')
            .ok_or_else(|| VisionError::HeaderMalformed("shape without [".into()))?;
        let close = after[open..]
            .find(']')
            .ok_or_else(|| VisionError::HeaderMalformed("shape without ]".into()))?;
        let inner = &after[open + 1..open + close];
        let mut count: u64 = 1;
        if !inner.trim().is_empty() {
            for part in inner.split(',') {
                let dim: u64 = part
                    .trim()
                    .parse()
                    .map_err(|_| VisionError::HeaderMalformed(format!("bad shape dim {part}")))?;
                count = count.saturating_mul(dim);
            }
        }
        params = params.saturating_add(count);
        bytes += count as f64 * per_param;
    }
    if params == 0 {
        return Err(VisionError::HeaderMalformed("no tensors found".into()));
    }
    Ok((params, bytes / (1024.0 * 1024.0)))
}

pub fn envelope_check(safetensors: &Path, target_f16: bool) -> Result<EnvelopeReport, VisionError> {
    let header = read_safetensors_header(safetensors)?;
    let (params, weights_mb) = estimate_from_header(&header, target_f16)?;
    let report = EnvelopeReport {
        params,
        weights_mb,
        kv_mb: f64::from(KV_CACHE_ALLOC_MB),
        overhead_mb: f64::from(OVERHEAD_ALLOC_MB),
        limit_mb: f64::from(VRAM_LIMIT_MB),
        target_f16,
    };
    if weights_mb > f64::from(WEIGHTS_ALLOC_MB) {
        return Err(VisionError::ModelTooLarge {
            estimated_mb: weights_mb,
            limit_mb: WEIGHTS_ALLOC_MB,
        });
    }
    if weights_mb + report.kv_mb + report.overhead_mb > report.limit_mb {
        return Err(VisionError::ModelTooLarge {
            estimated_mb: weights_mb,
            limit_mb: WEIGHTS_ALLOC_MB,
        });
    }
    Ok(report)
}

pub fn format_envelope(report: &EnvelopeReport) -> String {
    format!(
        "params={} | weights={:.1}MB ({}) <= {}MB budget | + kv={}MB + overhead={}MB = {:.1}MB / {}MB limit | headroom={:.1}MB | PASS",
        report.params,
        report.weights_mb,
        if report.target_f16 { "f16" } else { "f32" },
        WEIGHTS_ALLOC_MB,
        KV_CACHE_ALLOC_MB,
        OVERHEAD_ALLOC_MB,
        report.weights_mb + report.kv_mb + report.overhead_mb,
        VRAM_LIMIT_MB,
        report.limit_mb - (report.weights_mb + report.kv_mb + report.overhead_mb)
    )
}
