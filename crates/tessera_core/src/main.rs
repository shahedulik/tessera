use std::error::Error;
use std::path::PathBuf;

fn resolve_evidence_path(args: &[String]) -> (PathBuf, bool) {
    let mut allow_unsealed = false;
    let mut path: Option<PathBuf> = None;
    for arg in args {
        if arg == "--allow-unsealed" {
            allow_unsealed = true;
        } else {
            path = Some(PathBuf::from(arg));
        }
    }
    (
        path.unwrap_or_else(|| PathBuf::from(tessera_core::benford::DEFAULT_FIXTURE)),
        allow_unsealed,
    )
}

fn run_db_command(args: &[String], rings: bool) -> i32 {
    let (path, allow_unsealed) = resolve_evidence_path(args);
    #[cfg(feature = "db")]
    {
        if rings {
            tessera_core::graph::execute(&path, allow_unsealed)
        } else {
            tessera_core::ingest::execute(&path, allow_unsealed)
        }
    }
    #[cfg(not(feature = "db"))]
    {
        let _ = (&path, allow_unsealed, rings);
        eprintln!(
            "ERROR: this subcommand requires the db feature: cargo build --release -p tessera_core --features db"
        );
        4
    }
}

fn run_pdfscan(args: &[String]) -> i32 {
    #[cfg(feature = "pdf")]
    {
        tessera_vision::pdf_vein::execute(args)
    }
    #[cfg(not(feature = "pdf"))]
    {
        let _ = args;
        eprintln!(
            "ERROR: pdfscan requires the pdf feature: cargo build --release -p tessera_core --features pdf"
        );
        4
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("benford") => {
            let (path, allow_unsealed) = resolve_evidence_path(&args[1..]);
            let code = tessera_core::benford::execute(&path, allow_unsealed);
            std::process::exit(code);
        }
        Some("ingest") => {
            let code = run_db_command(&args[1..], false);
            std::process::exit(code);
        }
        Some("rings") => {
            let code = run_db_command(&args[1..], true);
            std::process::exit(code);
        }
        Some("pdfscan") => {
            let code = run_pdfscan(&args[1..]);
            std::process::exit(code);
        }
        _ => tessera_core::orchestrator::run_pipeline(),
    }
}
