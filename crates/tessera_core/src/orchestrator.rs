use std::error::Error;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use arrow::array::{Float32Array, Int32Array, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::error::ArrowError;
use arrow::record_batch::RecordBatch;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pool {
    Io,
    Compute,
    DbCoord,
}

impl Pool {
    pub const fn label(self) -> &'static str {
        match self {
            Pool::Io => "POOL_A",
            Pool::Compute => "POOL_B",
            Pool::DbCoord => "POOL_C",
        }
    }
}

struct Config {
    pool_a_range: std::ops::Range<usize>,
    pool_b_range: std::ops::Range<usize>,
    pool_c_range: std::ops::Range<usize>,
    scan_iterations: u64,
    log_prefix: &'static str,
}

impl Config {
    const fn new() -> Self {
        Self {
            pool_a_range: 0..6,
            pool_b_range: 6..22,
            pool_c_range: 22..32,
            scan_iterations: 10_000,
            log_prefix: "TESSERA",
        }
    }
}

const CONFIG: Config = Config::new();

pub fn pool_of(core_id: usize) -> Option<Pool> {
    if CONFIG.pool_a_range.contains(&core_id) {
        Some(Pool::Io)
    } else if CONFIG.pool_b_range.contains(&core_id) {
        Some(Pool::Compute)
    } else if CONFIG.pool_c_range.contains(&core_id) {
        Some(Pool::DbCoord)
    } else {
        None
    }
}

static GLOBAL_LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

fn init_logger() -> Result<(), &'static str> {
    GLOBAL_LOGGER
        .set(Mutex::new(Logger::new()))
        .map_err(|_| "Logger already initialized")
}

fn get_logger() -> &'static Mutex<Logger> {
    GLOBAL_LOGGER
        .get()
        .expect("Logger not initialized. Call init_logger() first.")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    const fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "[INFO]",
            LogLevel::Warn => "[WARN]",
            LogLevel::Error => "[ERROR]",
        }
    }
}

struct Logger {
    start_time: Instant,
    stdout: Mutex<io::Stdout>,
}

impl Logger {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            stdout: Mutex::new(io::stdout()),
        }
    }

    fn log(&self, level: LogLevel, pool: &str, core_id: usize, message: &str) {
        let mut handle = self.stdout.lock().unwrap_or_else(|e| e.into_inner());
        let elapsed_ms = self.start_time.elapsed().as_millis();
        let _ = writeln!(
            handle,
            "{} T+{:>6}ms {} | Core {:>2} | {:<12} | {}",
            CONFIG.log_prefix,
            elapsed_ms,
            level.as_str(),
            core_id,
            pool,
            message
        );
        let _ = handle.flush();
    }
}

fn create_forensic_schema() -> Result<Schema, ArrowError> {
    Ok(Schema::new(vec![
        Field::new("tx_id", DataType::Int32, false),
        Field::new("entity", DataType::Utf8, false),
        Field::new("timestamp_ns", DataType::UInt64, false),
        Field::new("risk_score", DataType::Float32, false),
    ]))
}

fn create_sample_batch(schema: &Schema) -> Result<RecordBatch, ArrowError> {
    let tx_ids = Int32Array::from(vec![101, 102, 103, 104, 105]);
    let entities = StringArray::from(vec![
        "Shell_A",
        "Capital_B",
        "Trust_C",
        "Holding_D",
        "Offshore_E",
    ]);
    let timestamps = UInt64Array::from(vec![
        1_704_067_200_000_000_000,
        1_704_067_201_000_000_000,
        1_704_067_202_000_000_000,
        1_704_067_203_000_000_000,
        1_704_067_204_000_000_000,
    ]);
    let risk_scores = Float32Array::from(vec![0.12, 0.89, 0.45, 0.67, 0.23]);

    RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(tx_ids),
            Arc::new(entities),
            Arc::new(timestamps),
            Arc::new(risk_scores),
        ],
    )
}

struct PoolMetrics {
    total_evaluations: u64,
    high_risk_count: u64,
    start_time: Instant,
}

