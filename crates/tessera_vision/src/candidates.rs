use crate::error::VisionError;
use crate::pdf_vein::{json_escape, TextSpan};
use std::path::Path;

pub const ANOMALY_KEYWORDS: [&str; 10] = [
    "stamp",
    "seal",
    "overlay",
    "altered",
    "tamper",
    "mismatch",
    "font",
    "hidden",
    "forged",
    "inconsisten",
];
pub const NO_ANOMALY_MARKERS: [&str; 7] = [
    "no anomal",
    "no visual anomal",
    "no signs of",
    "appears clean",
    "looks normal",
    "no evidence of",
    "cannot detect",
];

#[derive(Debug, Clone)]
pub struct AnomalyCandidate {
    pub bbox: [f32; 4],
    pub description: String,
    pub confidence: f32,
    pub grounding: String,
}

#[derive(Debug, Clone)]
pub struct PageAnalysis {
    pub png_file: String,
    pub src_width_px: u32,
    pub src_height_px: u32,
    pub answer: String,
    pub candidates: Vec<AnomalyCandidate>,
}

impl PageAnalysis {
    pub fn to_jsonl_line(&self) -> String {
        let candidates: Vec<String> = self
            .candidates
            .iter()
            .map(|c| {
                format!(
                    "{{\"bbox\": [{:.4}, {:.4}, {:.4}, {:.4}], \"description\": \"{}\", \"confidence\": {:.2}, \"grounding\": \"{}\"}}",
                    c.bbox[0],
                    c.bbox[1],
                    c.bbox[2],
                    c.bbox[3],
                    json_escape(&c.description),
                    c.confidence,
                    json_escape(&c.grounding)
                )
            })
            .collect();
        format!(
            "{{\"png_file\": \"{}\", \"src_width_px\": {}, \"src_height_px\": {}, \"model\": \"moondream2-f16\", \"answer\": \"{}\", \"candidates\": [{}]}}",
            json_escape(&self.png_file),
            self.src_width_px,
            self.src_height_px,
            json_escape(&self.answer),
            candidates.join(", ")
        )
    }
}

pub fn validate_page_image(path: &Path) -> Result<(u32, u32), VisionError> {
    let img = image::open(path).map_err(|e| VisionError::Image(e.to_string()))?;
    use image::GenericImageView as _;
    Ok(img.dimensions())
}

fn strip_leading_numbering(line: &str) -> &str {
    let trimmed = line.trim_start_matches(|c: char| c.is_ascii_digit());
    let stripped = trimmed
        .strip_prefix(". ")
        .or_else(|| trimmed.strip_prefix(") "));
    stripped.map(str::trim_start).unwrap_or(line)
}

pub fn build_candidates(
    answer: &str,
    spans: &[TextSpan],
    page_w_pts: f32,
    page_h_pts: f32,
) -> Vec<AnomalyCandidate> {
    let normalized = answer.replace("<END>", " ");
    let lower_all = normalized.to_lowercase();
    let mut candidates = Vec::new();
    for raw_line in normalized.lines() {
        let line = raw_line
            .trim()
            .trim_start_matches(['-', '*', '\u{2022}'])
            .trim();
        let line = strip_leading_numbering(line).trim();
        if line.is_empty() {
            continue;
        }
        let line_lower = line.to_lowercase();
        if NO_ANOMALY_MARKERS.iter().any(|m| line_lower.contains(m)) {
            continue;
        }
        let keyword_hits = ANOMALY_KEYWORDS
            .iter()
            .filter(|k| line_lower.contains(**k))
            .count();
        let mut confidence = 0.5f32 + 0.1 * keyword_hits as f32;
        let mut bbox = [0.0f32, 0.0, page_w_pts, page_h_pts];
        let mut grounding = "page";
        let mut best: Option<&TextSpan> = None;
        let mut best_len = 0usize;
        for span in spans {
            let text = span.text.trim();
            if text.len() >= 8 && text.len() > best_len && line_lower.contains(&text.to_lowercase())
            {
                best = Some(span);
                best_len = text.len();
            }
        }
        if let Some(span) = best {
            bbox = span.bbox;
            grounding = "span-match";
            confidence += 0.3;
        }
        confidence = confidence.min(0.95);
        candidates.push(AnomalyCandidate {
            bbox,
            description: line.to_string(),
            confidence,
            grounding: grounding.to_string(),
        });
    }
    if candidates.is_empty()
        && ANOMALY_KEYWORDS.iter().any(|k| lower_all.contains(k))
        && !NO_ANOMALY_MARKERS.iter().any(|m| lower_all.contains(m))
    {
        let keyword_hits = ANOMALY_KEYWORDS
            .iter()
            .filter(|k| lower_all.contains(**k))
            .count();
        candidates.push(AnomalyCandidate {
            bbox: [0.0, 0.0, page_w_pts, page_h_pts],
            description: normalized.trim().to_string(),
            confidence: (0.5 + 0.1 * keyword_hits as f32).min(0.95),
            grounding: "page".to_string(),
        });
    }
    candidates
}
