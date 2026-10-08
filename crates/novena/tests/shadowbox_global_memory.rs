//! Optional translator experiments. Provenance: docs/provenance/0024-gmem-flat-merge.md.

#![cfg(feature = "vulkan")]

use std::{env, fs, path::PathBuf, process::Command};

fn run(test: &str) {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_root.join("../..").canonicalize().unwrap();
    let shadowbox = env::var_os("NOVENA_SHADOWBOX_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("NOVENA_SHADOWBOX_PATH-unset"));
    if !shadowbox.join("Cargo.toml").is_file() {
        eprintln!(
            "SKIP {test}: Shadowbox checkout absent at {}",
            shadowbox.display()
        );
        return;
    }
    let shadowbox = shadowbox.canonicalize().unwrap();
    let directory = root.join("target/shadowbox-global-memory").join(test);
    fs::create_dir_all(&directory).unwrap();
    let manifest = directory.join("Cargo.toml");
    // Keep path dependency resolution outside Novena's workspace. All dependencies
    // belong only to this test package and all generated files stay under target.
    fs::write(
        &manifest,
        format!(
            r#"[package]
name = "novena-shadowbox-global-memory-tests"
version = "0.0.0"
edition = "2021"

[workspace]

[[test]]
name = "global_memory"
path = {source:?}

[dev-dependencies]
novena = {{ path = {novena:?}, features = ["vulkan"] }}
ash = {{ version = "0.38", default-features = false, features = ["loaded"] }}
shadowbox = {{ path = {shadowbox:?} }}
"#,
            source = crate_root.join("tests/shadowbox/global_memory.rs"),
            novena = crate_root,
        ),
    )
    .unwrap();
    let status = Command::new(env!("CARGO"))
        .args(["test", "--manifest-path"])
        .arg(manifest)
        .arg("--target-dir")
        .arg(root.join("target/shadowbox-global-memory/build"))
        .args([
            "--test",
            "global_memory",
            test,
            "--",
            "--exact",
            "--include-ignored",
            "--nocapture",
        ])
        .status()
        .expect("Cargo must run the isolated Shadowbox test");
    assert!(status.success(), "Shadowbox test failed: {status}");
}

#[test]
fn synthetic_translations_match_novena_push_constants() {
    run("synthetic_translations_match_novena_push_constants");
}

#[test]
#[ignore = "requires Shadowbox, a Vulkan GPU, 1 GiB coherent device-address memory and spirv-val"]
fn shadowbox_programs_execute_in_novena_arena() {
    run("shadowbox_programs_execute_in_novena_arena");
}
