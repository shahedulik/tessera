use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn vcpkg_provision_if_needed(workspace_root: &Path) {
    let manifest = workspace_root.join("vcpkg.json");
    let Ok(text) = fs::read_to_string(&manifest) else {
        return;
    };
    if text.contains("\"dependencies\": []") {
        return;
    }
    let installed = workspace_root.join("vcpkg_installed").join("x64-windows");
    if installed.exists() {
        return;
    }
    let vcpkg_root =
        env::var("VCPKG_ROOT").expect("VCPKG_ROOT not set - required for vcpkg provisioning");
    let cache = env::var("VCPKG_CACHE_ROOT").unwrap_or_else(|_| "D:\\tessera-cache\\vcpkg".into());
    let args: Vec<String> = vec![
        "install".into(),
        "--triplet".into(),
        "x64-windows".into(),
        "--buildtrees-root".into(),
        format!("{cache}\\buildtrees"),
        "--downloads-root".into(),
        format!("{cache}\\downloads"),
        "--packages-root".into(),
        format!("{cache}\\packages"),
        "--x-manifest-root".into(),
        workspace_root.to_string_lossy().into_owned(),
        "--install-root".into(),
        installed.to_string_lossy().into_owned(),
    ];
    println!("cargo:warning=vcpkg provisioning started - all roots routed to D: ({cache})");
    let status = Command::new(Path::new(&vcpkg_root).join("vcpkg.exe"))
        .current_dir(workspace_root)
        .args(&args)
        .status()
        .expect("vcpkg install invocation failed");
    assert!(status.success(), "vcpkg install failed - inspect D: routing flags above");
}

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
    println!("cargo:rerun-if-env-changed=KUZU_LIBRARY_DIR");
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(&manifest_dir).join("..").join("..");
    let profile_dir = Path::new(&env::var("OUT_DIR").expect("OUT_DIR"))
        .join("..")
        .join("..")
        .join("..");

    vcpkg_provision_if_needed(&workspace_root);
    stage_dll(
        &workspace_root.join("vendor").join("pdfium").join("pdfium.dll"),
        &profile_dir,
    );
    if let Ok(kuzu_lib_dir) = env::var("KUZU_LIBRARY_DIR") {
        stage_dll(&Path::new(&kuzu_lib_dir).join("kuzu_shared.dll"), &profile_dir);
    } else {
        println!("cargo:warning=KUZU_LIBRARY_DIR not set - required for '--features db' builds");
    }
}
