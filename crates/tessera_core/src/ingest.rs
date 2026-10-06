use crate::benford::{
    analyze_amounts, parse_csv_bytes, sha256_hex, BenfordError, BenfordStats,
    SEALED_FIXTURE_SHA256,
};
use arrow::array::{Array, Float64Array, RecordBatch, StringArray};
use duckdb::{params, Connection};
use std::fmt;
use std::fs;
use std::path::Path;

pub const EXPECTED_SEALED_ROW_COUNT: usize = 5000;

pub const TENDERS_SCHEMA: [(&str, &str); 6] = [
    ("transaction_id", "VARCHAR"),
    ("company_name", "VARCHAR"),
    ("owner_name", "VARCHAR"),
    ("amount_usd", "DOUBLE"),
    ("date", "VARCHAR"),
    ("district", "VARCHAR"),
];

#[derive(Debug, Clone)]
pub struct TenderRow {
    pub transaction_id: String,
    pub company_name: String,
    pub owner_name: String,
    pub amount_usd: f64,
    pub date: String,
    pub district: String,
}

#[derive(Debug)]
pub enum IngestError {
    Io(std::io::Error),
    Benford(BenfordError),
    Duck(String),
    ForensicBreak { found: String },
    NullField { row: usize, column: &'static str },
    RowCount { expected: usize, found: usize },
}

impl fmt::Display for IngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Benford(e) => write!(f, "{e}"),
            Self::Duck(m) => write!(f, "DuckDB error: {m}"),
            Self::ForensicBreak { found } => {
                write!(f, "FORENSIC BREAK: sha256 {found} != sealed {SEALED_FIXTURE_SHA256}")
            }
            Self::NullField { row, column } => write!(f, "null field: row {row} column {column}"),
            Self::RowCount { expected, found } => {
                write!(f, "row count mismatch: expected {expected} found {found}")
            }
        }
    }
}

impl std::error::Error for IngestError {}

impl From<std::io::Error> for IngestError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<BenfordError> for IngestError {
    fn from(e: BenfordError) -> Self {
        Self::Benford(e)
    }
}

impl From<duckdb::Error> for IngestError {
    fn from(e: duckdb::Error) -> Self {
        Self::Duck(e.to_string())
    }
}

#[derive(Debug)]
pub struct IngestReport {
    pub sha256: String,
    pub row_count: usize,
    pub duckdb_count: i64,
    pub duckdb_version: String,
    pub stats: BenfordStats,
}

fn string_column<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<&'a StringArray, IngestError> {
    batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| IngestError::Benford(BenfordError::Schema(format!("{name} column missing"))))
}

fn non_null_cell(
    values: &StringArray,
    index: usize,
    column: &'static str,
    row: usize,
) -> Result<String, IngestError> {
    if values.is_null(index) {
        return Err(IngestError::NullField { row, column });
    }
    Ok(values.value(index).to_string())
}

pub fn extract_rows(batches: &[RecordBatch]) -> Result<Vec<TenderRow>, IngestError> {
    let mut rows = Vec::new();
    let mut row_index = 0usize;
    for batch in batches {
        let ids = string_column(batch, "transaction_id")?;
        let companies = string_column(batch, "company_name")?;
        let owners = string_column(batch, "owner_name")?;
        let dates = string_column(batch, "date")?;
        let districts = string_column(batch, "district")?;
        let amounts = batch
            .column_by_name("amount_usd")
            .and_then(|column| column.as_any().downcast_ref::<Float64Array>())
            .ok_or_else(|| {
                IngestError::Benford(BenfordError::Schema("amount_usd column missing".into()))
            })?;
        for index in 0..batch.num_rows() {
            row_index += 1;
            let amount_usd = if amounts.is_null(index) {
                return Err(IngestError::NullField {
                    row: row_index,
                    column: "amount_usd",
                });
            } else {
                amounts.value(index)
            };
            rows.push(TenderRow {
                transaction_id: non_null_cell(ids, index, "transaction_id", row_index)?,
                company_name: non_null_cell(companies, index, "company_name", row_index)?,
                owner_name: non_null_cell(owners, index, "owner_name", row_index)?,
                amount_usd,
                date: non_null_cell(dates, index, "date", row_index)?,
                district: non_null_cell(districts, index, "district", row_index)?,
            });
        }
    }
    Ok(rows)
}

