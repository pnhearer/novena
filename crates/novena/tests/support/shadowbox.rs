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
    let cache_test =
        test == "emitted_cache_is_loaded_at_instance_creation_and_runtime_misses_reopen";
    let shadowbox = shadowbox.canonicalize().unwrap();
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("novena-checks"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    let directory = target
        .join("translator-checks")
        .join(test.replace("::", "-"));
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
novena = {{ path = {novena:?}, features = {features:?} }}
ash = {{ version = "0.38", default-features = false, features = ["loaded"] }}
shadowbox = {{ path = {shadowbox:?} }}
"#,
            features = if cache_test { vec![] } else { vec!["vulkan"] },
            source = crate_root.join(if cache_test {
                "tests/shadowbox/startup.rs"
            } else if test == "startup::cold_and_warm_first_draw"
                || matches!(
                    test,
                    "first_draw_executes_translated_triangle"
                        | "textured_checkerboard_and_blend_pixels"
                        | "translated_texture_pixels"
                        | "multiple_target_blend_pixels"
                        | "textured_uniform_banks_and_persistence"
                        | "primitive_topologies_read_back_pixels"
                        | "vertex_formats_read_back_pixels"
                        | "uniform_banks_colour_two_draws"
                        | "uniform_banks_with_strip_and_normalized_attribute"
                        | "indexed_draw_executes_translated_triangle"
                        | "depth_stencil_and_raster_pixels"
                        | "indexed_strip_uniform_depth_pixels"
                )
            {
                "tests/shadowbox/drawing.rs"
            } else {
                "tests/shadowbox/global_memory.rs"
            }),
            novena = crate_root,
        ),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO"));
    command.arg("test");
    if !cfg!(debug_assertions) {
        command.arg("--release");
    }
    let status = command
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--target-dir")
        .arg(target.join("translator-build"))
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