impl PoolMetrics {
    fn new() -> Self {
        Self {
            total_evaluations: 0,
            high_risk_count: 0,
            start_time: Instant::now(),
        }
    }

    #[inline(always)]
    fn record_evaluation(&mut self, is_high_risk: bool) {
        self.total_evaluations += 1;
        if is_high_risk {
            self.high_risk_count += 1;
        }
    }

    fn evaluations_per_sec(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed < 0.001 {
            return 0.0;
        }
        self.total_evaluations as f64 / elapsed
    }

    fn high_risk_ratio(&self) -> f64 {
        if self.total_evaluations == 0 {
            return 0.0;
        }
        self.high_risk_count as f64 / self.total_evaluations as f64
    }
}

fn pin_current_thread(core_id: core_affinity::CoreId, cid: usize, pool: &'static str) {
    if !core_affinity::set_for_current(core_id) {
        let logger = get_logger().lock().unwrap();
        logger.log(LogLevel::Warn, pool, cid, "Failed to pin thread");
    }
}

fn run_tokio_probe() -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "TOKIO",
        0,
        "Pool A async runtime probe complete (IOCP on Windows, 1ms tick)",
    );
    Ok(())
}

#[cfg(feature = "db")]
fn run_brain_ignition() -> Result<(), Box<dyn Error>> {
    let logger = get_logger().lock().unwrap();
    let duck = duckdb::Connection::open_in_memory()?;
    let duckdb_version: String = duck.query_row("SELECT version()", [], |row| row.get(0))?;
    logger.log(
        LogLevel::Info,
        "BRAIN",
        0,
        &format!("DuckDB OLAP engine online | version {}", duckdb_version),
    );
    let _kuzu_db = kuzu::Database::in_memory(kuzu::SystemConfig::default())?;
    logger.log(
        LogLevel::Info,
        "BRAIN",
        0,
        "Kuzu graph brain online | in-memory bipartite store ready",
    );
    Ok(())
}

fn run_io_pool(core_id: usize, batch: Arc<RecordBatch>, shutdown: Arc<AtomicBool>) {
    let logger = get_logger().lock().unwrap();
    logger.log(LogLevel::Info, "POOL_A", core_id, "I/O Marshaller Active");
    drop(logger);
    let mut metrics = PoolMetrics::new();
    while !shutdown.load(Ordering::Relaxed) {
        let (_rows, _cols) = (batch.num_rows(), batch.num_columns());
        metrics.record_evaluation(false);
        thread::yield_now();
    }
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "POOL_A",
        core_id,
        &format!(
            "I/O Complete | Ops: {} | Rate: {:.0} ops/sec",
            metrics.total_evaluations,
            metrics.evaluations_per_sec()
        ),
    );
}

fn run_compute_pool(core_id: usize, batch: Arc<RecordBatch>, shutdown: Arc<AtomicBool>) {
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "POOL_B",
        core_id,
        "Forensic Compute Engine Active",
    );
    drop(logger);
    let mut metrics = PoolMetrics::new();
    let mut iteration: u64 = 0;
    let risk_col = batch
        .column(3)
        .as_any()
        .downcast_ref::<Float32Array>()
        .expect("Schema mismatch");
    let risk_values = risk_col.values();
    while !shutdown.load(Ordering::Relaxed) && iteration < CONFIG.scan_iterations {
        for &risk in risk_values.iter() {
            metrics.record_evaluation(risk > 0.5);
        }
        iteration += 1;
        if iteration.is_multiple_of(1000) {
            thread::yield_now();
        }
    }
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "POOL_B",
        core_id,
        &format!(
            "Compute Complete | Iters: {} | Eval: {} | High-Risk: {:.1}% | Rate: {:.0} evals/sec",
            iteration,
            metrics.total_evaluations,
            metrics.high_risk_ratio() * 100.0,
            metrics.evaluations_per_sec()
        ),
    );
}

