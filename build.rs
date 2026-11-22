use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-changed=crates/nixos-manager-apply");
    println!("cargo::rerun-if-changed=crates/nixos-manager-apply-protocol");
    println!("cargo::rerun-if-changed=Cargo.toml");
    println!("cargo::rerun-if-changed=Cargo.lock");
    println!("cargo::rerun-if-env-changed=NIXOS_MANAGER_PACKAGED");

    // The Nix package builds the workspace and installs the helper in libexec.
    if env::var_os("NIXOS_MANAGER_PACKAGED").is_some() {
        return;
    }

    // Cargo run only builds the GUI target. Use a separate target directory for
    // the helper to avoid waiting on the outer Cargo process's build lock.
    // Use a Cargo artifact dependency once bindeps is stable.
    // https://github.com/rust-lang/cargo/issues/9096
    let target = env::var("TARGET").expect("Cargo target");
    let host = env::var("HOST").expect("Cargo host");
    let target_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory")).join("apply-helper");
    let mut cargo = Command::new(env::var_os("CARGO").expect("Cargo executable"));
    cargo
        .args([
            "build",
            "--locked",
            "--offline",
            "--package",
            "nixos-manager-apply",
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .env("CARGO_BUILD_BUILD_DIR", &target_dir);
    let binary_dir = if target == host {
        target_dir
    } else {
        cargo.args(["--target", &target]);
        target_dir.join(&target)
    };
    if let Some(linker) = env::var_os("RUSTC_LINKER") {
        let target_env = target.to_ascii_uppercase().replace('-', "_");
        cargo.env(format!("CARGO_TARGET_{target_env}_LINKER"), linker);
    }
    assert!(
        cargo
            .status()
            .expect("launch Cargo for the helper")
            .success(),
        "could not build nixos-manager-apply",
    );
    let binary = binary_dir.join("debug/nixos-manager-apply");
    println!(
        "cargo::rustc-env=NIXOS_MANAGER_DEV_HELPER={}",
        binary.display()
    );
}
