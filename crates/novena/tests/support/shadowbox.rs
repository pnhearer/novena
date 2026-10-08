use std::{env, fs, path::PathBuf, process::Command};

pub fn run(test: &str) {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_root.join("../..").canonicalize().unwrap();
    // The translator is a separate project: point NOVENA_SHADOWBOX_PATH at its
    // crates/shadowbox directory to run these checks; without it they skip.
    let Some(shadowbox) = env::var_os("NOVENA_SHADOWBOX_PATH").map(PathBuf::from) else {
        eprintln!("SKIP {test}: NOVENA_SHADOWBOX_PATH is not set");
        return;
    };
    if !shadowbox.join("Cargo.toml").is_file() {
        panic!(
            "NOVENA_SHADOWBOX_PATH has no Cargo.toml: {}",
            shadowbox.display()
        );
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
            source = crate_root.join(
                if matches!(
                    test,
                    "first_draw_executes_translated_triangle"
                        | "primitive_topologies_read_back_pixels"
                        | "vertex_formats_read_back_pixels"
                        | "uniform_banks_colour_two_draws"
                        | "uniform_banks_with_strip_and_normalized_attribute"
                        | "indexed_draw_executes_translated_triangle"
                ) {
                    "tests/shadowbox/drawing.rs"
                } else {
                    "tests/shadowbox/global_memory.rs"
                }
            ),
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