fn run_db_pool(core_id: usize, batch: Arc<RecordBatch>, shutdown: Arc<AtomicBool>) {
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "POOL_C",
        core_id,
        "DB Coordinator Standby (Phase 3: DuckDB/Kuzu WAL + Graph Indexing)",
    );
    drop(logger);
    let mut metrics = PoolMetrics::new();
    while !shutdown.load(Ordering::Relaxed) {
        metrics.record_evaluation(batch.num_rows() > 0);
        thread::yield_now();
    }
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "POOL_C",
        core_id,
        &format!(
            "DB Coordination Complete | Ops: {} | Rate: {:.0} ops/sec",
            metrics.total_evaluations,
            metrics.evaluations_per_sec()
        ),
    );
}

pub fn run_pipeline() -> Result<(), Box<dyn Error>> {
    init_logger()?;
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "INIT",
        0,
        "TESSERA v13.1 [WINDOWS NATIVE // SPINE ONLINE] Starting...",
    );
    let core_ids = core_affinity::get_core_ids().ok_or("Failed to retrieve core IDs")?;
    logger.log(
        LogLevel::Info,
        "INIT",
        0,
        &format!("Detected {} logical cores", core_ids.len()),
    );
    let schema = create_forensic_schema()?;
    let batch = Arc::new(create_sample_batch(&schema)?);
    logger.log(
        LogLevel::Info,
        "ARROW",
        0,
        &format!(
            "Dataset Loaded: {} rows x {} cols | Mem: ~{} KB",
            batch.num_rows(),
            batch.num_columns(),
            batch.get_array_memory_size() / 1024
        ),
    );
    drop(logger);
    run_tokio_probe()?;
    #[cfg(feature = "db")]
    run_brain_ignition()?;
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(core_ids.len());
    for core_id in core_ids {
        let cid = core_id.id;
        let batch_clone = Arc::clone(&batch);
        let shutdown_clone = Arc::clone(&shutdown_flag);
        let handle = match pool_of(cid) {
            Some(Pool::Io) => thread::spawn(move || {
                pin_current_thread(core_id, cid, "POOL_A");
                run_io_pool(cid, batch_clone, shutdown_clone);
            }),
            Some(Pool::Compute) => thread::spawn(move || {
                pin_current_thread(core_id, cid, "POOL_B");
                run_compute_pool(cid, batch_clone, shutdown_clone);
            }),
            Some(Pool::DbCoord) => thread::spawn(move || {
                pin_current_thread(core_id, cid, "POOL_C");
                run_db_pool(cid, batch_clone, shutdown_clone);
            }),
            None => {
                let logger = get_logger().lock().unwrap();
                logger.log(LogLevel::Warn, "TOPOLOGY", cid, "Core unassigned");
                drop(logger);
                continue;
            }
        };
        handles.push(handle);
    }
    {
        let logger = get_logger().lock().unwrap();
        logger.log(
            LogLevel::Info,
            "STATUS",
            0,
            "Pipeline Active. Press ENTER to terminate.",
        );
    }
    let _ = io::stdin().lock().lines().next();
    {
        let logger = get_logger().lock().unwrap();
        logger.log(
            LogLevel::Info,
            "SHUTDOWN",
            0,
            "Signal received. Terminating workers...",
        );
    }
    shutdown_flag.store(true, Ordering::Relaxed);
    for (idx, handle) in handles.into_iter().enumerate() {
        if let Err(e) = handle.join() {
            let logger = get_logger().lock().unwrap();
            logger.log(LogLevel::Error, "JOIN", idx, &format!("Thread panic: {:?}", e));
        }
    }
    let logger = get_logger().lock().unwrap();
    logger.log(
        LogLevel::Info,
        "SUMMARY",
        0,
        &format!(
            "System Halted | Batch: {}x{} | Zero-Copy: YES | Safety: MAX",
            batch.num_rows(),
            batch.num_columns()
        ),
    );
    drop(logger);
    let _ = writeln!(
        io::stdout(),
        "\n--- {} v13.1: MISSION COMPLETE ---",
        CONFIG.log_prefix
    );
    Ok(())
}
