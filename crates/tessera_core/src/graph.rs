use crate::benford::{parse_csv_bytes, sha256_hex, BenfordError, SEALED_FIXTURE_SHA256};
use crate::ingest::{extract_rows, TenderRow};
use kuzu::{Connection, Database, SystemConfig, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;

pub const EXPECTED_RING_COUNT: usize = 3;
pub const RING_WINDOW_DAYS: i64 = 7;
const STATEMENT_CHUNK: usize = 64;

pub const SCHEMA_STATEMENTS: [&str; 8] = [
    "CREATE NODE TABLE Person(name STRING, PRIMARY KEY(name))",
    "CREATE NODE TABLE Company(name STRING, PRIMARY KEY(name))",
    "CREATE NODE TABLE BankAccount(id STRING, PRIMARY KEY(id))",
    "CREATE NODE TABLE Tender(id STRING, amount DOUBLE, date STRING, district STRING, PRIMARY KEY(id))",
    "CREATE REL TABLE OWNED_BY(FROM Company TO Person)",
    "CREATE REL TABLE BID_ON(FROM Company TO Tender)",
    "CREATE REL TABLE TRANSFERRED_TO(FROM Company TO Company, amount DOUBLE, date STRING, tx_id STRING)",
    "CREATE REL TABLE SHARES_ADDRESS(FROM Company TO Company, district STRING)",
];

pub const CIRCULAR_FLOW_TEMPLATE: &str = "MATCH (a:Company)-[t1:TRANSFERRED_TO]->(b:Company)-[t2:TRANSFERRED_TO]->(c:Company)-[t3:TRANSFERRED_TO]->(a) WHERE a.name < b.name AND a.name < c.name RETURN a.name, b.name, c.name, t1.tx_id, t2.tx_id, t3.tx_id, t1.date, t2.date, t3.date";

#[derive(Debug)]
pub enum GraphError {
    Io(std::io::Error),
    Benford(BenfordError),
    Kuzu(kuzu::Error),
    Ingest(crate::ingest::IngestError),
    MalformedResult(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Benford(e) => write!(f, "{e}"),
            Self::Kuzu(e) => write!(f, "Kuzu error: {e:?}"),
            Self::Ingest(e) => write!(f, "{e}"),
            Self::MalformedResult(m) => write!(f, "malformed query result: {m}"),
        }
    }
}

impl std::error::Error for GraphError {}

impl From<std::io::Error> for GraphError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<BenfordError> for GraphError {
    fn from(e: BenfordError) -> Self {
        Self::Benford(e)
    }
}

impl From<kuzu::Error> for GraphError {
    fn from(e: kuzu::Error) -> Self {
        Self::Kuzu(e)
    }
}

