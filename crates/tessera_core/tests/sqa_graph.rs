#![cfg(feature = "db")]

use proptest::prelude::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use kuzu::{Connection, Database, SystemConfig};
use tessera_core::benford::parse_csv_bytes;
use tessera_core::graph::{
    cypher_escape, detect_rings, load_fixture, parse_iso_days, RING_WINDOW_DAYS,
};
use tessera_core::ingest::extract_rows;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence/synthetic/synthetic_tenders.csv")
}

fn kuzu_runtime_available() -> bool {
    std::env::var("KUZU_LIBRARY_DIR").is_ok()
}

fn fixture_rows(exclude_tx: Option<&str>) -> Vec<tessera_core::ingest::TenderRow> {
    let bytes = fs::read(fixture_path()).expect("fixture missing");
    let batches = parse_csv_bytes(&bytes).expect("fixture parse failed");
    let mut rows = extract_rows(&batches).expect("fixture extract failed");
    if let Some(tx) = exclude_tx {
        rows.retain(|row| row.transaction_id != tx);
    }
    rows
}

fn ring_sets<const N: usize>(expected: [&str; N]) -> BTreeSet<String> {
    expected.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn graph_counts_and_exact_ring_ids() {
    if !kuzu_runtime_available() {
        println!("SKIP: KUZU_LIBRARY_DIR absent - db-gated graph test self-skips (CI stays spine-only)");
        return;
    }
    let rows = fixture_rows(None);
    let database = Database::in_memory(SystemConfig::default()).expect("kuzu init failed");
    let conn = Connection::new(&database).expect("kuzu connection failed");
    let counts = load_fixture(&conn, &rows).expect("graph load failed");

    assert_eq!(counts.companies, 4960);
    assert_eq!(counts.persons, 4947);
    assert_eq!(counts.tenders, 5000);
    assert_eq!(counts.owned_by, 4951);
    assert_eq!(counts.bid_on, 5000);
    assert_eq!(counts.transferred_to, 9);
    assert_eq!(counts.shares_address, 4);

    let rings = detect_rings(&conn, &rows).expect("ring detection failed");
    assert_eq!(rings.len(), 3, "exactly three rings required");

    let ground_truth: [([&str; 3], [&str; 3]); 3] = [
        (
            ["Meridian Axis Ltd", "Basalt Logistics LLC", "Cobalt Ridge Systems Inc"],
            ["TXN-000214", "TXN-002709", "TXN-002962"],
        ),
        (
            ["Halcyon Drilling Co", "Verdant Gate Holdings", "Solace Marine Works Ltd"],
            ["TXN-004331", "TXN-003758", "TXN-004065"],
        ),
        (
            ["Ironvale Construction Group", "Peregrine Fuel Trading LLC", "Northmoor Equipment Rental"],
            ["TXN-002918", "TXN-001967", "TXN-002271"],
        ),
    ];
    for (index, ring) in rings.iter().enumerate() {
        assert_eq!(ring.ring_id, format!("RING-{:02}", index + 1));
        let (expected_companies, expected_tx) = ground_truth[index];
        assert_eq!(
            BTreeSet::from(ring.companies.clone()),
            ring_sets(expected_companies),
            "ring {} company set mismatch",
            ring.ring_id
        );
        assert_eq!(
            ring.tx_ids.iter().cloned().collect::<BTreeSet<String>>(),
            ring_sets(expected_tx),
            "ring {} tx set mismatch",
            ring.ring_id
        );
        assert!(ring.span_days <= RING_WINDOW_DAYS);
        assert!(ring.amounts.iter().all(|a| a.is_finite() && *a > 0.0));
        assert!(ring.dates.iter().all(|d| parse_iso_days(d).is_some()));
    }
}

#[test]
fn ring_breaks_when_hop_removed() {
    if !kuzu_runtime_available() {
        println!("SKIP: KUZU_LIBRARY_DIR absent");
        return;
    }
    let rows = fixture_rows(Some("TXN-002709"));
    let database = Database::in_memory(SystemConfig::default()).expect("kuzu init failed");
    let conn = Connection::new(&database).expect("kuzu connection failed");
    load_fixture(&conn, &rows).expect("graph load failed");
    let rings = detect_rings(&conn, &rows).expect("ring detection failed");
    assert_eq!(rings.len(), 2);
    assert_eq!(rings[0].ring_id, "RING-01");
    assert_eq!(
        BTreeSet::from(rings[0].companies.clone()),
        ring_sets(["Halcyon Drilling Co", "Verdant Gate Holdings", "Solace Marine Works Ltd"])
    );
    assert_eq!(
        BTreeSet::from(rings[1].companies.clone()),
        ring_sets([
            "Ironvale Construction Group",
            "Peregrine Fuel Trading LLC",
            "Northmoor Equipment Rental"
        ])
    );
}

#[test]
fn iso_day_arithmetic() {
    assert_eq!(parse_iso_days("1970-01-01"), Some(0));
    assert_eq!(
        parse_iso_days("2025-03-14").map(|d| d - parse_iso_days("2025-03-10").unwrap()),
        Some(4)
    );
    assert_eq!(
        parse_iso_days("2024-03-01").map(|d| d - parse_iso_days("2024-02-28").unwrap()),
        Some(2)
    );
    assert_eq!(parse_iso_days("2025-13-01"), None);
    assert_eq!(parse_iso_days("abc"), None);
    assert_eq!(parse_iso_days("2025-01"), None);
}

#[test]
fn escape_handles_quotes_explicitly() {
    assert_eq!(cypher_escape("O'Brien"), "O\\'Brien");
    assert_eq!(cypher_escape("back\\slash"), "back\\\\slash");
    assert_eq!(cypher_escape("plain name"), "plain name");
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn fuzz_cypher_escape_zero_panics_roundtrip(value in any::<String>()) {
        let escaped = cypher_escape(&value);
        prop_assert_eq!(unescape(&escaped), value);
    }
}
