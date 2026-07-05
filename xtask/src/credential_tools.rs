//! compiles the host-only credential tools alongside the workspace. neither is containerized:
//!
//! - `tools/keychain-export` (Swift): reads a secret from the macOS login Keychain. macOS-only.
//! - `tools/runinator-secret-sync` (Go): syncs credentials into Kubernetes Secrets/files.
//!
//! a missing toolchain (or a failing build) is reported as a warning, not a hard error, so a build
//! host without swift/go still gets a working rust workspace build.

use std::path::Path;

use crate::exec;

pub fn build_credential_tools(workspace_root: &Path) {
    build_keychain_export(workspace_root);
    build_secret_sync(workspace_root);
}

fn build_keychain_export(workspace_root: &Path) {
    let swift_dir = workspace_root.join("tools/keychain-export");
    if !swift_dir.join("Package.swift").exists() {
        return;
    }

    if !cfg!(target_os = "macos") {
        println!("==> Skipping keychain-export build (macOS-only Keychain helper).");
        return;
    }

    if !exec::tool_available("swift") {
        eprintln!("warning: swift toolchain not found on PATH; skipping keychain-export build.");
        return;
    }

    println!("==> Building keychain-export (Swift Keychain helper, release)");
    exec::warn_on_err("keychain-export build failed", || {
        exec::run("swift", &["build", "-c", "release"], &swift_dir)
    });
}

fn build_secret_sync(workspace_root: &Path) {
    let go_dir = workspace_root.join("tools/runinator-secret-sync");
    if !go_dir.join("go.mod").exists() {
        return;
    }

    if !exec::tool_available("go") {
        eprintln!("warning: go toolchain not found on PATH; skipping runinator-secret-sync build.");
        return;
    }

    println!("==> Building runinator-secret-sync (Go credential sync engine)");
    exec::warn_on_err("runinator-secret-sync build failed", || {
        exec::run("go", &["build", "./..."], &go_dir)
    });
}
