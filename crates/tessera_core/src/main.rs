use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("benford") => {
            let mut allow_unsealed = false;
            let mut path: Option<PathBuf> = None;
            for arg in &args[1..] {
                if arg == "--allow-unsealed" {
                    allow_unsealed = true;
                } else {
                    path = Some(PathBuf::from(arg));
                }
            }
            let target =
                path.unwrap_or_else(|| PathBuf::from(tessera_core::benford::DEFAULT_FIXTURE));
            let code = tessera_core::benford::execute(&target, allow_unsealed);
            std::process::exit(code);
        }
        _ => tessera_core::orchestrator::run_pipeline(),
    }
}
