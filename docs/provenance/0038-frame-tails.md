# 0038: frame tails and retained compilation resources

- Date: 2026-10-09
- Covers: compilation stage timing, startup parts, retained shader modules,
  completion polling, descriptor ownership, and mapped-memory selection

## Sources

This work extends the original synthetic experiments in provenance 0037.
No external program data, proprietary implementation, or excluded documentation
was used. The implementation uses existing project code and original experiments.

Public documentation defines the relevant rules:

- [Library subsets](https://docs.vulkan.org/refpages/latest/refpages/source/VkGraphicsPipelineLibraryCreateInfoEXT.html)
  separate input and output state from shader state.
- [Shader module lifetime](https://docs.vulkan.org/refpages/latest/refpages/source/vkDestroyShaderModule.html)
  permits modules to remain alive across pipeline creations.
- [Memory properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkMemoryPropertyFlagBits.html)
  distinguish host-cached memory from uncached coherent memory. Uncached host
  reads can have severe performance costs.

## Cause and changes

The largest measured synchronous cost was copying the mapped arena back to the
host. The allocator preferred device-local coherent memory without considering
host caching. The timing split places most download time in mapped reads rather
than device completion. Every submission copied the whole live pool, including
submissions whose draw was still waiting for compilation.

The allocator now prefers host-cached coherent memory. Device-local memory wins
among choices with the same caching property. Uncached coherent memory remains
an allocation fallback. The existing host callbacks, completion barriers, and
whole-pool synchronization contract remain intact.

Compiler construction creates six common input parts and eight output parts.
Input parts cover triangle lists, strips, and fans with one float4 attribute,
either tightly packed or in a 32-byte stream at offset eight. Output parts cover
one through four unblended targets with full write masks, with or without depth.
Other states still compile on demand. Common parts need no shader reflection or
shader module, and their keys remain independent of shader words and layouts.
Shader combinations still validate interfaces before reusing those parts.

A compiler-owned cache retains modules by complete original words, stage, and
storage choice. Shader pipelines also retain their module owners. Only workers
access the module-cache mutex. Failed module creation inserts no cache entry.
Changing raster or output state can reuse the same module, as can whole-pipeline
fallback compilation.

Completed requests publish immutable results through `OnceLock`. Skip polling
reads that result without a mutex. A separate mutex and condition variable serve
explicit waits. Publication holds that mutex to avoid a missed wakeup. Ready and
failed results can be read even while a waiter holds the mutex.

Descriptor caches belong to the exclusively borrowed graphics service. Each
pipeline handle selects its own cache. Draw preparation mutates that cache
without a pipeline mutex, and resource invalidation clears the service caches.
Existing retained descriptors still own their pools until queued draws finish.
The backend's existing submission mutex and bounded worker channel remain.

All links run on workers, including devices that advertise fast linking. This
removes driver linking from draw recording but can require another skipped draw
before readiness. One wait deadline still covers both subsets and the link.
The capability query continues to report device support, rather than policy.

## Measurement

The baseline is revision `effc7b0` with timing instrumentation only. Both versions
use the release synthetic first-use test, default skip policy, an empty
application cache, and disabled driver disk caching. Each version launches five
fresh processes and draws 24 translated fragment variants per process. Setup,
translation, the initial clear, and startup part creation precede each measured
first draw. Every variant verifies completed pixel output.

A frame sample is the longest recording-plus-submission interval for one
variant, including completion and canonical readback. It is not presentation
spacing. First-use samples include retries and the one-millisecond sleep between
skipped draws. Quantiles pool all 120 variant samples per version and use the
nearest-rank rule, `ceil(p * count)`, with one-based ranks.

| Interval, milliseconds | Before p50 | Before p95 | Before p99 | After p50 | After p95 | After p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Longest frame per variant | 11.015 | 20.501 | 21.374 | 0.398 | 0.624 | 1.960 |
| First use | 20.216 | 37.606 | 42.320 | 2.832 | 3.245 | 6.389 |

The [frame samples](0038-frame-samples.csv) retain all observations in nanoseconds.
The [stage samples](0038-stage-samples.csv) retain total time, call count, and
maximum call time per process. Stage counters also include each variant's clear
submission, which is outside the frame samples. Worker stages overlap draw
stages. Their totals must not be added to obtain frame cost.

The maximum call across the five processes distinguishes the suspected costs:

| Stage, microseconds | Before | After |
| --- | ---: | ---: |
| Descriptor and pipeline layouts, including render pass | 50.186 | 8.922 |
| Shader lowering and module creation | 53.404 | 25.405 |
| Input driver creation during variants | 100.471 | 0 |
| Fragment driver creation | 2051.190 | 1969.188 |
| Output driver creation during variants | 13.651 | 0 |
| Link, including reflection | 37.163 | 48.181 |
| Descriptor cache lock access | 0.301 | 0 |
| Driver cache read-lock access | 0.460 | 0.509 |
| Completion polling | 0.830 | 5.553 |
| Complete host download | 24270.239 | 1981.118 |
| Download device completion | 4943.979 | 1945.820 |
| One mapped read chunk | 7410.949 | 48.036 |

The earlier exploratory run also observed an 18.135 ms fragment driver call on
a worker. The final baseline had shorter compilation calls, so that exploratory
maximum is not substituted into the paired table. Layout creation, linking,
output misses, and lock contention did not explain the largest tails here.
Precreation and retained modules remove repeated work but the cached arena
selection accounts for the large mapped-read reduction. Module creation itself
was already small and was not the main cause of the frame tails.

Each library process creates six startup input parts, one raster part, 24
fragment parts, eight startup output parts, and 24 worker links. Timed variants
create no input or output parts. Fallback processes create 24 whole pipelines.
The retained-module device test checks reuse, survival across user and cache
lifetimes, storage-key separation, and rejection of malformed words.

After configuring the translator and build and temporary storage through the
environment, reproduce the sample with:

```sh
cargo test --release --all-features --test shadowbox_drawing translated_pipeline_first_use -- --include-ignored --nocapture
```

## Verification and limits

Formatting, default and all-feature Clippy with warnings denied, default tests,
all-feature documentation tests, and all-target all-feature tests pass. The
all-feature run includes every ignored device test and the configured translator
without skip messages. All 14 translated drawing tests also pass with libraries
disabled.

The first-use test verifies part counts and actual pixels. Both compilation modes
create exactly one vertex module and 24 fragment modules for the 24 pipelines,
including the whole-pipeline fallback. Queue pressure with
one worker and one slot exercises partial admission, persistence, a shared wait
budget, and worker linking. Whole-pipeline fallback uses the same retained-module
cache and completion publication.

Startup precreation adds work before the first draw. These synthetic measurements
do not establish startup or throughput parity for larger workloads. Host-cached
memory may be less efficient for device access on a discrete device. Devices
without a suitable cached coherent type retain the original uncached fallback.
Results vary with machine load, and 120 observations give limited p99 precision.
No claim is made that all frame spikes or all driver compilation tails are gone.
