#![cfg(feature = "pdf")]

use proptest::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tessera_vision::pdf_vein::{
    enforce_output_root, execute, json_escape, scan_pdf, sha256_hex, PdfVeinError, PdfVeinOptions,
    DEFAULT_DPI, MAX_PAGE_COUNT,
};

const FIXTURE_SHA256: &str = "96bdc602f391621565cf76ed362cecd53326fa5ca520a4c52f3291bd6a578909";
const FIXTURE_PREFIX: &str = "96bdc602f3916215";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence/synthetic/tessera_fixture_1p.pdf")
}

fn temp_root(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("pdf_vein")
        .join(tag);
    fs::create_dir_all(&root).expect("cannot create temp root");
    root
}

fn fixture_options(root: PathBuf) -> PdfVeinOptions {
    PdfVeinOptions {
        source: fixture_path(),
        output_root: root,
        dpi: DEFAULT_DPI,
        dry_run: false,
        allow_unsafe_output_root: true,
    }
}

fn json_unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let digits: String = chars.by_ref().take(4).collect();
                let code = u32::from_str_radix(&digits, 16).unwrap_or(0xFFFD);
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[test]
fn fixture_seal() {
    let bytes = fs::read(fixture_path()).expect("sealed PDF fixture missing from pack");
    assert_eq!(sha256_hex(&bytes), FIXTURE_SHA256);
    assert_eq!(bytes.len(), 635);
}

#[test]
fn dry_run_validates_and_writes_nothing() {
    let root = temp_root("dry");
    let out_dir = root.join(FIXTURE_PREFIX);
    let _ = fs::remove_dir_all(&out_dir);
    let mut options = fixture_options(root);
    options.dry_run = true;
    let report = scan_pdf(&options).expect("dry run failed");
    assert_eq!(report.page_count, 1);
    assert!(report.pages.is_empty());
    assert!(!out_dir.exists(), "dry-run must write nothing");
}

#[test]
fn full_scan_acceptance() {
    let root = temp_root("full");
    let report = scan_pdf(&fixture_options(root.clone())).expect("full scan failed");
    assert_eq!(report.pdf_sha256, FIXTURE_SHA256);
    assert_eq!(report.page_count, 1);
    assert_eq!(report.pages.len(), 1);
    let page = &report.pages[0];
    assert_eq!(page.page_no, 1);
    assert!((page.width_pts - 612.0).abs() < 0.5);
    assert!((page.height_pts - 792.0).abs() < 0.5);
    assert_eq!(page.target_width_px, 1700);
    assert_eq!(page.width_px, 1700);
    assert!(page.height_px > 0);
    assert!(page.spans.len() >= 2);
    let joined: Vec<&str> = page.spans.iter().map(|span| span.text.as_str()).collect();
    let joined = joined.join(" ");
    assert!(joined.contains("TESSERA EVIDENCE FIXTURE"), "spans: {joined:?}");
    assert!(joined.contains("PAGE ONE OF ONE"), "spans: {joined:?}");
    for span in &page.spans {
        assert!(span.bbox.iter().all(|v| v.is_finite()));
        assert!(span.bbox[0] < span.bbox[2]);
        assert!(span.bbox[1] < span.bbox[3]);
        assert!(span.bbox[0] >= -1.0 && span.bbox[2] <= page.width_pts + 1.0);
        assert!(span.bbox[1] >= -1.0 && span.bbox[3] <= page.height_pts + 1.0);
    }
    let out = root.join(FIXTURE_PREFIX);
    for name in [
        "evidence_index.json",
        "pages.jsonl",
        "render_manifest.jsonl",
        "page_0001.png",
    ] {
        assert!(out.join(name).exists(), "missing output {name}");
    }
    let png = fs::read(out.join("page_0001.png")).expect("png read");
    assert_eq!(sha256_hex(&png), page.image_sha256);
    let index = fs::read_to_string(out.join("evidence_index.json")).expect("index read");
    assert!(index.starts_with('{') && index.trim_end().ends_with('}'));
    for key in [
        "schema_version",
        "tool",
        "source_pdf",
        "output_dir",
        "dpi",
        "page_count",
        "hash_definitions",
        "bbox_convention",
        "pages",
    ] {
        assert!(index.contains(&format!("\"{key}\"")), "index missing key {key}");
    }
    let pages_jsonl = fs::read_to_string(out.join("pages.jsonl")).expect("pages read");
    assert_eq!(pages_jsonl.lines().count(), 1);
    assert!(pages_jsonl.contains("\"page_no\": 1"));
    assert!(pages_jsonl.contains("TESSERA EVIDENCE FIXTURE"));
    let render = fs::read_to_string(out.join("render_manifest.jsonl")).expect("render read");
    assert_eq!(render.lines().count(), 1);
    assert!(render.contains(&page.image_sha256));
    for text in [&index, &pages_jsonl, &render] {
        assert!(!text.contains("C:\\"), "C: path leaked into outputs");
        assert!(!text.contains("C:/"), "C: path leaked into outputs");
    }
    let code = execute(&[
        fixture_path().to_string_lossy().into_owned(),
        "--output-root".into(),
        root.to_string_lossy().into_owned(),
        "--allow-unsafe-output-root".into(),
    ]);
    assert_eq!(code, 0);
}

