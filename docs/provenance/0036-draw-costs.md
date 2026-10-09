# 0036: measured draw submission costs

- Date: 2026-10-09
- Covers: retained draw state, pipeline and descriptor caches, grouped submission,
  push descriptors, and the synthetic CPU benchmark

## Sources

Signatures 0002, 0009, and 0010 establish recorded draws, state bindings,
vertex ranges, and uniform bindings. Provenance 0027 through 0030 and 0033
establish the bounded host contracts and original shader experiments reused
here. This work adds no inferred guest enum, object layout, or binding rule.

The host changes follow public Vulkan documentation:

- [Descriptor updates](https://docs.vulkan.org/refpages/latest/refpages/source/vkUpdateDescriptorSets.html)
  define descriptor writes and the lifetime of sets referenced by commands.
- [Descriptor binding](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdBindDescriptorSets.html)
  defines set numbers and pipeline layout compatibility.
- [Push descriptors](https://docs.vulkan.org/refpages/latest/refpages/source/VK_KHR_push_descriptor.html)
  define capability discovery and command-buffer-owned descriptors.
- [Pipeline layouts](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineLayoutCreateInfo.html)
  permit at most one push descriptor set in a layout.
- [Push limits](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDevicePushDescriptorPropertiesKHR.html)
  bound the descriptor count in that set.
- [Shader memory](https://docs.vulkan.org/spec/latest/chapters/shaders.html)
  describes physical storage access. The SPIR-V grammar referenced in 0025
  supplies the pointer opcode and storage-class value used by the conservative
  batching check.

All scene data and source shaders are original. The source shaders use the
existing public host interface. No observed program data enters the benchmark.

## Implementation choices

A ready pipeline lookup hashes shader contents and interpreted state into
buckets of dense vector indices. Full shader and state equality resolves hash
collisions. Reflection and owned compilation keys remain on the miss path.
Uniform resolution uses the ready pipeline's reflected banks. Pending
pipelines still validate missing or invalid uniform bindings before skipping
the draw.

Each pipeline caches immutable descriptor records by complete buffer handles,
offsets, ranges, image views, samplers, and layouts. Dense indices address those
records. The cache retains at most 256 records per pipeline. Texture release
and sampler replacement invalidate cached descriptors before resource handles
can be reused. A descriptor miss batches all ordinary writes in one update.
A hit performs no descriptor allocation or update.

When the device advertises push descriptors, one nonempty set uses that path
if its count fits both the device limit and the 64-write host capacity.
Other sets use the immutable cache. A device without this capability uses
ordinary sets throughout. The host can force that path by setting
`NOVENA_DISABLE_PUSH_DESCRIPTORS=1` before device creation. Descriptor buffers
are not required by this implementation.

Between recorded state changes, a draw reuses a validated state record.
An array draw still checks its current first vertex and count. An indexed
draw reuses validation only for the same index request and count.
The command vector returns to its recording object after successful
consumption. Submission reserves reusable draw storage once per recording.
The hot path appends geometry without a Rust allocation.

The draws in a group share one framebuffer, render pass, and draw submission.
Attachment transfers happen once per group. Unchanged draws issue geometry
without repeating pipeline,
vertex, descriptor, viewport, scissor, or push-constant bindings. Repeated
indexed draws also retain the index binding. Uniform contents upload with
arena contents at submission boundaries. Uniform and descriptor resolution
happen once per state group.

A state change completes the preceding group before resource preparation.
Clears, copies, target changes, and other execution boundaries complete
pending draws. Error readback also completes the preceding valid draws.
Vertex, index, or uniform ranges that overlap exact attachment storage disable
retained batching. Physical storage pointers in either shader also disable it,
because those shaders can access arena bytes beyond declared bindings.
These cases retain the synchronous path.

## Measurement

The scene records 10,000 three-vertex draws into a 64 by 64 target.
One hundred groups alternate scissor widths and uniform ranges while cycling
through three small textures. The host changes uniform contents between
frames. Pixel checks verify the last group's bindings and the preceding
group's pixels outside the last scissor.

Each process waits for its pipeline, then runs one untimed full frame and
three measured frames. The table reports totals divided by commands, draws,
or frames. Shader compilation, initial setup, warmup, and pixel assertions
are outside the measured interval. FPS includes command recording, queue
submission, GPU execution, and canonical arena download.

Recording and submit CPU times use the calling thread's CPU clock. Lookup
and preparation spans use monotonic elapsed time. Allocation counts include
Rust allocations and reallocations across the process. They exclude internal
driver allocations. Separate counters report actual Vulkan descriptor updates,
pool creations, state binds, and push writes.

The measurements use a release build with Rust 1.99.0 on Linux 7.2.9,
on the same machine with a discrete GPU. Instrumentation is opt-in through
the `draw-metrics` feature. The baseline uses implementation revision
`eb5866c` with the final scene harness, corrected command denominator, and
operation counters. Its reuse assertions are disabled for measurement.
The optimized implementation is `a01ca6b`.

| Measurement | Before | After, push descriptors | After, ordinary sets |
| --- | ---: | ---: | ---: |
| Recording CPU, ns per command | 111.96 | 95.68 | 94.90 |
| Submit CPU, ns per draw | 173,421.41 | 1,052.44 | 1,228.10 |
| Pipeline lookup, ns per draw | 4,416.87 | 14.14 | 18.48 |
| Descriptor preparation, ns per draw | 25,651.47 | 2.44 | 3.75 |
| Uniform resolution, ns per draw | 17,851.10 | 2.77 | 2.71 |
| Frames per second | 0.343 | 54.318 | 52.553 |
| Recording allocations per frame | 22 | 9 | 9 |
| Submit allocations per frame | 1,340,013 | 2,613 | 2,613 |
| Pipeline lookups per frame | 10,000 | 100 | 100 |
| Descriptor preparations per frame | 10,000 | 100 | 100 |
| Descriptor updates per warm frame | 10,000 | 0 | 0 |
| Descriptor pool creations per warm frame | 10,000 | 0 | 0 |
| State binds per frame | 10,000 | 100 | 100 |
| Push writes per frame | 0 | 100 | 0 |

After optimization, each actual pipeline lookup averages 1,414.31 ns,
descriptor preparation 243.88 ns, and uniform resolution 277.17 ns.
Their per-draw costs fall further because unchanged draws bypass them.
Submit CPU cost falls by about 165 times in this run.

Doubling geometry to 20,000 draws while retaining 100 state groups leaves
both allocation counts unchanged at 9 and 2,613 per frame. That run reports
626.84 ns of submit CPU time per draw and 42.707 FPS.
The indexed 10,000-draw variant reports 1,382.74 ns per draw and 38.240 FPS,
with the same allocations, cache counts, and 100 push writes.
These are synthetic throughput measurements, with no display pacing.
Short timing runs vary with concurrent CPU and GPU load.

The remaining allocations belong to frame and state-group preparation.
A new pipeline or descriptor identity still allocates on a cache miss.
The measurements do not claim that every possible state-changing draw or
driver command is allocation-free.

The benchmark commands are:

```sh
cargo run --release --features draw-metrics --example draw_bench -- 3
NOVENA_DISABLE_PUSH_DESCRIPTORS=1 cargo run --release --features draw-metrics --example draw_bench -- 3
cargo run --release --features draw-metrics --example draw_bench -- 3 20000
cargo run --release --features draw-metrics --example draw_bench -- 3 10000 indexed
```

Build and temporary output use the environment-provided storage directories.
Final driver and dependency caches use the build directory. The first exploratory baseline
inherited a default driver-cache location. Final measurements use an explicit
driver-cache directory.

## Verification

Formatting, default and all-feature Clippy with warnings denied, default
workspace tests, all-feature documentation tests, and all-feature GPU tests
pass. The GPU run includes ignored tests and the configured external
translator. It does not skip GPU or translator checks.

The original suite checks translated draws, uniform and storage banks,
depth and stencil, texture formats, tiled storage, blend state, pipeline
persistence, shader caching, and canonical readback. The new scene tests check
grouped array and indexed draws, descriptor reuse, uniform content changes,
scissor preservation, and partial completion before an invalid vertex count.
They also accept an empty draw with the maximum first vertex. The scene tests
pass with push descriptors enabled and with ordinary sets forced.
The full GPU and translator suite also passes with ordinary sets forced.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --doc --locked
cargo test --workspace --all-targets --all-features --locked -- --include-ignored --nocapture --test-threads=1
NOVENA_DISABLE_PUSH_DESCRIPTORS=1 cargo test --workspace --all-targets --all-features --locked -- --include-ignored --nocapture --test-threads=1
cargo test --features draw-metrics --example draw_bench -- --include-ignored --nocapture --test-threads=1
NOVENA_DISABLE_PUSH_DESCRIPTORS=1 cargo test --features draw-metrics --example draw_bench -- --include-ignored --nocapture --test-threads=1
```
