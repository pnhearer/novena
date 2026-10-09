# 0037: graphics libraries and first-use latency

- Date: 2026-10-09
- Covers: graphics pipeline subsets, background compilation, pending-draw policy,
  persistent driver data, startup recipes, and translated synthetic measurements

## Sources

Provenance 0027 through 0032 and 0036 establish the existing draw contracts,
translated synthetic programs, shader interfaces, cache identities, and startup
recipes. This change adds host execution choices without adding an observed
function signature, object layout, or enum interpretation.

The implementation follows public Vulkan documentation:

- [Graphics library subsets and linking](https://docs.vulkan.org/refpages/latest/refpages/source/VkGraphicsPipelineCreateInfo.html)
  define vertex input, pre-rasterization, fragment shader, and fragment output
  state, complete layouts, and compatible render passes.
- [Library capability](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceGraphicsPipelineLibraryFeaturesEXT.html)
  defines device feature discovery and enablement.
- [Fast linking](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceGraphicsPipelineLibraryPropertiesEXT.html)
  identifies devices suitable for draw-time linking.
- [Pipeline creation](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateGraphicsPipelines.html)
  permits concurrent access to an internally synchronized cache.
- [The extension proposal](https://docs.vulkan.org/features/latest/features/proposals/VK_EXT_graphics_pipeline_library.html)
  describes separate compilation and linking without link-time optimization.

Only existing original synthetic programs and their translated output enter
these experiments. No external program data or proprietary source was used.

## Execution and ownership

Both library extensions and the graphics-library feature must be available.
Otherwise workers compile whole pipelines through the existing path.

A library key includes only the state its subset consumes. Vertex input includes
bindings, attributes, and topology. Pre-rasterization includes the vertex words
and raster state. Fragment shader includes the fragment words and depth/stencil
state. Fragment output includes attachment blending and write masks.
Shader subsets also include storage choice, the complete descriptor layout
signature, and render-pass compatibility. Output compatibility includes color
count and depth attachment presence. The supported sample count remains one.

Every new shader combination checks attribute locations, varying interfaces,
and color outputs before reusing parts. Workers retain complete validation of
shader resources and device limits. An equivalent layout can reuse a shader
part across changes to the other shader's words. Changed descriptor layouts
require new shader parts.

One bounded worker pool accepts missing parts before the caller waits.
Partial queue admission retains accepted requests. Later draws and startup
polling retry the remaining parts. Duplicate requests share completed or failed
results. Explicit retry forgets failed jobs without discarding successful parts.

Ready parts link during a draw only when the device advertises fast linking.
The link uses no compilation-cache lock or disk I/O. Other devices queue the
link on a worker. Linked pipelines retain the compatible layout and render pass
owner. A ready executable remains in the existing collision-checked draw cache.

Pipeline-cache creation calls share a read lock. Snapshots take an exclusive
lock. A separate writer lock serializes snapshot persistence so an older save
cannot replace a newer snapshot. Cache identity, envelope validation, corruption
recovery, and startup recipe encoding remain compatible. All four parts use the
persistent driver cache. Linking uses no link-time optimization.

Startup recipes enqueue the same reusable parts. Pending-count polling advances
partial admission and schedules the executable link on a worker once the parts
finish. A draw can also complete the assembly directly.

## Host policy

A Rust host calls `Instance::set_pending_draw_policy` with
`PendingDrawPolicy::Skip` or `PendingDrawPolicy::Wait(Duration)`.
Skip is the default. Wait accepts zero through 16 milliseconds.
Larger budgets are rejected without changing the selected policy.
Cache reconfiguration preserves the policy.

One deadline covers waiting for all parts and an asynchronous link.
A timeout or full queue skips that draw for the current frame; later commands
continue. Compilation failures stay visible. This budget bounds compilation
waiting, not command recording, GPU completion, attachment transfers, or the
driver's fast-link call. It is not a total frame-time limit.

`graphics_library_support` reports enabled library and fast-link capability.
`graphics_compilation_stats` reports successful subset, link, and whole-pipeline
creation counts. These distinguish measured library execution from fallback.
Setting `NOVENA_DISABLE_PIPELINE_LIBRARIES=1` before device creation forces the
whole-pipeline fallback. `NOVENA_DISABLE_FAST_LINKING=1` retains libraries and
forces links onto workers.

## Measurement

The release test `translated_pipeline_first_use` launches five fresh processes.
Each draws 24 new translated fragment variants with one unchanged vertex shader,
input layout, and output state. Translation, initial device setup, vertex writes,
and the initial clear occur outside the timed interval. The application pipeline
cache is empty and driver disk shader caching is disabled.

First-use time starts before recording the first draw that needs the new
pipeline and ends after its expected color appears in completed pixel readback.
It is an upper bound on draw execution latency. The test retries skipped draws
with a one-millisecond sleep. Frame cost is the longest recording-plus-submission
interval for that variant, including GPU completion and canonical readback.
Every variant verifies its pixel color, so readiness polling alone cannot
satisfy the measurement.

The baseline uses revision `1d04491` with the timing harness and a test-adapter
update for the translator's current capability signature. The update leaves
64-bit floating-point support disabled. The implementation is unchanged for
the baseline. The final measurements use a discrete GPU with a Mesa driver
on the same machine. Timing varies with concurrent machine load.

Each column below is the median of five process-level quantiles, in milliseconds.
The maximum row is the largest observation across all processes.
Individual process summaries are in [the samples](0037-pipeline-samples.csv).

| Measurement | Before, skip | Libraries, skip | Libraries, wait 2 ms |
| --- | ---: | ---: | ---: |
| First use, p50 | 30.403 | 24.283 | 18.276 |
| First use, p95 | 44.272 | 41.370 | 26.381 |
| First use, maximum | 48.392 | 45.707 | 32.326 |
| Longest frame per variant, p50 | 17.034 | 12.163 | 18.275 |
| Longest frame per variant, p95 | 21.450 | 20.949 | 26.058 |
| Longest frame, maximum | 25.010 | 23.724 | 32.325 |

Skip reduces the median first-use measurement by 20.1 percent and the median
frame-spike measurement by 28.6 percent in these runs. Waiting reduces first-use
latency further but increases frame spikes. The results do not establish that
all stutter is gone. Tail frame costs still include synchronous GPU and memory
work, and a wait policy deliberately spends more time in the requesting frame.

For each library process, counters prove exactly one vertex-input part, one
pre-rasterization part, 24 fragment parts, one output part, and 24 links.
The fallback creates 24 whole pipelines. This reuse result is independent of
timing noise.

Reproduce after configuring the external translator and the build, temporary,
and driver-cache storage locations through the environment:

```sh
cargo test --release --all-features --test shadowbox_drawing translated_pipeline_first_use -- --include-ignored --nocapture
NOVENA_PIPELINE_TIMING_WAIT=1 cargo test --release --all-features --test shadowbox_drawing translated_pipeline_first_use -- --include-ignored --nocapture
NOVENA_DISABLE_PIPELINE_LIBRARIES=1 cargo test --release --all-features --test shadowbox_drawing translated_pipeline_first_use -- --include-ignored --nocapture
NOVENA_PIPELINE_TIMING_WAIT=1 NOVENA_PIPELINE_TIMING_PRESSURE=1 NOVENA_DISABLE_FAST_LINKING=1 cargo test --release --all-features --test shadowbox_drawing translated_pipeline_first_use -- --include-ignored --nocapture
```

The pressure case uses one worker and one queue slot with private persistence,
then removes its temporary cache after the workers drain. It verifies eventual
pixel completion and the same part-reuse counts despite partial admission.

## Verification and limits

Formatting, default and all-feature Clippy with warnings denied, default tests,
all-feature documentation tests, and all-target all-feature tests pass.
The GPU run includes ignored tests and the configured external translator.
The complete translated drawing suite also passes with libraries disabled.

CPU tests check subset-key separation, a shared deadline across a compiling
request and a queued request, queue saturation, completion wakeup, and retained
failure diagnostics. GPU tests check actual translated output, indexed draws,
uniform and storage layouts, depth/stencil, raster state, multiple targets,
textures, blending, persistence corruption recovery, and startup prewarm.
The pressure measurement also verifies waiting and worker-based linking.

The existing fresh-process startup test still loads two translations without
runtime translation and queues the saved recipe. The all-feature run reports
63.147 ms cold and 53.187 ms warm median time from instance creation through
first pixel completion over 40 trials each. Those intervals include setup and
are distinct from the pipeline first-use interval above.

Library pipelines use fast linking without an optimized replacement. Device
throughput can therefore differ from whole-pipeline compilation. These small
synthetic scenes do not establish throughput parity or performance on every
driver. No validation layer was installed for this run.
