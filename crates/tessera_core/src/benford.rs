use arrow::array::{Float64Array, RecordBatch};
use arrow::csv::ReaderBuilder;
use arrow::datatypes::{DataType, Field, Schema};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

pub const DEFAULT_FIXTURE: &str = "D:\\tessera\\evidence\\synthetic\\synthetic_tenders.csv";
pub const SEALED_FIXTURE_SHA256: &str =
    "2fe27f0f06c362361a407c34df53b3fc3abe9534effcf3e7a78c01421e61a8a4";
pub const ACCEPTANCE_CHI2: f64 = 199.733;
pub const ACCEPTANCE_CHI2_TOLERANCE: f64 = 0.01;
pub const ACCEPTANCE_P_VALUE: f64 = 7.274221e-39;
pub const CHI2_DF: u32 = 8;
pub const FLAG_CONTRIBUTION_THRESHOLD: f64 = 3.841;
pub const P_VALUE_GATE: f64 = 0.05;

#[derive(Debug)]
pub enum BenfordError {
    Io(std::io::Error),
    Arrow(arrow::error::ArrowError),
    Schema(String),
}

impl fmt::Display for BenfordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Arrow(e) => write!(f, "Arrow CSV error: {e}"),
            Self::Schema(m) => write!(f, "Schema error: {m}"),
        }
    }
}

impl std::error::Error for BenfordError {}