#[test]
fn outputs_are_byte_stable_across_runs() {
    let root = temp_root("stable");
    let first = scan_pdf(&fixture_options(root.clone())).expect("run 1 failed");
    let out = root.join(FIXTURE_PREFIX);
    let index1 = fs::read(out.join("evidence_index.json")).expect("index read");
    let pages1 = fs::read(out.join("pages.jsonl")).expect("pages read");
    let render1 = fs::read(out.join("render_manifest.jsonl")).expect("render read");
    let png1 = fs::read(out.join("page_0001.png")).expect("png read");
    let second = scan_pdf(&fixture_options(root.clone())).expect("run 2 failed");
    assert_eq!(first.pages[0].page_sha256, second.pages[0].page_sha256);
    assert_eq!(first.pages[0].image_sha256, second.pages[0].image_sha256);
    assert_eq!(index1, fs::read(out.join("evidence_index.json")).expect("index read 2"));
    assert_eq!(pages1, fs::read(out.join("pages.jsonl")).expect("pages read 2"));
    assert_eq!(render1, fs::read(out.join("render_manifest.jsonl")).expect("render read 2"));
    assert_eq!(png1, fs::read(out.join("page_0001.png")).expect("png read 2"));
}

#[test]
fn missing_file_is_typed_error() {
    let mut options = fixture_options(temp_root("missing"));
    options.source = PathBuf::from("Z:/definitely/missing.pdf");
    assert!(matches!(scan_pdf(&options), Err(PdfVeinError::Io(_))));
    assert!(matches!(
        scan_pdf(&PdfVeinOptions::default()),
        Err(PdfVeinError::MissingSource)
    ));
}

#[test]
fn corrupt_pdf_is_typed_error_zero_panic() {
    let dir = temp_root("corrupt");
    let corrupt = dir.join("corrupt.pdf");
    let mut bytes = b"%PDF-1.4\n".to_vec();
    bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    bytes.extend_from_slice(b"GARBAGEGARBAGEGARBAGE not a pdf body at all");
    fs::write(&corrupt, &bytes).expect("corrupt fixture write");
    let mut options = fixture_options(dir);
    options.source = corrupt;
    let err = scan_pdf(&options).expect_err("corrupt PDF must be rejected");
    assert!(matches!(err, PdfVeinError::Pdfium(_) | PdfVeinError::EmptyDocument));
}

#[test]
fn output_root_policy_enforces_d_drive() {
    assert!(matches!(
        enforce_output_root(Path::new("C:\\evil\\out"), false),
        Err(PdfVeinError::OutputRootViolation { .. })
    ));
    assert!(enforce_output_root(Path::new("D:\\tessera\\evidence\\pdf_out"), false).is_ok());
    assert!(enforce_output_root(Path::new("D:/tessera/out"), false).is_ok());
    assert!(enforce_output_root(Path::new("/tmp/whatever"), false).is_err());
    assert!(enforce_output_root(Path::new("/tmp/whatever"), true).is_ok());
    let mut options = fixture_options(PathBuf::from("C:\\evil\\out"));
    options.allow_unsafe_output_root = false;
    assert!(matches!(
        scan_pdf(&options),
        Err(PdfVeinError::OutputRootViolation { .. })
    ));
}

#[test]
fn json_escape_known_vectors() {
    assert_eq!(json_escape("plain"), "plain");
    assert_eq!(json_escape("quo\"te"), "quo\\\"te");
    assert_eq!(json_escape("back\\slash"), "back\\\\slash");
    assert_eq!(json_escape("line\nbreak\r\n"), "line\\nbreak\\r\\n");
    assert_eq!(json_escape("tab\there"), "tab\\there");
    assert_eq!(json_escape("\u{1}"), "\\u0001");
    assert_eq!(json_escape("\u{8}\u{c}"), "\\b\\f");
    assert_eq!(json_escape("unicode \u{e9} \u{4e2d}"), "unicode \u{e9} \u{4e2d}");
}

fn random_pdf_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..8192),
        prop::collection::vec(any::<u8>(), 0..4096).prop_map(|mut tail| {
            let mut bytes = b"%PDF-1.4\n".to_vec();
            bytes.append(&mut tail);
            bytes
        }),
        Just(Vec::new()),
        Just(b"%PDF-1.4\n%%EOF\n".to_vec()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn fuzz_json_escape_roundtrip_no_raw_controls(value in any::<String>()) {
        let escaped = json_escape(&value);
        prop_assert!(!escaped.chars().any(|c| u32::from(c) < 0x20));
        prop_assert_eq!(json_unescape(&escaped), value);
    }

    #[test]
    fn fuzz_random_pdf_streams_zero_panics(bytes in random_pdf_strategy()) {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pdf_vein_fuzz");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fuzz_case.pdf");
        fs::write(&path, &bytes).unwrap();
        let options = PdfVeinOptions {
            source: path,
            output_root: dir.join("out"),
            dpi: 72,
            dry_run: false,
            allow_unsafe_output_root: true,
        };
        if let Ok(report) = scan_pdf(&options) {
            prop_assert!(report.page_count <= MAX_PAGE_COUNT);
            prop_assert_eq!(report.page_count, report.pages.len());
            for page in &report.pages {
                prop_assert!(page.width_px >= 0 && page.height_px >= 0);
                for span in &page.spans {
                    prop_assert!(span.bbox.iter().all(|v| v.is_finite()));
                }
            }
        }
        let _ = fs::remove_dir_all(dir.join("out"));
    }
}
