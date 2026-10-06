#![cfg(feature = "db")]

use proptest::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tessera_core::benford::SEALED_FIXTURE_SHA256;
use tessera_core::ingest::{execute, ingest_bytes, ingest_file, IngestError, TENDERS_SCHEMA};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence/synthetic/synthetic_tenders.csv")
}

#[test]
fn ingest_acceptance() {
    let report = ingest_file(&fixture_path(), false).expect("sealed fixture ingest failed");
    assert_eq!(report.sha256, SEALED_FIXTURE_SHA256);
    assert_eq!(report.row_count, 5000);
    assert_eq!(report.duckdb_count, 5000);
    assert!(report.duckdb_version.starts_with('v'));
    assert_eq!(TENDERS_SCHEMA.len(), 6);
    assert_eq!(
        report.stats.observed,
        [1172, 800, 685, 563, 478, 359, 316, 264, 363]
    );
    assert!((report.stats.chi2 - 199.733).abs() <= 0.01);
    let rel_err = (report.stats.p_value - 7.274221e-39).abs() / 7.274221e-39;
    assert!(rel_err < 1e-6);
    assert_eq!(execute(&fixture_path(), false), 0);
}

#[test]
fn ingest_rejects_tampered_fixture() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(dir).expect("cannot create CARGO_TARGET_TMPDIR");
    let tampered = dir.join("tampered_ingest.csv");
    let mut bytes = fs::read(fixture_path()).expect("fixture missing");
    let pos = bytes
        .windows(8)
        .rposition(|w| w == b"District")
        .expect("district marker missing");
    bytes[pos] += 0x20;
    fs::write(&tampered, &bytes).expect("tamper write failed");
    assert!(matches!(
        ingest_file(&tampered, false),
        Err(IngestError::ForensicBreak { .. })
    ));
    assert_eq!(execute(&tampered, false), 3);
    assert_eq!(execute(&tampered, true), 0);
    fs::remove_file(&tampered).ok();
}

#[test]
fn ingest_rejects_missing_file() {
    let missing = Path::new("Z:/definitely/missing.csv");
    assert!(matches!(ingest_file(missing, false), Err(IngestError::Io(_))));
    assert_eq!(execute(missing, false), 2);
}

#[test]
fn ingest_rejects_null_amount() {
    let csv = "transaction_id,company_name,owner_name,amount_usd,date,district\n\
               T1,Acme Holdings,Owner Person,,2025-01-01,District 01\n";
    let err = ingest_bytes(csv.as_bytes(), false).expect_err("null amount must be rejected");
    assert!(matches!(err, IngestError::NullField { column: "amount_usd", .. }));
}

fn malformed_cell() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("abc".to_string()),
        Just("1e999".to_string()),
        Just("-5".to_string()),
        Just("NaN".to_string()),
        Just("inf".to_string()),
        Just("90000.1".to_string()),
        Just("0.0".to_string()),
        Just("\"unterminated".to_string()),
        Just("1,2".to_string()),
        Just("\t".to_string()),
        Just("9".repeat(400)),
        "-?[0-9]{0,12}(\\.[0-9]{0,4})?(e-?[0-9]{0,3})?",
        any::<f64>().prop_map(|v| v.to_string()),
    ]
}

fn malformed_csv() -> impl Strategy<Value = String> {
    let row = prop::collection::vec(malformed_cell(), 6);
    prop::collection::vec(row, 0..8).prop_map(|rows| {
        let mut out =
            String::from("transaction_id,company_name,owner_name,amount_usd,date,district\n");
        for r in rows {
            out.push_str(&r.join(","));
            out.push('\n');
        }
        out
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn fuzz_ingest_malformed_rows_zero_panics(body in malformed_csv()) {
        if let Ok(report) = ingest_bytes(body.as_bytes(), false) {
            prop_assert_eq!(report.row_count as i64, report.duckdb_count);
            prop_assert!(report.stats.chi2 >= 0.0);
            prop_assert!((0.0..=1.0).contains(&report.stats.p_value));
        }
    }
}