impl From<std::io::Error> for BenfordError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<arrow::error::ArrowError> for BenfordError {
    fn from(e: arrow::error::ArrowError) -> Self {
        Self::Arrow(e)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DigitFlag {
    pub digit: u32,
    pub observed: u64,
    pub expected: f64,
    pub deviation: f64,
    pub contribution: f64,
}

#[derive(Debug, Clone)]
pub struct BenfordStats {
    pub scored: usize,
    pub excluded: usize,
    pub observed: [u64; 9],
    pub chi2: f64,
    pub p_value: f64,
    pub flags: Vec<DigitFlag>,
}

pub fn expected_proportion(digit: u32) -> f64 {
    (1.0 + 1.0 / f64::from(digit)).log10()
}

fn first_digit(value: f64) -> usize {
    let mut mantissa = value;
    while mantissa >= 10.0 {
        mantissa /= 10.0;
    }
    while mantissa < 1.0 {
        mantissa *= 10.0;
    }
    (mantissa as usize).clamp(1, 9)
}

fn gamma_lower_series(a: f64, x: f64) -> f64 {
    let mut ap = a;
    let mut sum = 1.0 / a;
    let mut delta = sum;
    for _ in 0..10_000 {
        ap += 1.0;
        delta *= x / ap;
        sum += delta;
        if delta.abs() < sum.abs() * 1e-16 {
            break;
        }
    }
    sum * (-x + a * x.ln() - libm::lgamma(a)).exp()
}

fn gamma_upper_cf(a: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / TINY;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..10_000 {
        let i_f = i as f64;
        let an = -i_f * (i_f - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < TINY {
            d = TINY;
        }
        c = b + an / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            break;
        }
    }
    (-x + a * x.ln() - libm::lgamma(a)).exp() * h
}

pub fn chi2_sf(x: f64, df: u32) -> f64 {
    if x.is_nan() {
        return 1.0;
    }
    if x.is_infinite() {
        return 0.0;
    }
    if x <= 0.0 {
        return 1.0;
    }
    let a = f64::from(df) / 2.0;
    let half_x = x / 2.0;
    let p = if half_x < a + 1.0 {
        1.0 - gamma_lower_series(a, half_x)
    } else {
        gamma_upper_cf(a, half_x)
    };
    p.clamp(0.0, 1.0)
}

pub fn analyze_amounts(amounts: &[f64]) -> BenfordStats {
    let mut observed = [0u64; 9];
    let mut scored = 0usize;
    let mut excluded = 0usize;
    for &value in amounts {
        if !value.is_finite() || value <= 0.0 {
            excluded += 1;
            continue;
        }
        observed[first_digit(value) - 1] += 1;
        scored += 1;
    }
    let n = scored as f64;
    let mut chi2 = 0.0;
    let mut flags = Vec::new();
    for (idx, &count) in observed.iter().enumerate() {
        let digit = (idx + 1) as u32;
        let expected = n * expected_proportion(digit);
        if expected <= 0.0 {
            continue;
        }
        let observed_f = count as f64;
        let deviation = observed_f - expected;
        let contribution = deviation * deviation / expected;
        chi2 += contribution;
        if contribution > FLAG_CONTRIBUTION_THRESHOLD {
            flags.push(DigitFlag {
                digit,
                observed: count,
                expected,
                deviation,
                contribution,
            });
        }
    }
    let p_value = if scored == 0 {
        1.0
    } else {
        chi2_sf(chi2, CHI2_DF)
    };
    BenfordStats {
        scored,
        excluded,
        observed,
        chi2,
        p_value,
        flags,
    }
}

pub fn fixture_schema() -> Schema {
    Schema::new(vec![
        Field::new("transaction_id", DataType::Utf8, false),
        Field::new("company_name", DataType::Utf8, false),
        Field::new("owner_name", DataType::Utf8, false),
        Field::new("amount_usd", DataType::Float64, true),
        Field::new("date", DataType::Utf8, false),
        Field::new("district", DataType::Utf8, false),
    ])
}

pub fn parse_csv_bytes(bytes: &[u8]) -> Result<Vec<RecordBatch>, BenfordError> {
    let reader = ReaderBuilder::new(Arc::new(fixture_schema()))
        .with_header(true)
        .build(Cursor::new(bytes))?;
    let mut batches = Vec::new();
    for batch in reader {
        batches.push(batch?);
    }
    Ok(batches)
}

pub fn collect_amounts(batches: &[RecordBatch]) -> Result<(Vec<f64>, usize, usize), BenfordError> {
    let mut amounts = Vec::new();
    let mut rows_total = 0usize;
    let mut null_count = 0usize;
    for batch in batches {
        rows_total += batch.num_rows();
        let column = batch
            .column_by_name("amount_usd")
            .ok_or_else(|| BenfordError::Schema("amount_usd column missing".into()))?;
        let values = column
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| BenfordError::Schema("amount_usd is not Float64".into()))?;
        for value in values {
            if let Some(v) = value {
                amounts.push(v);
            } else {
                null_count += 1;
            }
        }
    }
    Ok((amounts, rows_total, null_count))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn read_fixture_amounts(path: &Path) -> Result<(Vec<f64>, usize, usize), BenfordError> {
    let bytes = fs::read(path)?;
    let batches = parse_csv_bytes(&bytes)?;
    collect_amounts(&batches)
}

fn format_p(p: f64) -> String {
    if p == 0.0 {
        "< 1e-300".to_string()
    } else {
        format!("{p:.6e}")
    }
}

pub fn execute(path: &Path, allow_unsealed: bool) -> i32 {
    println!("TESSERA // BENFORD FIRST-DIGIT GATE (SQA spine)");
    println!("file   = {}", path.display());
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ERROR: cannot read fixture: {e}");
            return 2;
        }
    };
    let seal = sha256_hex(&bytes);
    println!("sha256 = {seal}");
    if seal == SEALED_FIXTURE_SHA256 {
        println!("chain-of-custody: PASS (sealed fixture, Manual s6.1)");
    } else if allow_unsealed {
        println!("chain-of-custody: WARN unsealed evidence under --allow-unsealed");
    } else {
        eprintln!("FORENSIC BREAK: sha256 mismatch vs sealed fixture {SEALED_FIXTURE_SHA256}");
        return 3;
    }
    let batches = match parse_csv_bytes(&bytes) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ERROR: CSV parse failed: {e}");
            return 2;
        }
    };
    let (amounts, rows_total, null_count) = match collect_amounts(&batches) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    let stats = analyze_amounts(&amounts);
    println!(
        "rows: total={rows_total} scored={} excluded={} null={null_count}",
        stats.scored, stats.excluded
    );
    println!("{:>5} {:>9} {:>10} {:>10}", "digit", "observed", "expected", "deviation");
    let n = stats.scored as f64;
    for (idx, &count) in stats.observed.iter().enumerate() {
        let expected = n * expected_proportion((idx + 1) as u32);
        println!(
            "{:>5} {:>9} {:>10.1} {:>+10.1}",
            idx + 1,
            count,
            expected,
            count as f64 - expected
        );
    }
    println!(
        "chi2 = {:.3}   df = {}   p-value = {}",
        stats.chi2,
        CHI2_DF,
        format_p(stats.p_value)
    );
    for flag in &stats.flags {
        println!(
            "FLAG | digit={} observed={} expected={:.1} deviation={:+.1} contribution={:.2}",
            flag.digit, flag.observed, flag.expected, flag.deviation, flag.contribution
        );
    }
    let verdict = if stats.p_value < P_VALUE_GATE {
        "DETECTABLE"
    } else {
        "NOT DETECTABLE"
    };
    println!(
        "FLAG | BENFORD verdict={verdict} p={} threshold={P_VALUE_GATE}",
        format_p(stats.p_value)
    );
    0
}
