use std::env;
use std::fs;
use std::path::Path;

fn stage_dll(source: &Path, profile_dir: &Path) {
    if source.exists() {
        let name = source.file_name().expect("dll name");
        for dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
            if dir.exists() {
                let staged = dir.join(name);
                fs::copy(source, &staged).expect("dll staging failed");
            }
        }
        println!(
            "cargo:warning={} staged beside executable and test binaries",
            name.to_string_lossy()
        );
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../vendor/pdfium/pdfium.dll");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(&manifest_dir).join("..").join("..");
    let profile_dir = Path::new(&env::var("OUT_DIR").expect("OUT_DIR"))
        .join("..")
        .join("..")
        .join("..");

    stage_dll(
        &workspace_root.join("vendor").join("pdfium").join("pdfium.dll"),
        &profile_dir,
    );
}
