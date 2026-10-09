# Building and testing

Run these commands from the repository root. Use a current stable Rust
toolchain. The default features are empty and need no Vulkan loader.

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

The crate produces a Rust library, a shared C library, and a static C library.
The [public header](../../include/novena.h) describes the C interface.
Use the host interface version check before creating an instance.

## Optional Vulkan backend

Install a Vulkan loader and a compatible driver. The backend selects a graphics
queue on a Vulkan 1.2 device with 64-bit shader integers and buffer device
addresses. Arena allocation also needs compatible host-visible memory.
A software Vulkan driver can satisfy some tests without a physical GPU.

```sh
cargo build --workspace --features vulkan
cargo test --workspace --features vulkan
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

Ordinary feature-enabled tests include CPU checks and opportunistic backend
checks. Some backend checks return early when no device exists. A successful
ordinary run therefore does not prove GPU execution.

Install `glslangValidator` and `spirv-val` for the required GPU checks.
They compile and validate original synthetic shaders. Run these tests explicitly:

```sh
cargo test --workspace --features vulkan --test global_memory --test presentation --test pipeline_cache --test texture_images -- --include-ignored --nocapture --test-threads=1
cargo test --workspace --features vulkan --lib gpu::texture_transfer -- --include-ignored --nocapture --test-threads=1
```

These required tests fail when their device or shader tools are unavailable.
Compressed native image support is queried; an unsupported format is reported
explicitly. Geometry and byte conversion tests still cover its storage layout.
No display is needed for offscreen pixel and memory checks. A native window
example needs an X11 display and development headers.

If installed, enable the validation layer with
`VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation`. Read the output for validation
errors, skipped checks, and optional format support.

## External translator

The library does not include a shader translator. Its `ShaderTranslator` trait
lets a host supply one. The `shadowbox` feature enables integration tests which
build separate test packages against an external translator crate.

Set `NOVENA_SHADOWBOX_PATH` in your shell to the directory containing that
crate's `Cargo.toml`. No path is built into the repository. For CPU cache
interoperability alone:

```sh
cargo test --workspace --features shadowbox --test startup_translation -- --nocapture
```

That test package builds the library without Vulkan, though the outer
`shadowbox` feature includes `vulkan`. If the variable is absent the wrapper
prints a skip and returns success. Always inspect the output.

With the translator configured and a capable Vulkan device, run every test,
including the ignored tests:

```sh
cargo test --workspace --all-features -- --include-ignored --nocapture --test-threads=1
```

The translated memory tests need a 1 GiB coherent device-address arena.
Drawing tests also need the shader tools above. The first-draw timing test uses
fresh child processes and takes longer than the ordinary suite.

## Storage

Set `CARGO_TARGET_DIR` to your build directory and `TMPDIR` to your scratch
directory before running checks. Generated translator packages use the build
directory. Test shader files and temporary application caches use the operating
system temporary directory. Keep both locations outside the source tree.
Set `XDG_CACHE_HOME` separately if you also want driver caches outside it.

Evidence: [backend setup](../provenance/0013-vulkan-backend.md),
[flat memory](../provenance/0022-flat-global-memory.md),
[texture transfers](../provenance/0031-tiled-texture-transfers.md), and
[startup validation](../provenance/0032-startup-cache-validation.md).
The corresponding test entry points are in `crates/novena/tests`.