impl From<crate::ingest::IngestError> for GraphError {
    fn from(e: crate::ingest::IngestError) -> Self {
        Self::Ingest(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphCounts {
    pub companies: usize,
    pub persons: usize,
    pub tenders: usize,
    pub owned_by: usize,
    pub bid_on: usize,
    pub transferred_to: usize,
    pub shares_address: usize,
}

#[derive(Debug, Clone)]
pub struct RingHit {
    pub ring_id: String,
    pub companies: [String; 3],
    pub tx_ids: [String; 3],
    pub dates: [String; 3],
    pub amounts: [f64; 3],
    pub span_days: i64,
}

pub fn cypher_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era =
        year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub fn parse_iso_days(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn run_chunked(conn: &Connection, statements: &[String]) -> Result<(), GraphError> {
    for chunk in statements.chunks(STATEMENT_CHUNK) {
        conn.query(&chunk.join("; "))?;
    }
    Ok(())
}

pub fn load_fixture(conn: &Connection, rows: &[TenderRow]) -> Result<GraphCounts, GraphError> {
    for statement in SCHEMA_STATEMENTS {
        conn.query(statement)?;
    }

    let mut companies: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        companies.insert(row.company_name.clone());
    }

    let mut persons: BTreeSet<String> = BTreeSet::new();
    let mut owned_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    let mut address_groups: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut transfer_statements: Vec<String> = Vec::new();
    let mut tender_statements: Vec<String> = Vec::with_capacity(rows.len());
    let mut bid_statements: Vec<String> = Vec::with_capacity(rows.len());

    for row in rows {
        tender_statements.push(format!(
            "CREATE (:Tender {{id:'{}', amount:{:.2}, date:'{}', district:'{}'}})",
            cypher_escape(&row.transaction_id),
            row.amount_usd,
            cypher_escape(&row.date),
            cypher_escape(&row.district)
        ));
        bid_statements.push(format!(
            "MATCH (c:Company {{name:'{}'}}), (t:Tender {{id:'{}'}}) CREATE (c)-[:BID_ON]->(t)",
            cypher_escape(&row.company_name),
            cypher_escape(&row.transaction_id)
        ));
        if companies.contains(&row.owner_name) {
            transfer_statements.push(format!(
                "MATCH (a:Company {{name:'{}'}}), (b:Company {{name:'{}'}}) CREATE (a)-[:TRANSFERRED_TO {{amount:{:.2}, date:'{}', tx_id:'{}'}}]->(b)",
                cypher_escape(&row.company_name),
                cypher_escape(&row.owner_name),
                row.amount_usd,
                cypher_escape(&row.date),
                cypher_escape(&row.transaction_id)
            ));
        } else {
            persons.insert(row.owner_name.clone());
            owned_pairs.insert((row.company_name.clone(), row.owner_name.clone()));
            address_groups
                .entry((row.owner_name.clone(), row.district.clone()))
                .or_default()
                .insert(row.company_name.clone());
        }
    }

    let company_statements: Vec<String> = companies
        .iter()
        .map(|name| format!("CREATE (:Company {{name:'{}'}})", cypher_escape(name)))
        .collect();
    let person_statements: Vec<String> = persons
        .iter()
        .map(|name| format!("CREATE (:Person {{name:'{}'}})", cypher_escape(name)))
        .collect();
    let owned_statements: Vec<String> = owned_pairs
        .iter()
        .map(|(company, person)| {
            format!(
                "MATCH (c:Company {{name:'{}'}}), (p:Person {{name:'{}'}}) CREATE (c)-[:OWNED_BY]->(p)",
                cypher_escape(company),
                cypher_escape(person)
            )
        })
        .collect();
    let mut shares_statements: Vec<String> = Vec::new();
    for ((_, district), members) in &address_groups {
        let members: Vec<&String> = members.iter().collect();
        for (i, left) in members.iter().enumerate() {
            for right in members.iter().skip(i + 1) {
                shares_statements.push(format!(
                    "MATCH (a:Company {{name:'{}'}}), (b:Company {{name:'{}'}}) CREATE (a)-[:SHARES_ADDRESS {{district:'{}'}}]->(b)",
                    cypher_escape(left),
                    cypher_escape(right),
                    cypher_escape(district)
                ));
            }
        }
    }

    run_chunked(conn, &company_statements)?;
    run_chunked(conn, &person_statements)?;
    run_chunked(conn, &tender_statements)?;
    run_chunked(conn, &owned_statements)?;
    run_chunked(conn, &bid_statements)?;
    run_chunked(conn, &transfer_statements)?;
    run_chunked(conn, &shares_statements)?;

    Ok(GraphCounts {
        companies: companies.len(),
        persons: persons.len(),
        tenders: rows.len(),
        owned_by: owned_pairs.len(),
        bid_on: rows.len(),
        transferred_to: transfer_statements.len(),
        shares_address: shares_statements.len(),
    })
}

pub fn detect_rings(conn: &Connection, rows: &[TenderRow]) -> Result<Vec<RingHit>, GraphError> {
    let tx_map: BTreeMap<&str, &TenderRow> =
        rows.iter().map(|row| (row.transaction_id.as_str(), row)).collect();
    let result = conn.query(CIRCULAR_FLOW_TEMPLATE)?;
    let mut hits: Vec<(i64, RingHit)> = Vec::new();
    for tuple in result {
        if tuple.len() != 9 {
            return Err(GraphError::MalformedResult(format!(
                "expected 9 columns, got {}",
                tuple.len()
            )));
        }
        let cells: Vec<String> = tuple.iter().map(Value::to_string).collect();
        let dates = [cells[6].clone(), cells[7].clone(), cells[8].clone()];
        let days: Vec<i64> = dates
            .iter()
            .map(|date| parse_iso_days(date).unwrap_or(i64::MAX))
            .collect();
        let span_days = days.iter().max().unwrap_or(&0) - days.iter().min().unwrap_or(&0);
        if span_days > RING_WINDOW_DAYS {
            continue;
        }
        let tx_ids = [cells[3].clone(), cells[4].clone(), cells[5].clone()];
        let mut amounts = [f64::NAN; 3];
        for (slot, tx_id) in tx_ids.iter().enumerate() {
            if let Some(row) = tx_map.get(tx_id.as_str()) {
                amounts[slot] = row.amount_usd;
            }
        }
        let sort_day = days.iter().min().copied().unwrap_or(i64::MAX);
        hits.push((
            sort_day,
            RingHit {
                ring_id: String::new(),
                companies: [cells[0].clone(), cells[1].clone(), cells[2].clone()],
                tx_ids,
                dates,
                amounts,
                span_days,
            },
        ));
    }
    hits.sort_by(|left, right| {
        left.0.cmp(&right.0)
            .then_with(|| left.1.companies.cmp(&right.1.companies))
    });
    let mut rings: Vec<RingHit> = Vec::with_capacity(hits.len());
    for (index, (_, mut hit)) in hits.into_iter().enumerate() {
        hit.ring_id = format!("RING-{:02}", index + 1);
        rings.push(hit);
    }
    Ok(rings)
}

pub fn execute(path: &Path, allow_unsealed: bool) -> i32 {
    println!("TESSERA // P1-003 CIRCULAR FLOW GATE (Kuzu bipartite, prebuilt contract)");
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
    let batches = match parse_csv_bytes(&bytes) {
        Ok(batches) => batches,
        Err(e) => {
            eprintln!("ERROR: CSV parse failed: {e}");
            return 2;
        }
    };
    let rows = match extract_rows(&batches) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    let database = match Database::in_memory(SystemConfig::default()) {
        Ok(database) => database,
        Err(e) => {
            eprintln!("ERROR: Kuzu database init failed: {e:?}");
            return 2;
        }
    };
    let conn = match Connection::new(&database) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("ERROR: Kuzu connection failed: {e:?}");
            return 2;
        }
    };
    let counts = match load_fixture(&conn, &rows) {
        Ok(counts) => counts,
        Err(e) => {
            eprintln!("ERROR: graph load failed: {e}");
            return 2;
        }
    };
    println!("kuzu: schema OK | 4 node tables (Person/Company/BankAccount/Tender) + 4 rel tables (OWNED_BY/BID_ON/TRANSFERRED_TO/SHARES_ADDRESS)");
    println!(
        "loaded: companies={} persons={} tenders={} | OWNED_BY={} BID_ON={} TRANSFERRED_TO={} SHARES_ADDRESS={}",
        counts.companies,
        counts.persons,
        counts.tenders,
        counts.owned_by,
        counts.bid_on,
        counts.transferred_to,
        counts.shares_address
    );
    let rings = match detect_rings(&conn, &rows) {
        Ok(rings) => rings,
        Err(e) => {
            eprintln!("ERROR: ring detection failed: {e}");
            return 2;
        }
    };
    for ring in &rings {
        println!(
            "{} | {} -> {} -> {} | week_of={} span={}d | tx={} | amounts={:.2},{:.2},{:.2}",
            ring.ring_id,
            ring.companies[0],
            ring.companies[1],
            ring.companies[2],
            ring.dates.iter().min().map(String::as_str).unwrap_or("?"),
            ring.span_days,
            ring.tx_ids.join(","),
            ring.amounts[0],
            ring.amounts[1],
            ring.amounts[2]
        );
    }
    let verdict = if rings.len() == EXPECTED_RING_COUNT {
        "PASS"
    } else {
        "FAIL"
    };
    println!(
        "RINGS | detected={} expected={} window={}d verdict={verdict}",
        rings.len(),
        EXPECTED_RING_COUNT,
        RING_WINDOW_DAYS
    );
    if verdict == "PASS" {
        0
    } else {
        3
    }
}
