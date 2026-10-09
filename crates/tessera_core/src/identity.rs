use crate::benford::{parse_csv_bytes, sha256_hex, BenfordError, SEALED_FIXTURE_SHA256};
use arrow::array::{Array, Float64Array, RecordBatch, StringArray};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_JW_THRESHOLD: f64 = 0.92;
pub const LEV_FLOOR: usize = 2;
pub const LEV_DIVISOR: usize = 8;
pub const WINKLER_PREFIX_LIMIT: usize = 4;
pub const WINKLER_SCALING: f64 = 0.1;
pub const MIN_TOKENS_AFTER_STRIP: usize = 2;
pub const EXPECTED_ALIAS_CLUSTER_COUNT: usize = 4;
pub const EXPECTED_SEALED_ROW_COUNT: usize = 5000;
pub const EXPECTED_SEALED_COMPANY_COUNT: usize = 4960;
pub const EXPECTED_SEALED_SUPER_NODE_COUNT: usize = 4956;
pub const EXPECTED_SEALED_BLOCKING_PAIR_COUNT: usize = 4;

pub const LEGAL_SUFFIXES: [&str; 13] = [
    "ltd",
    "limited",
    "llc",
    "inc",
    "incorporated",
    "corp",
    "corporation",
    "co",
    "company",
    "plc",
    "gmbh",
    "pvt",
    "private",
];

pub const ABBREVIATION_EXPANSIONS: [(&str, &str); 16] = [
    ("bldrs", "builders"),
    ("bldr", "builder"),
    ("serv", "services"),
    ("svcs", "services"),
    ("agro", "agricultural"),
    ("agric", "agricultural"),
    ("constr", "construction"),
    ("mgmt", "management"),
    ("tech", "technology"),
    ("equip", "equipment"),
    ("comm", "commercial"),
    ("trdg", "trading"),
    ("trans", "transport"),
    ("intl", "international"),
    ("dev", "developments"),
    ("eng", "engineering"),
];

const BANNER: &str = "TESSERA // P1-004 IDENTITY GATE (alias resolution + Super-Node collapse)";

#[derive(Debug, Clone, PartialEq)]
pub struct TenderRow {
    pub transaction_id: String,
    pub company_name: String,
    pub owner_name: String,
    pub amount_usd: f64,
    pub date: String,
    pub district: String,
}

impl TenderRow {
    pub fn new(
        transaction_id: &str,
        company_name: &str,
        owner_name: &str,
        amount_usd: f64,
        date: &str,
        district: &str,
    ) -> Self {
        Self {
            transaction_id: transaction_id.to_string(),
            company_name: company_name.to_string(),
            owner_name: owner_name.to_string(),
            amount_usd,
            date: date.to_string(),
            district: district.to_string(),
        }
    }
}

pub trait FromArrowRow: Sized {
    fn from_arrow(
        transaction_id: &str,
        company_name: &str,
        owner_name: &str,
        amount_usd: f64,
        date: &str,
        district: &str,
    ) -> Self;
}

impl FromArrowRow for TenderRow {
    fn from_arrow(
        transaction_id: &str,
        company_name: &str,
        owner_name: &str,
        amount_usd: f64,
        date: &str,
        district: &str,
    ) -> Self {
        Self::new(
            transaction_id,
            company_name,
            owner_name,
            amount_usd,
            date,
            district,
        )
    }
}

#[cfg(feature = "db")]
impl FromArrowRow for crate::ingest::TenderRow {
    fn from_arrow(
        transaction_id: &str,
        company_name: &str,
        owner_name: &str,
        amount_usd: f64,
        date: &str,
        district: &str,
    ) -> Self {
        Self {
            transaction_id: transaction_id.to_string(),
            company_name: company_name.to_string(),
            owner_name: owner_name.to_string(),
            amount_usd,
            date: date.to_string(),
            district: district.to_string(),
        }
    }
}

#[cfg(feature = "db")]
impl From<crate::ingest::TenderRow> for TenderRow {
    fn from(row: crate::ingest::TenderRow) -> Self {
        Self::new(
            &row.transaction_id,
            &row.company_name,
            &row.owner_name,
            row.amount_usd,
            &row.date,
            &row.district,
        )
    }
}

