use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Watch web source files for changes
    println!("cargo:rerun-if-changed=../../web/src/");
    println!("cargo:rerun-if-changed=../../web/package.json");
    println!("cargo:rerun-if-changed=../../web/package-lock.json");
    println!("cargo:rerun-if-changed=../../web/vite.config.js");
    println!("cargo:rerun-if-env-changed=SKIP_WEB_BUILD");

    // Allow skipping web build for CI (where web build runs separately)
    // or for Rust-only development
    if std::env::var("SKIP_WEB_BUILD").as_deref() == Ok("1") {
        println!("cargo:warning=SKIP_WEB_BUILD=1 set, skipping web build");
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let web_dir = manifest_dir.join("../../web");
    let web_dir = web_dir.canonicalize().unwrap_or_else(|_| web_dir.clone());

    // Check if npm is available
    let npm_check = Command::new("npm").arg("--version").output();
    if npm_check.is_err() || !npm_check.unwrap().status.success() {
        panic!(
            "npm is not available. Install Node.js/npm or set SKIP_WEB_BUILD=1 to skip web build."
        );
    }

    // Run npm ci if node_modules doesn't exist
    let node_modules = web_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo:warning=web/node_modules not found, running npm ci...");
        let output = Command::new("npm")
            .args(["ci"])
            .current_dir(&web_dir)
            .output()
            .unwrap_or_else(|e| panic!("Failed to spawn npm ci: {e}"));

        if !output.status.success() {
            panic!(
                "npm ci failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    // Run npm run build
    println!("cargo:warning=Building web assets (npm run build)...");
    let output = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&web_dir)
        .output()
        .unwrap_or_else(|e| panic!("Failed to spawn npm run build: {e}"));

    if !output.status.success() {
        panic!(
            "npm run build failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    println!("cargo:warning=Web assets built successfully.");
}
