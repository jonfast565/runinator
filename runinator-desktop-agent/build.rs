use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    if target_os != "macos" || !host.contains("apple-darwin") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap();
    let tool_dir = workspace_root.join("tools/keychain-export");
    let package = tool_dir.join("Package.swift");
    let entry = tool_dir.join("Sources/keychain-export/Entry.swift");
    let keychain = tool_dir.join("Sources/keychain-export/Keychain.swift");
    let support = tool_dir.join("Sources/keychain-export/Support.swift");
    let format = tool_dir.join("Sources/keychain-export/Format.swift");
    for input in [&package, &entry, &keychain, &support, &format] {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    println!("cargo:rerun-if-env-changed=RUNINATOR_KEYCHAIN_CODESIGN_IDENTITY");

    let source = tool_dir.join(".build/release/keychain-export");
    if !source.is_file() {
        run(&tool_dir, "swift", ["build", "-c", "release"]);
        if !source.is_file() {
            panic!("Swift did not produce {}", source.display());
        }
        sign_if_configured(&tool_dir, &source);
    }

    let staged = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("keychain-export");
    fs::copy(&source, &staged).unwrap_or_else(|error| {
        panic!(
            "could not stage keychain-export from {} to {}: {error}",
            source.display(),
            staged.display()
        )
    });
    println!(
        "cargo:rustc-env=RUNINATOR_KEYCHAIN_EXPORT_PATH={}",
        staged.display()
    );
}

fn sign_if_configured(tool_dir: &Path, binary: &Path) {
    let Ok(identity) = env::var("RUNINATOR_KEYCHAIN_CODESIGN_IDENTITY") else {
        return;
    };
    if identity.is_empty() {
        return;
    }
    run(
        tool_dir,
        "codesign",
        [
            "--force",
            "--sign",
            &identity,
            "--identifier",
            "com.runinator.keychain-export",
            binary.to_str().unwrap(),
        ],
    );
}

fn run<const N: usize>(working_dir: &Path, program: &str, args: [&str; N]) {
    let status = Command::new(program)
        .args(args)
        .current_dir(working_dir)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "could not run {program} in {}: {error}",
                working_dir.display()
            )
        });
    if !status.success() {
        panic!(
            "{program} failed in {} with {status}",
            working_dir.display()
        );
    }
}
