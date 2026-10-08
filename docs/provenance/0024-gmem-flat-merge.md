# 0024: gmem-flat merge preparation

- Date: 2026-10-07
- Author: Khargoosh
- Covers: main integration, optional test dependencies, output paths, and review

`git merge main` reported `Already up to date`. Main revision
`2fc997b1c0e284274d95db06eec9e4dcd5ec02b3` was already an ancestor of
`gmem-flat`. There were no conflicts.

The wrapper in `crates/novena/tests/shadowbox_global_memory.rs` now creates
an isolated Cargo test package under `target`. Shadowbox is only a
`[dev-dependencies]` entry in that package, not a workspace dependency.
The checks run only when `NOVENA_SHADOWBOX_PATH` names a Shadowbox `crates/shadowbox` directory; otherwise they skip.
`NOVENA_SHADOWBOX_PATH` can select another crate directory. A missing manifest
prints `SKIP`; a present checkout must pass translation, validation, and GPU
assertions. Its test programs moved to `tests/shadowbox/global_memory.rs`
without changing their encodings or assertions.

## Sources and review

These are project-authored build and test choices. The complete diff against
main was reviewed against [0022](0022-flat-global-memory.md) for memory and
Vulkan behavior, and [0023](0023-shadowbox-global-memory-proof.md) for synthetic
encodings and the translator contract. Those notes cite the existing signature
records and public sources. No new target ABI or instruction fact was inferred.
No proprietary material, program data, or external implementation code was added.
Hardware and driver names were removed from the public design document and
retained in provenance. The public documentation scan found no console or vendor
names. Production shader draw and dispatch execution remains unfinished.

## Verification

Formatting and strict clippy passed for the workspace and the isolated test
package. These workspace commands passed with exit code 0:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture
```

Default tests passed 35 tests. The explicit Vulkan run passed all 43 workspace
tests with no ignored tests, including both direct global-memory shader tests,
the pool-address API test, and all 156 translated GPU cases. Every translated
GPU module passed SPIR-V validation and every expected pool byte matched.

A copy under `target/merge-without-shadowbox/snapshot` had no checkout at the
default Shadowbox path. Locked builds and tests passed with default features
and Vulkan. Both optional tests printed `SKIP` in the explicit Vulkan run.
The sibling checkout stayed read-only. Temporary output and caches stayed in
this worktree; the shader-dump unit test now honors `TMPDIR` through `temp_dir`.

The project-profile packet and receipt reported that this worktree is unregistered.
No profile was changed or candidate promoted.
