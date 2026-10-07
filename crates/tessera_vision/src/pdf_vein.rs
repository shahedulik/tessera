use image::ImageFormat;
use pdfium_render::prelude::*;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const DEFAULT_OUTPUT_ROOT: &str = "D:\\tessera\\evidence\\pdf_out";
pub const DEFAULT_DPI: u32 = 200;
pub const MAX_PAGE_COUNT: usize = 65_535;
pub const SCHEMA_VERSION: &str = "tessera.pdf_vein/1";
pub const TOOL_ID: &str = "TESSERA pdf_vein (P2A, CPU-only, deterministic)";

#[derive(Debug, Clone)]
pub struct PdfVeinOptions {
    pub source: PathBuf,
    pub output_root: PathBuf,
    pub dpi: u32,
    pub dry_run: bool,
    pub allow_unsafe_output_root: bool,
}

impl Default for PdfVeinOptions {
    fn default() -> Self {
        Self {
            source: PathBuf::new(),
            output_root: PathBuf::from(DEFAULT_OUTPUT_ROOT),
            dpi: DEFAULT_DPI,
            dry_run: false,
            allow_unsafe_output_root: false,
        }
    }
}

#[derive(Debug)]
pub enum PdfVeinError {
    Io(std::io::Error),
    Pdfium(String),
    Image(String),
    OutputRootViolation { root: String },
    PageCountExcessive { count: usize },
    InvalidPageGeometry { page_no: usize },
    EmptyDocument,
    MissingSource,
}

impl fmt::Display for PdfVeinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Pdfium(m) => write!(f, "pdfium error: {m}"),
            Self::Image(m) => write!(f, "image encode error: {m}"),
            Self::OutputRootViolation { root } => {
                write!(f, "output root violation: {root} is not on D: (zero C: writes law)")
            }
            Self::PageCountExcessive { count } => {
                write!(f, "page count {count} exceeds MAX_PAGE_COUNT {MAX_PAGE_COUNT}")
            }
            Self::InvalidPageGeometry { page_no } => {
                write!(f, "page {page_no} reports non-finite or non-positive geometry")
            }
            Self::EmptyDocument => {
                write!(f, "PDF contains zero pages (forensically empty document)")
            }
            Self::MissingSource => write!(f, "no source PDF path supplied"),
        }
    }
}

impl std::error::Error for PdfVeinError {}

impl From<std::io::Error> for PdfVeinError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Clone)]
pub struct TextSpan {
    pub text: String,
    pub bbox: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct PageEvidence {
    pub page_no: usize,
    pub page_sha256: String,
    pub image_sha256: String,
    pub png_file: String,
    pub width_pts: f32,
    pub height_pts: f32,
    pub target_width_px: i32,
    pub width_px: i32,
    pub height_px: i32,
    pub spans: Vec<TextSpan>,
}

#[derive(Debug, Clone)]
pub struct PdfScanReport {
    pub source_path: String,
    pub pdf_sha256: String,
    pub size_bytes: u64,
    pub page_count: usize,
    pub dpi: u32,
    pub dry_run: bool,
    pub output_dir: String,
    pub pages: Vec<PageEvidence>,
}

pub fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn short(hash: &str) -> &str {
    hash.get(..16).unwrap_or(hash)
}

pub fn enforce_output_root(root: &Path, allow_unsafe: bool) -> Result<(), PdfVeinError> {
    if allow_unsafe {
        return Ok(());
    }
    let upper = root.to_string_lossy().to_uppercase();
    if upper.starts_with("D:\\") || upper.starts_with("D:/") {
        return Ok(());
    }
    Err(PdfVeinError::OutputRootViolation {
        root: root.to_string_lossy().into_owned(),
    })
}

fn bind_pdfium() -> Result<Pdfium, PdfVeinError> {
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| PdfVeinError::Pdfium(format!("bind failed: {e:?}")))?;
    Ok(Pdfium::new(bindings))
}

fn extract_spans(page: &PdfPage) -> Result<Vec<TextSpan>, PdfVeinError> {
    let text = page
        .text()
        .map_err(|e| PdfVeinError::Pdfium(format!("text extraction failed: {e:?}")))?;
    let mut spans = Vec::new();
    for segment in text.segments().iter() {
        let rect = segment.bounds();
        let left = rect.left().value;
        let right = rect.right().value;
        let top = rect.top().value;
        let bottom = rect.bottom().value;
        spans.push(TextSpan {
            text: segment.text(),
            bbox: [left.min(right), top.min(bottom), left.max(right), top.max(bottom)],
        });
    }
    Ok(spans)
}