pub fn load_into_duckdb(rows: &[TenderRow]) -> Result<(i64, String), IngestError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE tenders (
            transaction_id VARCHAR NOT NULL,
            company_name VARCHAR NOT NULL,
            owner_name VARCHAR NOT NULL,
            amount_usd DOUBLE NOT NULL,
            date VARCHAR NOT NULL,
            district VARCHAR NOT NULL
         );",
    )?;
    {
        let mut appender = conn.appender("tenders")?;
        for row in rows {
            appender.append_row(params![
                row.transaction_id.as_str(),
                row.company_name.as_str(),
                row.owner_name.as_str(),
                row.amount_usd,
                row.date.as_str(),
                row.district.as_str(),
            ])?;
        }
        appender.flush()?;
    }
    let count: i64 = conn.query_row("SELECT count(*) FROM tenders", [], |row| row.get(0))?;
    let version: String = conn.query_row("SELECT version()", [], |row| row.get(0))?;
    Ok((count, version))
}

pub fn ingest_bytes(bytes: &[u8], enforce_seal: bool) -> Result<IngestReport, IngestError> {
    let sha256 = sha256_hex(bytes);
    if enforce_seal && sha256 != SEALED_FIXTURE_SHA256 {
        return Err(IngestError::ForensicBreak { found: sha256 });
    }
    let batches = parse_csv_bytes(bytes)?;
    let rows = extract_rows(&batches)?;
    if enforce_seal && rows.len() != EXPECTED_SEALED_ROW_COUNT {
        return Err(IngestError::RowCount {
            expected: EXPECTED_SEALED_ROW_COUNT,
            found: rows.len(),
        });
    }
    let amounts: Vec<f64> = rows.iter().map(|row| row.amount_usd).collect();
    let stats = analyze_amounts(&amounts);
    let (duckdb_count, duckdb_version) = load_into_duckdb(&rows)?;
    if duckdb_count != rows.len() as i64 {
        return Err(IngestError::RowCount {
            expected: rows.len(),
            found: duckdb_count as usize,
        });
    }
    Ok(IngestReport {
        sha256,
        row_count: rows.len(),
        duckdb_count,
        duckdb_version,
        stats,
    })
}

pub fn ingest_file(path: &Path, allow_unsealed: bool) -> Result<IngestReport, IngestError> {
    let bytes = fs::read(path)?;
    ingest_bytes(&bytes, !allow_unsealed)
}

pub fn execute(path: &Path, allow_unsealed: bool) -> i32 {
    println!("TESSERA // P1-002 INGEST GATE (Arrow -> DuckDB bundled, zero-copy buffers)");
    println!("file   = {}", path.display());
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("ERROR: cannot read fixture: {e}");
            return 2;
        }
    };
    let sha256 = sha256_hex(&bytes);
    println!("sha256 = {sha256}");
    if sha256 == SEALED_FIXTURE_SHA256 {
        println!("chain-of-custody: PASS (sealed fixture, Manual s6.1)");
    } else if allow_unsealed {
        println!("chain-of-custody: WARN unsealed evidence under --allow-unsealed");
    } else {
        eprintln!("FORENSIC BREAK: sha256 mismatch vs sealed fixture {SEALED_FIXTURE_SHA256}");
        return 3;
    }
    let report = match ingest_bytes(&bytes, false) {
        Ok(report) => report,
        Err(e @ IngestError::ForensicBreak { .. }) => {
            eprintln!("{e}");
            return 3;
        }
        Err(e @ IngestError::RowCount { .. }) => {
            eprintln!("FORENSIC BREAK: {e}");
            return 3;
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    println!(
        "arrow: rows={} scored={} excluded={}",
        report.row_count, report.stats.scored, report.stats.excluded
    );
    println!(
        "duckdb: engine {} | CREATE TABLE tenders OK | appended={} | SELECT count(*)={}",
        report.duckdb_version, report.row_count, report.duckdb_count
    );
    let schema: Vec<String> = TENDERS_SCHEMA
        .iter()
        .map(|(name, kind)| format!("{name} {kind}"))
        .collect();
    println!("schema: {}", schema.join(" | "));
    let digits: Vec<String> = report
        .stats
        .observed
        .iter()
        .enumerate()
        .map(|(index, count)| format!("{}:{count}", index + 1))
        .collect();
    println!(
        "first-digit: {} | chi2={:.3} p={:.6e}",
        digits.join(" "),
        report.stats.chi2,
        report.stats.p_value
    );
    println!(
        "INGEST | rows={} | sha256={} | verdict=PASS",
        report.row_count, report.sha256
    );
    0
}
