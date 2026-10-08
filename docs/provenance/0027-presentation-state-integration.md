# 0027: presentation and retained state integration

- Date: 2026-10-08
- Author: Khargoosh
- Covers: version-5 host fixtures, mixed state and clear recordings, and integration verification

## What was learned

The presentation and remaining-command changes share the command recorder and
queue. The 17 new state handlers append records. Queue submission skips those
records while executing the supported clears and copies around them.

Version 5 adds the optional native presentation pointer to the host struct.
The state-command submission fixture now supplies a null pointer, keeping its
existing callback contract. No new guest signature or execution meaning is
inferred by this integration.

## How

The interface and recording facts come from the repository's existing records:

- [Presentation](0026-presentation.md)
- [Remaining command state](0026-remaining-command-state.md)
- [Remaining signatures](../signatures/0010-remaining-command-state.md)
- [Compute pipeline proof](0025-compute-pipelines.md)

The synthetic presentation test now places a blend-state reference and a
barrier before each colour clear, then a retained compute dispatch after it.
Exact pixel and arena-byte assertions still pass. This checks that retained
state and executed clears can share one recording without running a dispatch.
Existing state-command tests cover all 17 handlers and their recording lifetimes.

The optional translator proof uses the revision recorded in note 0025,
`2ee031ad7e62a76296d1465c5438c8aeb3173803`. The proof is isolated under
`target`; the dependency is not part of the library's workspace contract.
No external implementation was used to infer new guest behaviour.

## Verification

All commands below returned exit code zero:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/global_memory.rs
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo clippy --locked --manifest-path target/shadowbox-global-memory/compute_pipeline_cache_executes_translated_program/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets -- -D warnings
cargo build --locked --workspace --all-features
cargo test --locked --workspace
cargo test --locked --workspace --all-features -- --include-ignored --nocapture --test-threads=1
python3 scripts/check-presentation.py
```

Default tests passed 41 tests. The all-feature run passed 56 workspace tests,
with zero failed or ignored tests and no optional translator skips. The direct
GPU tests and both presentation pixel tests ran on a hardware Vulkan device.
The nested translator checks matched all 156 global-memory cases and passed
the translated compute pipeline cache proof.

The C host compiled with warnings denied and produced the exact black 4 by 3
PPM through both Vulkan and CPU fallback. The native X11 example also compiled
with warnings denied. No native window run is claimed by this headless check.

## Confidence and open questions

This integration preserves the limits in the two source notes. Retained
command state remains unexecuted, including compute dispatch, barriers and
buffer clears. Graphics draw execution, guest format decoding and the other
unresolved signatures remain open. Pixel checks establish the tested synthetic
cases, not the output of any external program.
