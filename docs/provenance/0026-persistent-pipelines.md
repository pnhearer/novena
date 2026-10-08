# 0026: persistent pipeline caches and background compilation

- Date: 2026-10-08
- Author: Khargoosh
- Covers: gpu/pipeline_disk.rs, gpu/pipelines.rs, pipeline cache tests,
  the translated pipeline proof, and docs/design/pipelines.md

## What was learned

Vulkan pipeline cache data can be retrieved and supplied to a later cache
creation on a compatible implementation. The version-one header is 32 bytes
and uses little-endian fields for header size, version, vendor, device and
pipelineCacheUUID. Vulkan deviceUUID identifies the device and driverUUID
identifies the driver build. Pipeline handles themselves cannot be persisted.

The shader capture and translation boundaries remain those recorded in
[0025](0025-compute-pipelines.md), signatures 0007 and 0008, and provenance 0021.
This change establishes no additional guest layout, stage mapping or enum.
All persistence and worker scheduling behavior is a host implementation choice.

## How

The permitted external sources for new Vulkan behavior are:

- [Cache creation](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineCacheCreateInfo.html)
  describes initial pipeline cache data.
- [Cache retrieval](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetPipelineCacheData.html)
  describes complete snapshots and synchronization with cache modification.
- [Version-one header](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineCacheHeaderVersionOne.html)
  defines the header fields and byte order.
- [Device identification](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceIDProperties.html)
  defines deviceUUID and driverUUID.
- [Compute creation](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateComputePipelines.html)
  defines pipeline creation using a cache.

The cache format and scheduling tests are original experiments for Novena.
No external implementation was copied. The BLAKE3 dependency provides stable
digests and checksums. The host supplies a translator version and configuration
that identify generation and options. The namespace also includes compute main,
interface and format revisions, device and driver UUIDs, pipelineCacheUUID,
vendor and device. Disk translation records compare all input bytes to prevent
digest collisions from aliasing programs.

Each cache record has a versioned envelope, namespace digest, payload length and
checksum. File reads are bounded. Writes use exclusive unique temporary files,
file flush, atomic replacement and directory flush. Concurrent writers publish
complete records; the last writer wins. This may lose driver warmup coverage
because there is no cross-process merge. Temporary files from interrupted
writers are ignored. Only a private host-owned directory is accepted by contract.
Cache failures produce diagnostics and preserve compilation.

The worker pool owns snapshots, shares duplicate requests, bounds waiting work,
and retains queued, compiling, ready and failed states. Requests use nonblocking
queue submission and polling uses an atomic phase with a nonblocking result lock.
Translation and I/O happen outside the driver cache mutex. That mutex serializes
Vulkan creation and snapshots. Workers never submit queues or use command pools.
Shutdown drains jobs and joins workers. Failed work requires explicit retry.
A program's current request handle owns its completion, so replacing that handle
prevents stale completion from replacing the current result.

Skipping a draw whose pipeline is unavailable is a deliberate host policy to
measure later. Queued, compiling, failed and queue-full requests skip that draw
for the frame. Other commands continue. Skipped draws are not replayed.
This is not an established guest semantic. Guest draw execution, resource
bindings and routing existing eager translation through the service remain
future work.

## Experiments and limits

Disk unit tests check translator version, options, deviceUUID and driverUUID
namespace separation; truncated, corrupted and oversized files; input mismatch;
every Vulkan header field; concurrent writers; and ignored partial temporary
files. Driver-header tests recompute the envelope checksum so header validation
is exercised independently of checksum rejection.

The pipeline integration proof compiles an original empty compute shader with
glslangValidator and validates it with spirv-val. Separate subprocesses create
and reopen both caches. Translation counters distinguish fresh translation
from disk reuse, and the driver-load statistic checks that validated initial
data was supplied to successful cache creation.
The proof truncates translation records, damages driver records, and supplies
checksummed SPIR-V with an invalid header. Fresh compilation repairs each entry,
and another subprocess reopens the repair. Concurrent subprocesses publish to
the same destination and a later process checks it again.

A gated translator holds two workers while the caller requests duplicate and
replacement programs, polls pending results, fills the waiting queue and observes
queue-full rejection. Releasing the gate completes requests and proves Arc reuse.
Failures, explicit retry, translator panic recovery, persisted asynchronous reuse,
and cache-directory I/O failure are also checked.

The existing original translated GPU proof now builds its uniform-buffer
pipeline on the worker pool, reuses it and executes it after worker shutdown.
Exact destination and sentinel bytes still verify physical-address copying and
the uniform load. Its instruction provenance remains 0025. No new instruction
encoding or translator implementation was consulted.

These experiments prove the host compute service. They do not prove guest
graphics execution, visual acceptability of skipped draws, driver compilation
hit rates or hostile-file authenticity.

## Verification

The following checks completed with exit code 0 on 2026-10-08:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/global_memory.rs
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy --manifest-path target/shadowbox-global-memory/compute_pipeline_cache_executes_translated_program/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture
```

The default run passed 35 tests. The all-feature run passed 56 workspace tests
with zero ignored tests. Both direct Vulkan memory tests and the configured
translated pipeline proof ran. All 156 translated memory cases matched.
The new restart, corruption, worker-pool and cache-I/O proofs ran on Vulkan;
none of those proofs skipped. SPIR-V fixtures passed spirv-val.

The first run of the checksummed-invalid-SPIR-V recovery case found an accounting
error. Recovery succeeded, but the cache counted the discarded disk record as a
translation hit. Moving the counter after successful pipeline creation fixed
that error, and the complete all-feature GPU run then passed.

The private cache format remains an implementation choice, not a guest fact.
The tests demonstrate complete file publication and recovery, not guaranteed
storage durability on every filesystem or measured driver-cache speedups.

## Integration verification

Rebased onto main at cd7a5a8 on 2026-10-08. The module-list conflict keeps
both the presentation modules and the private disk-cache module. Presentation,
clears, host interface version 5 and retained command state remain intact.

Formatting and both workspace clippy commands above passed again with exit code
0. Clippy also passed for all three generated translator test packages with
all targets, locked dependencies and warnings denied. The configured translator
was supplied through `NOVENA_SHADOWBOX_PATH`.

The default workspace suite passed 41 tests. The all-feature suite passed 64
workspace tests with zero ignored tests using:

```sh
cargo test --workspace --all-features --locked -- --include-ignored --nocapture --test-threads=1
```

Both presentation tests, both direct GPU memory tests, the persistent-cache
restart and recovery proof, worker scheduling and cache-I/O failure proofs,
and the translated pipeline proof ran. All 156 translated memory cases matched.
The rebased commit diff was checked for local paths, checkout names, excluded
product names in prose and attribution trailers. No excluded text was found.
