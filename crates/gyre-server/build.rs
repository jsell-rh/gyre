use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
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

    // Watch top-level config files individually
    for name in &["package.json", "package-lock.json", "vite.config.js"] {
        println!("cargo:rerun-if-changed={}", web_dir.join(name).display());
    }

    // Walk web/src/ and emit a rerun directive for every source file.
    // cargo:rerun-if-changed on a directory only watches directory *entries*
    // (adds/removes), not modifications to files inside — so we must enumerate.
    emit_rerun_for_dir(&web_dir.join("src"));

    // Ensure node_modules are installed before building.
    // Only checked for existence — if package-lock.json changes without removing
    // node_modules, developers should run `npm ci` manually or delete node_modules.
    let node_modules = web_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo:warning=web/node_modules missing — running npm ci");
        // Use .status() (streaming) so progress is visible during a potentially long install
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

    // Run the frontend build.
    // Use .output() (captured) so we only surface npm's output on failure.
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

/// Recursively walk `dir` and emit `cargo:rerun-if-changed` for each file.
fn emit_rerun_for_dir(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // directory may not exist yet (first checkout before build)
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            emit_rerun_for_dir(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
