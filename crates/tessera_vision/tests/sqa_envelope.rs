use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tessera_vision::envelope::{
    envelope_check, estimate_from_header, format_envelope, read_safetensors_header,
};
use tessera_vision::error::VisionError;

const HEADER_FIXTURE_PARAMS: u64 = 1_857_482_608;
const HEADER_FIXTURE_SHA256: &str = "68654f62808680dde009f7aab60e7a9928c6770030213883a3d20584588bd966";
const WEIGHTS_BUDGET_MB: f64 = 4608.0;

fn header_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../evidence/synthetic/moondream2_safetensors_header.json")
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("sqa_envelope")
        .join(tag);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn fake_safetensors(dir: &Path, header: &str) -> PathBuf {
    let path = dir.join("model.safetensors");
    let header_bytes = header.as_bytes();
    let mut file = Vec::with_capacity(8 + header_bytes.len());
    file.extend_from_slice(&(header_bytes.len() as u64).to_le_bytes());
    file.extend_from_slice(header_bytes);
    fs::write(&path, &file).expect("fake safetensors write");
    path
}

#[test]
fn header_fixture_seal() {
    let bytes = fs::read(header_fixture()).expect("header fixture missing from pack");
    assert_eq!(hex::encode(Sha256::digest(&bytes)), HEADER_FIXTURE_SHA256);
}

#[test]
fn envelope_real_moondream_header() {
    let header = fs::read_to_string(header_fixture()).expect("header fixture read");
    let (params, mb_f16) = estimate_from_header(&header, true).expect("estimate f16");
    assert_eq!(params, HEADER_FIXTURE_PARAMS);
    assert!(
        (mb_f16 - 3542.9).abs() < 1.0,
        "f16 weights estimate drifted: {mb_f16}"
    );
    assert!(mb_f16 <= WEIGHTS_BUDGET_MB);

    let dir = temp_dir("envelope_real");
    let fake = fake_safetensors(&dir, &header);
    let report = envelope_check(&fake, true).expect("envelope must PASS for f16 target");
    let line = format_envelope(&report);
    println!("VRAM-ENVELOPE {line}");
    assert!(line.contains("PASS"), "envelope line: {line}");
    assert!(line.contains("headroom"), "envelope line: {line}");
    assert_eq!(report.params, HEADER_FIXTURE_PARAMS);

    let reject = envelope_check(&fake, false);
    assert!(
        matches!(reject, Err(VisionError::ModelTooLarge { .. })),
        "f32 sizing (7085.7MB) must be REJECTED by the 4608MB law"
    );
}

#[test]
fn envelope_rejects_oversize_and_malformed() {
    let dir = temp_dir("envelope_bad");
    let oversize = fake_safetensors(
        &dir,
        "{\"t\":{\"dtype\":\"F16\",\"shape\":[3000000000,2],\"data_offsets\":[0,4]}}",
    );
    assert!(matches!(
        envelope_check(&oversize, true),
        Err(VisionError::ModelTooLarge { .. })
    ));
    let malformed = fake_safetensors(&dir, "this is not json");
    assert!(matches!(
        estimate_from_header("this is not json", true),
        Err(VisionError::HeaderMalformed(_))
    ));
    assert!(matches!(
        envelope_check(&malformed, true),
        Err(VisionError::HeaderMalformed(_))
    ));
    let no_tensors = "{\"__metadata__\":{\"format\":\"pt\"}}";
    assert!(matches!(
        estimate_from_header(no_tensors, true),
        Err(VisionError::HeaderMalformed(_))
    ));
    let truncated = dir.join("truncated.safetensors");
    fs::write(&truncated, [1u8, 2, 3, 4]).expect("truncated write");
    assert!(matches!(
        read_safetensors_header(&truncated),
        Err(VisionError::Io(_))
    ));
}

#[test]
fn mini_safetensors_roundtrip() {
    let dir = temp_dir("mini");
    let header = "{\"t\":{\"dtype\":\"F32\",\"shape\":[2,2],\"data_offsets\":[0,16]}}";
    let path = dir.join("mini.safetensors");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&[0u8; 16]);
    fs::write(&path, &bytes).expect("mini write");
    let parsed = read_safetensors_header(&path).expect("header read");
    assert_eq!(parsed, header);
    let (params, mb) = estimate_from_header(&parsed, true).expect("estimate");
    assert_eq!(params, 4);
    assert!(mb > 0.0 && mb < 0.001);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn fuzz_estimate_from_header_zero_panics(header in any::<String>()) {
        let _ = estimate_from_header(&header, true);
        let _ = estimate_from_header(&header, false);
    }

    #[test]
    fn fuzz_read_header_zero_panics(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sqa_envelope_fuzz");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fuzz.safetensors");
        fs::write(&path, &bytes).unwrap();
        let _ = read_safetensors_header(&path);
    }
}
