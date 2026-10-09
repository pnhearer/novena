# 0041: required subgroup sizes

- Date: 2026-10-09
- Covers: translated stage requirements, device controls, pipeline keys and readback

## Evidence boundary

This change consumes the translator's existing `requires_subgroup_size_32`
metadata. It adds no instruction encodings, guest enums, signatures or object
layouts. The earlier translation and persistence boundaries remain those in
[shader translation](0017-shader-translation.md),
[compute pipelines](0025-compute-pipelines.md) and
[startup shaders](0031-startup-shaders.md).

The host rules come from public documentation for
[stage size requirements](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineShaderStageRequiredSubgroupSizeCreateInfo.html),
[device features](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceSubgroupSizeControlFeatures.html),
[device limits](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceSubgroupSizeControlProperties.html)
and [pipeline validity](https://docs.vulkan.org/spec/latest/chapters/pipelines.html).
The shaders are original experiments written with public subgroup operations.

## Implementation

Device creation enables size controls through version 1.3 core support or
`VK_EXT_subgroup_size_control`. Version 1 of the extension implies both controls;
later versions report feature bits. A stage that needs size 32 checks the enabled
control, size range and supported stage bits before attaching a required size.
Compute also requires full subgroups. Its literal local size must have a positive
X dimension divisible by 32, and its total must fit the subgroup count limit.
`LocalSizeId` and specialized local sizes have no supported execution contract
here and receive a clear error. Unsupported devices keep executing other shaders.

Drawing and compute dispatch retain cached translation metadata. Graphics startup
recipes pass it to pipeline creation. Whole pipelines, shader library parts and
compute dispatch caches distinguish required and unrestricted stages.
The older compute compilation helper now uses the contextual translation hook
and persists the requirement alongside module words in a new cache namespace.
Legacy word-only records cannot silently lose the requirement.

## Proof

The compute fixture dispatches 64 invocations and reads back four words from each.
It checks vote results, a full ballot, shuffle reads from every source lane from
0 through 31, and equality, lower and upper lane masks. Each required subgroup
produces lane numbers 1 through 32, a success value of 255, shuffle sum 528 and
ballot count 32. The fragment fixture performs the same operations over a full
64 by 64 target. Every pixel has the exact expected operation bytes, and each
lane appears 128 times.

Both tests alternate unrestricted and required pipelines twice on the same
context. Unrestricted results differ from the expected 32-lane results on the
local device with a default size of 64. Required results match exactly. Fragment
compilation also prewarms a recipe from translated metadata. The same check can
run with `NOVENA_DISABLE_PIPELINE_LIBRARIES=1` to cover whole-pipeline creation.

Temporarily omitting the required-size stage structures made both GPU regression
tests fail. Restoring them made both pass. Formatting, strict default and
all-feature Clippy, and complete default and all-feature test suites pass,
including ignored GPU and external translator checks.

Run the subgroup checks with:

```sh
cargo test --workspace --all-features --lib gpu::subgroups::tests -- --include-ignored --test-threads=1
```

Unit checks cover unavailable controls, unsupported stages, incompatible size
ranges, unavailable full subgroups, local workgroup limits, persisted requirement
bits and metadata forwarded by reopened startup recipes.

## Limits

The GPU regression tests require a device whose default subgroup size is 64 and
which supports required size 32 in compute and fragment stages. They do not skip
when that contract is absent. Graphics does not require every fragment subgroup
to be full, since that control applies to compute. Other graphics stages remain
subject to the device's stage support, including vertex stages.