pub fn scan_pdf(options: &PdfVeinOptions) -> Result<PdfScanReport, PdfVeinError> {
    if options.source.as_os_str().is_empty() {
        return Err(PdfVeinError::MissingSource);
    }
    let bytes = fs::read(&options.source)?;
    let pdf_sha256 = sha256_hex(&bytes);
    let size_bytes = bytes.len() as u64;
    let prefix = pdf_sha256.get(..16).unwrap_or(&pdf_sha256).to_string();

    let pdfium = bind_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(&options.source, None)
        .map_err(|e| PdfVeinError::Pdfium(format!("open failed (corrupt or encrypted?): {e:?}")))?;
    let page_count = document.pages().len() as usize;
    if page_count > MAX_PAGE_COUNT {
        return Err(PdfVeinError::PageCountExcessive { count: page_count });
    }
    if page_count == 0 {
        return Err(PdfVeinError::EmptyDocument);
    }

    let mut report = PdfScanReport {
        source_path: options.source.to_string_lossy().into_owned(),
        pdf_sha256,
        size_bytes,
        page_count,
        dpi: options.dpi,
        dry_run: options.dry_run,
        output_dir: String::new(),
        pages: Vec::new(),
    };

    if options.dry_run {
        return Ok(report);
    }

    enforce_output_root(&options.output_root, options.allow_unsafe_output_root)?;
    let target_root = options.output_root.join(&prefix);
    report.output_dir = target_root.to_string_lossy().into_owned();
    fs::create_dir_all(&target_root)?;

    for index in 0..page_count {
        let page_no = index + 1;
        let page = document
            .pages()
            .get(index as u16)
            .map_err(|e| PdfVeinError::Pdfium(format!("page {page_no} access failed: {e:?}")))?;
        let width_pts = page.width().value;
        let height_pts = page.height().value;
        if !width_pts.is_finite() || !height_pts.is_finite() || width_pts <= 0.0 || height_pts <= 0.0
        {
            return Err(PdfVeinError::InvalidPageGeometry { page_no });
        }
        let target_width_px =
            ((width_pts as f64) * (options.dpi as f64) / 72.0).round() as i32;
        let config = PdfRenderConfig::new()
            .set_target_width(target_width_px)
            .render_form_data(true)
            .render_annotations(true);
        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| PdfVeinError::Pdfium(format!("page {page_no} render failed: {e:?}")))?;
        let rgba = bitmap.as_image().to_rgba8();
        let (width_px, height_px) = (rgba.width() as i32, rgba.height() as i32);
        let page_sha256 = sha256_hex(rgba.as_raw());
        let mut png_bytes: Vec<u8> = Vec::new();
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .map_err(|e| PdfVeinError::Image(e.to_string()))?;
        let image_sha256 = sha256_hex(&png_bytes);
        let png_file = format!("page_{page_no:04}.png");
        fs::write(target_root.join(&png_file), &png_bytes)?;
        let spans = extract_spans(&page)?;
        report.pages.push(PageEvidence {
            page_no,
            page_sha256,
            image_sha256,
            png_file,
            width_pts,
            height_pts,
            target_width_px,
            width_px,
            height_px,
            spans,
        });
    }

    write_outputs(&target_root, &report)?;
    Ok(report)
}

fn write_outputs(root: &Path, report: &PdfScanReport) -> Result<(), PdfVeinError> {
    let mut index = String::new();
    index.push_str("{\n");
    index.push_str(&format!("  \"schema_version\": \"{SCHEMA_VERSION}\",\n"));
    index.push_str(&format!("  \"tool\": \"{}\",\n", json_escape(TOOL_ID)));
    index.push_str(&format!(
        "  \"source_pdf\": {{\"path\": \"{}\", \"sha256\": \"{}\", \"size_bytes\": {}}},\n",
        json_escape(&report.source_path),
        report.pdf_sha256,
        report.size_bytes
    ));
    index.push_str(&format!(
        "  \"output_dir\": \"{}\",\n",
        json_escape(&report.output_dir)
    ));
    index.push_str(&format!("  \"dpi\": {},\n", report.dpi));
    index.push_str(&format!("  \"page_count\": {},\n", report.page_count));
    index.push_str("  \"hash_definitions\": {\"page_sha256\": \"SHA-256 of raw RGBA pixels rendered at dpi\", \"image_sha256\": \"SHA-256 of encoded PNG file\"},\n");
    index.push_str("  \"bbox_convention\": \"PDF points, normalized [x0,y0,x1,y1] from pdfium-render PdfRect\",\n");
    index.push_str("  \"pages\": [\n");
    for (i, page) in report.pages.iter().enumerate() {
        let comma = if i + 1 < report.pages.len() { "," } else { "" };
        index.push_str(&format!(
            "    {{\"page_no\": {}, \"png_file\": \"{}\", \"page_sha256\": \"{}\", \"image_sha256\": \"{}\", \"width_pts\": {:.4}, \"height_pts\": {:.4}, \"width_px\": {}, \"height_px\": {}, \"span_count\": {}}}{comma}\n",
            page.page_no,
            json_escape(&page.png_file),
            page.page_sha256,
            page.image_sha256,
            page.width_pts,
            page.height_pts,
            page.width_px,
            page.height_px,
            page.spans.len()
        ));
    }
    index.push_str("  ]\n}\n");
    fs::write(root.join("evidence_index.json"), index)?;

    let mut pages_jsonl = String::new();
    for page in &report.pages {
        let spans_json: Vec<String> = page
            .spans
            .iter()
            .map(|span| {
                format!(
                    "{{\"text\": \"{}\", \"bbox\": [{:.4}, {:.4}, {:.4}, {:.4}]}}",
                    json_escape(&span.text),
                    span.bbox[0],
                    span.bbox[1],
                    span.bbox[2],
                    span.bbox[3]
                )
            })
            .collect();
        pages_jsonl.push_str(&format!(
            "{{\"page_no\": {}, \"page_sha256\": \"{}\", \"image_sha256\": \"{}\", \"png_file\": \"{}\", \"width_pts\": {:.4}, \"height_pts\": {:.4}, \"width_px\": {}, \"height_px\": {}, \"span_count\": {}, \"spans\": [{}]}}\n",
            page.page_no,
            page.page_sha256,
            page.image_sha256,
            json_escape(&page.png_file),
            page.width_pts,
            page.height_pts,
            page.width_px,
            page.height_px,
            page.spans.len(),
            spans_json.join(", ")
        ));
    }
    fs::write(root.join("pages.jsonl"), pages_jsonl)?;

    let mut render_manifest = String::new();
    for page in &report.pages {
        render_manifest.push_str(&format!(
            "{{\"page_no\": {}, \"dpi\": {}, \"target_width_px\": {}, \"actual_width_px\": {}, \"actual_height_px\": {}, \"png_file\": \"{}\", \"image_sha256\": \"{}\", \"render_config\": {{\"form_data\": true, \"annotations\": true}}}}\n",
            page.page_no,
            report.dpi,
            page.target_width_px,
            page.width_px,
            page.height_px,
            json_escape(&page.png_file),
            page.image_sha256
        ));
    }
    fs::write(root.join("render_manifest.jsonl"), render_manifest)?;
    Ok(())
}

