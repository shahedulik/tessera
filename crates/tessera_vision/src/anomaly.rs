use crate::engine::{VisionEngine, DEFAULT_MODEL_DIR, DEFAULT_SEED};
use crate::envelope::{envelope_check, format_envelope};
use crate::error::VisionError;
use crate::pdf_vein::{scan_pdf, PdfVeinOptions, DEFAULT_DPI, DEFAULT_OUTPUT_ROOT};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_QUESTION: &str = "Examine this rendered document page. List visual anomalies such as altered stamps, font mismatches, hidden overlays, or signs of tampering. Start each anomaly with '-' on its own line. If there are none, answer exactly NO ANOMALIES.";

pub struct AnomalyOptions {
    pub pdf: Option<PathBuf>,
    pub model_dir: PathBuf,
    pub output_root: PathBuf,
    pub question: String,
    pub seed: u64,
    pub allow_unsafe_output_root: bool,
    pub envelope_only: bool,
}

impl Default for AnomalyOptions {
    fn default() -> Self {
        Self {
            pdf: None,
            model_dir: PathBuf::from(DEFAULT_MODEL_DIR),
            output_root: PathBuf::from(DEFAULT_OUTPUT_ROOT),
            question: DEFAULT_QUESTION.to_string(),
            seed: DEFAULT_SEED,
            allow_unsafe_output_root: false,
            envelope_only: false,
        }
    }
}

fn parse_args(args: &[String]) -> Result<AnomalyOptions, String> {
    let mut options = AnomalyOptions::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--pdf" => {
                options.pdf = iter.next().map(PathBuf::from);
            }
            "--model-dir" => {
                options.model_dir = iter
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--model-dir requires a value")?;
            }
            "--output-root" => {
                options.output_root = iter
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--output-root requires a value")?;
            }
            "--question" => {
                options.question = iter
                    .next()
                    .cloned()
                    .ok_or("--question requires a value")?;
            }
            "--seed" => {
                options.seed = iter
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or("--seed requires an unsigned integer")?;
            }
            "--allow-unsafe-output-root" => options.allow_unsafe_output_root = true,
            "--envelope-only" => options.envelope_only = true,
            other => {
                if options.pdf.is_none() && !other.starts_with("--") {
                    options.pdf = Some(PathBuf::from(other));
                } else {
                    return Err(format!("unexpected argument: {other}"));
                }
            }
        }
    }
    Ok(options)
}

fn run_envelope_only(model_dir: &Path) -> i32 {
    println!("TESSERA // P2B VRAM ENVELOPE PREFLIGHT (no GPU load, header-only)");
    println!("model_dir = {}", model_dir.display());
    let weights = model_dir.join("model.safetensors");
    match envelope_check(&weights, true) {
        Ok(report) => {
            println!("[VDU] VRAM envelope: {}", format_envelope(&report));
            println!("ENVELOPE | verdict=PASS");
            0
        }
        Err(VisionError::ModelTooLarge {
            estimated_mb,
            limit_mb,
        }) => {
            eprintln!("ENVELOPE | verdict=REJECT estimated={estimated_mb:.1}MB limit={limit_mb}MB");
            2
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            2
        }
    }
}

pub fn execute(args: &[String]) -> i32 {
    let options = match parse_args(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("ERROR: {message}");
            return 2;
        }
    };
    println!("TESSERA // P2B VISION ANOMALY ENGINE (local VLM, deterministic greedy)");
    if options.envelope_only {
        return run_envelope_only(&options.model_dir);
    }
    let Some(pdf) = options.pdf.clone() else {
        eprintln!("ERROR: anomaly requires --pdf <path>");
        return 2;
    };
    let vein_options = PdfVeinOptions {
        source: pdf,
        output_root: options.output_root,
        dpi: DEFAULT_DPI,
        dry_run: false,
        allow_unsafe_output_root: options.allow_unsafe_output_root,
    };
    println!("[P2A] running PDF evidence vein first ...");
    let report = match scan_pdf(&vein_options) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("ERROR: vein failed: {e}");
            return if matches!(e, crate::pdf_vein::PdfVeinError::OutputRootViolation { .. }) {
                3
            } else {
                2
            };
        }
    };
    let out_dir = PathBuf::from(&report.output_dir);
    println!(
        "[P2A] vein complete: pages={} output_dir={}",
        report.page_count, report.output_dir
    );
    let mut engine = match VisionEngine::open_vlm(&options.model_dir, options.seed, true) {
        Ok(engine) => engine,
        Err(VisionError::CudaRequired(message)) => {
            eprintln!("GPU_REQUIRED: {message}");
            return 5;
        }
        Err(VisionError::ModelTooLarge {
            estimated_mb,
            limit_mb,
        }) => {
            eprintln!("ENVELOPE REJECT: {estimated_mb:.1}MB > {limit_mb}MB");
            return 2;
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    let mut jsonl = String::new();
    let mut candidate_total = 0usize;
    for page in &report.pages {
        let png_path = out_dir.join(&page.png_file);
        let analysis = match engine.analyze_page(
            &png_path,
            &page.spans,
            &options.question,
            page.width_pts,
            page.height_pts,
        ) {
            Ok(analysis) => analysis,
            Err(e) => {
                eprintln!("ERROR: page {} analysis failed: {e}", page.page_no);
                return 2;
            }
        };
        println!(
            "page {}: answer={:?} | candidates={}",
            page.page_no,
            analysis.answer,
            analysis.candidates.len()
        );
        for candidate in &analysis.candidates {
            println!(
                "  CANDIDATE bbox=[{:.2}, {:.2}, {:.2}, {:.2}] conf={:.2} grounding={} desc={:?}",
                candidate.bbox[0],
                candidate.bbox[1],
                candidate.bbox[2],
                candidate.bbox[3],
                candidate.confidence,
                candidate.grounding,
                candidate.description
            );
        }
        candidate_total += analysis.candidates.len();
        let mut line = analysis.to_jsonl_line();
        line.push('\n');
        jsonl.push_str(&line);
    }
    if let Err(e) = fs::write(out_dir.join("anomaly_candidates.jsonl"), &jsonl) {
        eprintln!("ERROR: cannot write anomaly_candidates.jsonl: {e}");
        return 2;
    }
    println!("wrote: anomaly_candidates.jsonl ({} page lines)", report.pages.len());
    println!(
        "ANOMALY | pages={} | candidates={} | model=moondream2-f16 | seed={} | verdict=PASS",
        report.page_count, candidate_total, options.seed
    );
    0
}
