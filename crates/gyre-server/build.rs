use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Declare rerun triggers — cargo will only re-invoke build.rs when these change
    println!("cargo:rerun-if-changed=../../web/src/");
    println!("cargo:rerun-if-changed=../../web/package.json");
    println!("cargo:rerun-if-changed=../../web/package-lock.json");
    println!("cargo:rerun-if-changed=../../web/vite.config.js");
    println!("cargo:rerun-if-env-changed=SKIP_WEB_BUILD");

    // Allow CI or Rust-only development to skip the npm build
    if std::env::var("SKIP_WEB_BUILD").as_deref() == Ok("1") {
        println!("cargo:warning=SKIP_WEB_BUILD=1 — skipping web build");
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .join("../..")
        .canonicalize()
        .expect("Failed to resolve workspace root");
    let web_dir = workspace_root.join("web");

    if !web_dir.exists() {
        panic!(
            "build.rs: web/ directory not found at {}",
            web_dir.display()
        );
    }

    // Ensure node_modules are installed before building
    let node_modules = web_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo:warning=web/node_modules missing — running npm ci");
        let status = Command::new("npm")
            .args(["ci"])
            .current_dir(&web_dir)
            .status()
            .unwrap_or_else(|e| {
                panic!("build.rs: failed to spawn npm ci — is Node.js installed? Error: {e}")
            });
        if !status.success() {
            panic!("build.rs: npm ci failed with exit code {:?}", status.code());
        }
    }

    // Run the frontend build
    let output = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&web_dir)
        .output()
        .unwrap_or_else(|e| {
            panic!("build.rs: failed to spawn npm run build — is Node.js installed? Error: {e}")
        });

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "build.rs: npm run build failed (exit {:?})\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
            output.status.code()
        );
    }

    println!("cargo:warning=web build complete");
}
