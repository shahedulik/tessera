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

fn run_db_command(args: &[String], rings: bool, collapse: bool) -> i32 {
    let (path, allow_unsealed) = resolve_evidence_path(args);
    #[cfg(feature = "db")]
    {
        if rings {
            tessera_core::graph::execute(&path, allow_unsealed, collapse)
        } else {
            tessera_core::ingest::execute(&path, allow_unsealed)
        }
    }
    #[cfg(not(feature = "db"))]
    {
        let _ = (&path, allow_unsealed, rings, collapse);
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

fn run_anomaly(args: &[String]) -> i32 {
    #[cfg(feature = "vision")]
    {
        tessera_vision::anomaly::execute(args)
    }
    #[cfg(not(feature = "vision"))]
    {
        let _ = args;
        eprintln!(
            "ERROR: anomaly requires the vision feature: cargo build --release -p tessera_core --features vision"
        );
        4
    }
}

fn run_identity(args: &[String]) -> i32 {
    let positional: Vec<String> = args
        .iter()
        .filter(|arg| {
            !matches!(
                arg.as_str(),
                "--csv" | "--threshold" | "--allow-unsealed"
            )
        })
        .cloned()
        .collect();
    let (path, allow_unsealed) = resolve_evidence_path(&positional);
    let mut forwarded: Vec<String> = Vec::with_capacity(args.len() + 2);
    forwarded.push("--csv".to_string());
    forwarded.push(path.to_string_lossy().into_owned());
    if allow_unsealed {
        forwarded.push("--allow-unsealed".to_string());
    }
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--threshold" {
            forwarded.push(args[index].clone());
            index += 1;
            match args.get(index) {
                Some(value) => forwarded.push(value.clone()),
                None => {
                    eprintln!("ERROR: --threshold requires a value in [0.0, 1.0]");
                    return 2;
                }
            }
        }
        index += 1;
    }
    tessera_core::identity::execute(&forwarded)
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
            let code = run_db_command(&args[1..], false, false);
            std::process::exit(code);
        }
        Some("rings") => {
            let collapse = !args[1..].iter().any(|arg| arg == "--no-collapse");
            let code = run_db_command(&args[1..], true, collapse);
            std::process::exit(code);
        }
        Some("identity") => {
            let code = run_identity(&args[1..]);
            std::process::exit(code);
        }
        Some("pdfscan") => {
            let code = run_pdfscan(&args[1..]);
            std::process::exit(code);
        }
        Some("anomaly") => {
            let code = run_anomaly(&args[1..]);
            std::process::exit(code);
        }
        _ => tessera_core::orchestrator::run_pipeline(),
    }
}