#[cfg(feature = "db")]
pub fn from_ingest_rows(rows: &[crate::ingest::TenderRow]) -> Vec<TenderRow> {
    rows.iter().cloned().map(TenderRow::from).collect()
}

#[derive(Debug)]
pub enum RowError {
    NullField { row: usize, column: &'static str },
    MissingColumn(&'static str),
}

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullField { row, column } => write!(f, "null field: row {row} column {column}"),
            Self::MissingColumn(name) => write!(f, "column missing from record batch: {name}"),
        }
    }
}

impl std::error::Error for RowError {}

#[derive(Debug)]
pub enum IdentityError {
    Io(std::io::Error),
    Benford(BenfordError),
    Row(RowError),
    ForensicBreak { found: String },
    BadThreshold(String),
    BadFlag(String),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Benford(e) => write!(f, "{e}"),
            Self::Row(e) => write!(f, "{e}"),
            Self::ForensicBreak { found } => write!(
                f,
                "FORENSIC BREAK: sha256 {found} != sealed {SEALED_FIXTURE_SHA256}"
            ),
            Self::BadThreshold(v) => write!(f, "invalid --threshold value: {v}"),
            Self::BadFlag(v) => write!(f, "invalid argument: {v}"),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<std::io::Error> for IdentityError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<BenfordError> for IdentityError {
    fn from(e: BenfordError) -> Self {
        Self::Benford(e)
    }
}

impl From<RowError> for IdentityError {
    fn from(e: RowError) -> Self {
        Self::Row(e)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AliasCluster {
    pub canonical: String,
    pub members: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateTier {
    T1,
    T1Near,
    T2,
    Reject,
    RejectEmpty,
    RejectNumeric,
}

impl GateTier {
    pub const fn label(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T1Near => "T1-NEAR",
            Self::T2 => "T2",
            Self::Reject => "REJECT",
            Self::RejectEmpty => "REJECT-EMPTY",
            Self::RejectNumeric => "REJECT-NUMERIC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AliasGate {
    pub merged: bool,
    pub tier: GateTier,
    pub jw: f64,
    pub lev: usize,
    pub budget: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergeRecord {
    pub left: String,
    pub right: String,
    pub owner: String,
    pub district: String,
    pub gate: AliasGate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AliasResolution {
    pub clusters: Vec<AliasCluster>,
    pub merges: Vec<MergeRecord>,
    pub company_count: usize,
    pub super_node_count: usize,
    pub block_count: usize,
    pub candidate_pair_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AliasCounts {
    pub companies: usize,
    pub super_nodes: usize,
    pub clusters: usize,
    pub merged_members: usize,
    pub blocks: usize,
    pub candidate_pairs: usize,
}

fn is_legal_suffix(token: &str) -> bool {
    LEGAL_SUFFIXES.contains(&token)
}

fn expand_abbreviation(token: &str) -> &str {
    for (abbrev, full) in ABBREVIATION_EXPANSIONS {
        if abbrev == token {
            return full;
        }
    }
    token
}

pub fn tokenize_name(value: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn normalize_core(value: &str) -> String {
    let tokens = tokenize_name(value);
    if tokens.is_empty() {
        return String::new();
    }
    let expanded: Vec<String> = tokens
        .iter()
        .map(|token| expand_abbreviation(token).to_string())
        .collect();
    let mut kept: Vec<&str> = expanded.iter().map(String::as_str).collect();
    while kept.len() > MIN_TOKENS_AFTER_STRIP && is_legal_suffix(kept[kept.len() - 1]) {
        kept.pop();
    }
    kept.join(" ")
}

pub fn normalize_owner(value: &str) -> String {
    normalize_core(value)
}

pub fn normalize_company(value: &str) -> String {
    normalize_core(value)
}

pub fn lev_budget(longer_len: usize) -> usize {
    LEV_FLOOR.max(longer_len / LEV_DIVISOR)
}

pub fn numeral_tokens(normalized: &str) -> Vec<&str> {
    normalized
        .split(' ')
        .filter(|token| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
        .collect()
}

pub fn jaro(a: &str, b: &str) -> f64 {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let left_len = left.len();
    let right_len = right.len();
    let window = (left_len.max(right_len) / 2).saturating_sub(1);
    let mut left_matched = vec![false; left_len];
    let mut right_matched = vec![false; right_len];
    let mut matches: usize = 0;
    for i in 0..left_len {
        let low = i.saturating_sub(window);
        let high = (i + window).min(right_len - 1);
        if low > high {
            continue;
        }
        for j in low..=high {
            if !right_matched[j] && right[j] == left[i] {
                right_matched[j] = true;
                left_matched[i] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let mut transpositions: usize = 0;
    let mut k: usize = 0;
    for i in 0..left_len {
        if !left_matched[i] {
            continue;
        }
        while !right_matched[k] {
            k += 1;
        }
        if left[i] != right[k] {
            transpositions += 1;
        }
        k += 1;
    }
    let m = matches as f64;
    let t = (transpositions as f64) / 2.0;
    (m / (left_len as f64) + m / (right_len as f64) + (m - t) / m) / 3.0
}

pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    let base = jaro(a, b);
    if base <= 0.0 {
        return 0.0;
    }
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let limit = WINKLER_PREFIX_LIMIT.min(left.len()).min(right.len());
    let mut prefix: usize = 0;
    for i in 0..limit {
        if left[i] != right[i] {
            break;
        }
        prefix += 1;
    }
    (base + (prefix as f64) * WINKLER_SCALING * (1.0 - base)).clamp(0.0, 1.0)
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current: Vec<usize> = vec![0; right.len() + 1];
    for i in 1..=left.len() {
        current[0] = i;
        for j in 1..=right.len() {
            let cost = usize::from(left[i - 1] != right[j - 1]);
            current[j] = (previous[j] + 1).min(current[j - 1] + 1).min(previous[j - 1] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

pub fn alias_gate(left: &str, right: &str, threshold: f64) -> AliasGate {
    let normalized_left = normalize_company(left);
    let normalized_right = normalize_company(right);
    if normalized_left.is_empty() || normalized_right.is_empty() {
        return AliasGate {
            merged: false,
            tier: GateTier::RejectEmpty,
            jw: 0.0,
            lev: 0,
            budget: 0,
        };
    }
    let threshold = if threshold.is_finite() {
        threshold.clamp(0.0, 1.0)
    } else {
        DEFAULT_JW_THRESHOLD
    };
    let jw = jaro_winkler(&normalized_left, &normalized_right);
    let lev = levenshtein(&normalized_left, &normalized_right);
    let budget = lev_budget(normalized_left.len().max(normalized_right.len()));
    if !(jw >= threshold) || lev > budget {
        return AliasGate {
            merged: false,
            tier: GateTier::Reject,
            jw,
            lev,
            budget,
        };
    }
    if numeral_tokens(&normalized_left) != numeral_tokens(&normalized_right) {
        return AliasGate {
            merged: false,
            tier: GateTier::RejectNumeric,
            jw,
            lev,
            budget,
        };
    }
    let tier = if normalized_left == normalized_right {
        GateTier::T1
    } else if lev <= 1 {
        GateTier::T1Near
    } else {
        GateTier::T2
    };
    AliasGate {
        merged: true,
        tier,
        jw,
        lev,
        budget,
    }
}

#[derive(Debug, Default)]
struct DisjointSet {
    parent: BTreeMap<String, String>,
    rank: BTreeMap<String, usize>,
}

impl DisjointSet {
    fn insert(&mut self, key: &str) {
        self.parent
            .entry(key.to_string())
            .or_insert_with(|| key.to_string());
        self.rank.entry(key.to_string()).or_insert(0);
    }

    fn find(&mut self, key: &str) -> String {
        self.insert(key);
        let mut root = key.to_string();
        loop {
            let next = match self.parent.get(&root) {
                Some(value) if value != &root => value.clone(),
                _ => break,
            };
            root = next;
        }
        let mut cursor = key.to_string();
        while cursor != root {
            let next = match self.parent.get(&cursor) {
                Some(value) => value.clone(),
                None => break,
            };
            self.parent.insert(cursor, root.clone());
            cursor = next;
        }
        root
    }

    fn union(&mut self, left: &str, right: &str) -> bool {
        let root_left = self.find(left);
        let root_right = self.find(right);
        if root_left == root_right {
            return false;
        }
        let rank_left = self.rank.get(&root_left).copied().unwrap_or(0);
        let rank_right = self.rank.get(&root_right).copied().unwrap_or(0);
        match rank_left.cmp(&rank_right) {
            std::cmp::Ordering::Less => {
                self.parent.insert(root_left, root_right);
            }
            std::cmp::Ordering::Greater => {
                self.parent.insert(root_right, root_left);
            }
            std::cmp::Ordering::Equal => {
                self.parent.insert(root_right, root_left.clone());
                self.rank.insert(root_left, rank_left + 1);
            }
        }
        true
    }
}

pub fn build_blocks(rows: &[TenderRow]) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut blocks: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        blocks
            .entry((normalize_owner(&row.owner_name), row.district.clone()))
            .or_default()
            .insert(row.company_name.clone());
    }
    blocks
}

pub fn resolve(rows: &[TenderRow], threshold: f64) -> AliasResolution {
    let threshold = if threshold.is_finite() {
        threshold.clamp(0.0, 1.0)
    } else {
        DEFAULT_JW_THRESHOLD
    };
    let mut companies: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        companies.insert(row.company_name.clone());
    }
    let blocks = build_blocks(rows);
    let mut set = DisjointSet::default();
    for company in &companies {
        set.insert(company);
    }
    let mut candidate_pair_count: usize = 0;
    let mut merges: Vec<MergeRecord> = Vec::new();
    for ((owner, district), members) in &blocks {
        if members.len() < 2 {
            continue;
        }
        let ordered: Vec<&String> = members.iter().collect();
        for i in 0..ordered.len() {
            for j in (i + 1)..ordered.len() {
                candidate_pair_count += 1;
                let left = ordered[i].as_str();
                let right = ordered[j].as_str();
                let gate = alias_gate(left, right, threshold);
                if gate.merged {
                    set.union(left, right);
                    merges.push(MergeRecord {
                        left: left.to_string(),
                        right: right.to_string(),
                        owner: owner.clone(),
                        district: district.clone(),
                        gate,
                    });
                }
            }
        }
    }
    let mut grouped: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for company in &companies {
        let root = set.find(company);
        grouped.entry(root).or_default().insert(company.clone());
    }
    let mut clusters: Vec<AliasCluster> = Vec::new();
    for (_, members) in grouped {
        if members.len() < 2 {
            continue;
        }
        let canonical = members.iter().min().cloned().unwrap_or_default();
        clusters.push(AliasCluster { canonical, members });
    }
    clusters.sort_by(|a, b| a.canonical.cmp(&b.canonical));
    merges.sort_by(|a, b| {
        a.left
            .cmp(&b.left)
            .then_with(|| a.right.cmp(&b.right))
            .then_with(|| a.district.cmp(&b.district))
    });
    let merged_members: usize = clusters.iter().map(|cluster| cluster.members.len()).sum();
    let collapsed = merged_members.saturating_sub(clusters.len());
    AliasResolution {
        clusters,
        merges,
        company_count: companies.len(),
        super_node_count: companies.len().saturating_sub(collapsed),
        block_count: blocks.len(),
        candidate_pair_count,
    }
}

pub fn resolve_aliases(rows: &[TenderRow]) -> Vec<AliasCluster> {
    resolve(rows, DEFAULT_JW_THRESHOLD).clusters
}

pub fn resolve_aliases_with_threshold(rows: &[TenderRow], threshold: f64) -> Vec<AliasCluster> {
    resolve(rows, threshold).clusters
}

pub fn build_alias_map(clusters: &[AliasCluster]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for cluster in clusters {
        for member in &cluster.members {
            map.insert(member.clone(), cluster.canonical.clone());
        }
    }
    map
}

pub fn canonical_of<'a>(map: &'a HashMap<String, String>, name: &'a str) -> &'a str {
    map.get(name).map_or(name, String::as_str)
}

pub fn counts_of(resolution: &AliasResolution) -> AliasCounts {
    let merged_members: usize = resolution
        .clusters
        .iter()
        .map(|cluster| cluster.members.len())
        .sum();
    AliasCounts {
        companies: resolution.company_count,
        super_nodes: resolution.super_node_count,
        clusters: resolution.clusters.len(),
        merged_members,
        blocks: resolution.block_count,
        candidate_pairs: resolution.candidate_pair_count,
    }
}

fn cluster_gate_summary(cluster: &AliasCluster, threshold: f64) -> (f64, usize, usize) {
    let mut jw_min = 1.0f64;
    let mut lev_max: usize = 0;
    let mut budget_max: usize = 0;
    for member in &cluster.members {
        if member == &cluster.canonical {
            continue;
        }
        let gate = alias_gate(&cluster.canonical, member, threshold);
        jw_min = jw_min.min(gate.jw);
        lev_max = lev_max.max(gate.lev);
        budget_max = budget_max.max(gate.budget);
    }
    (jw_min, lev_max, budget_max)
}

fn string_column<'a>(batch: &'a RecordBatch, name: &'static str) -> Result<&'a StringArray, RowError> {
    batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .ok_or(RowError::MissingColumn(name))
}

fn text_cell(
    values: &StringArray,
    index: usize,
    column: &'static str,
    row: usize,
) -> Result<String, RowError> {
    if values.is_null(index) {
        return Err(RowError::NullField { row, column });
    }
    Ok(values.value(index).to_string())
}

pub fn parse_rows<R: FromArrowRow>(batches: &[RecordBatch]) -> Result<Vec<R>, RowError> {
    let mut rows: Vec<R> = Vec::new();
    let mut row_index: usize = 0;
    for batch in batches {
        let ids = string_column(batch, "transaction_id")?;
        let companies = string_column(batch, "company_name")?;
        let owners = string_column(batch, "owner_name")?;
        let dates = string_column(batch, "date")?;
        let districts = string_column(batch, "district")?;
        let amounts = batch
            .column_by_name("amount_usd")
            .and_then(|column| column.as_any().downcast_ref::<Float64Array>())
            .ok_or(RowError::MissingColumn("amount_usd"))?;
        for index in 0..batch.num_rows() {
            row_index += 1;
            if amounts.is_null(index) {
                return Err(RowError::NullField {
                    row: row_index,
                    column: "amount_usd",
                });
            }
            rows.push(R::from_arrow(
                &text_cell(ids, index, "transaction_id", row_index)?,
                &text_cell(companies, index, "company_name", row_index)?,
                &text_cell(owners, index, "owner_name", row_index)?,
                amounts.value(index),
                &text_cell(dates, index, "date", row_index)?,
                &text_cell(districts, index, "district", row_index)?,
            ));
        }
    }
    Ok(rows)
}

pub fn load_rows(path: &Path, allow_unsealed: bool) -> Result<(String, Vec<TenderRow>), IdentityError> {
    let bytes = fs::read(path)?;
    let sha256 = sha256_hex(&bytes);
    if sha256 != SEALED_FIXTURE_SHA256 && !allow_unsealed {
        return Err(IdentityError::ForensicBreak { found: sha256 });
    }
    let batches = parse_csv_bytes(&bytes)?;
    let rows: Vec<TenderRow> = parse_rows(&batches)?;
    Ok((sha256, rows))
}

fn parse_cli(args: &[String]) -> Result<(Option<String>, f64, bool), IdentityError> {
    let mut csv: Option<String> = None;
    let mut threshold = DEFAULT_JW_THRESHOLD;
    let mut allow_unsealed = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--csv" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| IdentityError::BadFlag("--csv requires a path".to_string()))?;
                csv = Some(value.clone());
            }
            "--threshold" => {
                index += 1;
                let raw = args
                    .get(index)
                    .ok_or_else(|| IdentityError::BadFlag("--threshold requires a value".to_string()))?;
                let parsed: f64 = raw
                    .parse()
                    .map_err(|_| IdentityError::BadThreshold(raw.clone()))?;
                if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
                    return Err(IdentityError::BadThreshold(raw.clone()));
                }
                threshold = parsed;
            }
            "--allow-unsealed" => allow_unsealed = true,
            other => {
                if other.starts_with("--") {
                    return Err(IdentityError::BadFlag(other.to_string()));
                }
                csv = Some(other.to_string());
            }
        }
        index += 1;
    }
    Ok((csv, threshold, allow_unsealed))
}

pub fn execute(args: &[String]) -> i32 {
    println!("{BANNER}");
    let (csv, threshold, allow_unsealed) = match parse_cli(args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    let path: PathBuf = csv
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(crate::benford::DEFAULT_FIXTURE));
    println!("file   = {}", path.display());
    let bytes = match fs::read(&path) {
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
    let rows: Vec<TenderRow> = match parse_rows(&batches) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return 2;
        }
    };
    let resolution = resolve(&rows, threshold);
    let counts = counts_of(&resolution);
    println!(
        "rows={} distinct_companies={} blocks={} candidate_pairs={}",
        rows.len(),
        counts.companies,
        counts.blocks,
        counts.candidate_pairs
    );
    println!("normalization: lowercase -> ascii-alnum tokenize -> abbreviation expansion -> trailing legal-form strip (floor {MIN_TOKENS_AFTER_STRIP} tokens)");
    println!(
        "gate: jaro_winkler >= {threshold:.2} AND levenshtein <= max({LEV_FLOOR}, canonical_len/{LEV_DIVISOR}) AND numeral-token identity"
    );
    println!("blocking: (normalize_owner(owner_name), district) - load-bearing; unblocked precision is 0.004 on this fixture");
    println!("note: CLUSTER-nn is DETERMINISTIC OUTPUT ORDER (sorted by canonical), NOT the injection_manifest alias_id");
    for (index, cluster) in resolution.clusters.iter().enumerate() {
        let (jw_min, lev_max, budget_max) = cluster_gate_summary(cluster, threshold);
        let members: Vec<&String> = cluster.members.iter().collect();
        println!(
            "CLUSTER-{:02} | canonical={} | members={} | jw_min={:.6} lev_max={} budget={}",
            index + 1,
            cluster.canonical,
            members.len(),
            jw_min,
            lev_max,
            budget_max
        );
        for member in members {
            let role = if *member == &cluster.canonical { "canonical" } else { "alias" };
            println!("    - {member} [{role}]");
        }
    }
    for merge in &resolution.merges {
        println!(
            "MERGE | {} + {} | owner={} district={} tier={} jw={:.6} lev={} budget={}",
            merge.left,
            merge.right,
            merge.owner,
            merge.district,
            merge.gate.tier.label(),
            merge.gate.jw,
            merge.gate.lev,
            merge.gate.budget
        );
    }
    let alias_map = build_alias_map(&resolution.clusters);
    let collapsed = counts.merged_members.saturating_sub(counts.clusters);
    println!(
        "COLLAPSE | raw_companies={} super_nodes={} collapsed={}",
        counts.companies, counts.super_nodes, collapsed
    );
    println!(
        "SUPER-NODE | clusters={} merged_members={} alias_map_entries={} blocks={} candidate_pairs={}",
        counts.clusters, counts.merged_members, alias_map.len(), counts.blocks, counts.candidate_pairs
    );
    println!("CLUSTERS: {}", counts.clusters);
    let verdict = if counts.clusters == EXPECTED_ALIAS_CLUSTER_COUNT { "PASS" } else { "FAIL" };
    println!(
        "IDENTITY | clusters={} expected={} threshold={threshold:.2} verdict={verdict}",
        counts.clusters, EXPECTED_ALIAS_CLUSTER_COUNT
    );
    if verdict == "PASS" {
        0
    } else {
        3
    }
}