pub fn execute(args: &[String]) -> i32 {
    let mut options = PdfVeinOptions::default();
    let mut positional: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--allow-unsafe-output-root" => options.allow_unsafe_output_root = true,
            "--dpi" => match iter.next().and_then(|value| value.parse::<u32>().ok()) {
                Some(value) if value > 0 => options.dpi = value,
                _ => {
                    eprintln!("ERROR: --dpi requires a positive integer");
                    return 2;
                }
            },
            "--output-root" => match iter.next() {
                Some(value) => options.output_root = PathBuf::from(value),
                None => {
                    eprintln!("ERROR: --output-root requires a value");
                    return 2;
                }
            },
            other => {
                if positional.is_none() {
                    positional = Some(PathBuf::from(other));
                } else {
                    eprintln!("ERROR: unexpected argument: {other}");
                    return 2;
                }
            }
        }
    }
    let Some(source) = positional else {
        eprintln!("ERROR: pdfscan requires a PDF path argument");
        return 2;
    };
    options.source = source;

    println!("TESSERA // P2A PDF EVIDENCE VEIN (CPU-only, deterministic)");
    println!("file   = {}", options.source.display());
    let report = match scan_pdf(&options) {
        Ok(report) => report,
        Err(PdfVeinError::OutputRootViolation { root }) => {
            eprintln!(
                "OUTPUT ROOT VIOLATION: {root} is not on D: (tests may pass --allow-unsafe-output-root)"
            );
            return 3;
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    println!("sha256 = {}", report.pdf_sha256);
    println!("size_bytes = {}", report.size_bytes);
    if report.dry_run {
        println!(
            "dry-run: accessibility OK | pages={} | wrote nothing",
            report.page_count
        );
        println!(
            "PDFSCAN | dry-run | pages={} | verdict=PASS",
            report.page_count
        );
        return 0;
    }
    println!("output_dir = {}", report.output_dir);
    println!("pages = {} | dpi = {}", report.page_count, report.dpi);
    for page in &report.pages {
        println!(
            "page {}: {:.1}x{:.1} pts -> {}x{} px | spans={} | page_sha256={} | image_sha256={}",
            page.page_no,
            page.width_pts,
            page.height_pts,
            page.width_px,
            page.height_px,
            page.spans.len(),
            short(&page.page_sha256),
            short(&page.image_sha256)
        );
        for (i, span) in page.spans.iter().enumerate() {
            println!(
                "  span {}: bbox=[{:.2}, {:.2}, {:.2}, {:.2}] text={:?}",
                i + 1,
                span.bbox[0],
                span.bbox[1],
                span.bbox[2],
                span.bbox[3],
                span.text
            );
        }
    }
    println!(
        "wrote: evidence_index.json pages.jsonl render_manifest.jsonl + {} PNG(s)",
        report.pages.len()
    );
    println!(
        "PDFSCAN | pages={} | sha256={} | verdict=PASS",
        report.page_count,
        short(&report.pdf_sha256)
    );
    0
}
