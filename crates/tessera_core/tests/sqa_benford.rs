use proptest::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tessera_core::benford::{
    analyze_amounts, chi2_sf, execute, expected_proportion, parse_csv_bytes, read_fixture_amounts,
    sha256_hex, DEFAULT_FIXTURE, SEALED_FIXTURE_SHA256,
};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence/synthetic/synthetic_tenders.csv")
}

#[test]
fn default_fixture_path_is_windows_d_drive() {
    assert_eq!(
        DEFAULT_FIXTURE,
        "D:\\tessera\\evidence\\synthetic\\synthetic_tenders.csv"
    );
}

#[test]
fn fixture_hash_matches_seal() {
    let bytes = fs::read(fixture_path()).expect("sealed fixture missing from pack");
    assert_eq!(sha256_hex(&bytes), SEALED_FIXTURE_SHA256);
}

#[test]
fn acceptance_chi2_and_p_value() {
    let (amounts, rows_total, null_count) =
        read_fixture_amounts(&fixture_path()).expect("fixture ingest failed");
    assert_eq!(rows_total, 5000);
    assert_eq!(null_count, 0);
    let stats = analyze_amounts(&amounts);
    assert_eq!(stats.scored, 5000);
    assert_eq!(stats.excluded, 0);
    assert_eq!(
        stats.observed,
        [1172, 800, 685, 563, 478, 359, 316, 264, 363]
    );
    assert!(
        (stats.chi2 - 199.733).abs() <= 0.01,
        "chi2 acceptance failed: {}",
        stats.chi2
    );
    let rel_err = (stats.p_value - 7.274221e-39).abs() / 7.274221e-39;
    assert!(rel_err < 1e-6, "p-value acceptance failed: {:e}", stats.p_value);
    assert!(stats.p_value < 0.05);
    assert!(stats.flags.iter().any(|f| f.digit == 9));
}

#[test]
fn chi2_sf_calibration_and_edges() {
    assert!((chi2_sf(15.5073, 8) - 0.05).abs() < 1e-3);
    assert!((chi2_sf(0.0, 8) - 1.0).abs() < 1e-12);
    assert!(chi2_sf(f64::INFINITY, 8) < 1e-300);
    assert!(chi2_sf(f64::NAN, 8) > 0.999);
    assert!(chi2_sf(10.0, 8) > chi2_sf(20.0, 8));
    assert!(chi2_sf(20.0, 8) > chi2_sf(40.0, 8));
}

#[test]
fn expected_proportions_sum_to_one() {
    let total: f64 = (1..=9u32).map(expected_proportion).sum();
    assert!((total - 1.0).abs() < 1e-12);
}

#[test]
fn hash_gate_rejects_tampered_fixture() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(dir).expect("cannot create CARGO_TARGET_TMPDIR");
    let tampered = dir.join("tampered_tenders.csv");
    let mut bytes = fs::read(fixture_path()).expect("sealed fixture missing from pack");
    let pos = bytes
        .windows(8)
        .rposition(|w| w == b"District")
        .expect("district marker missing");
    bytes[pos] += 0x20;
    fs::write(&tampered, &bytes).expect("tamper write failed");
    assert_eq!(execute(&tampered, false), 3);
    assert_eq!(execute(&tampered, true), 0);
    assert_eq!(execute(Path::new("Z:/definitely/missing.csv"), false), 2);
    fs::remove_file(&tampered).ok();
}

fn edge_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(0.0),
        Just(-0.0),
        Just(f64::NAN),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(-1.0),
        Just(1e-320),
        Just(1e308),
        Just(9.999_999_999e-1),
        Just(10.0),
        any::<f64>(),
    ]
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
    fn fuzz_analyze_zero_panics(amounts in prop::collection::vec(edge_f64(), 0..512)) {
        let stats = analyze_amounts(&amounts);
        prop_assert_eq!(stats.scored + stats.excluded, amounts.len());
        prop_assert!(stats.chi2 >= 0.0);
        prop_assert!((0.0..=1.0).contains(&stats.p_value));
        let observed_sum: u64 = stats.observed.iter().sum();
        prop_assert_eq!(observed_sum, stats.scored as u64);
        for flag in &stats.flags {
            prop_assert!((1..=9).contains(&flag.digit));
        }
    }

    #[test]
    fn fuzz_parse_malformed_csv_zero_panics(body in malformed_csv()) {
        let _ = parse_csv_bytes(body.as_bytes());
    }
}
