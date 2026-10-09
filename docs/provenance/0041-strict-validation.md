# 0041: strict execution validation

- Date: 2026-10-09
- Covers: Vulkan validation setup, floating-depth transfers, and GPU test failures

## Sources and scope

This work checks the host backend with the repository's synthetic tests. It
adds no guest signatures, enum meanings, or object layouts. The execution
contracts remain those recorded in [command execution](0037-command-execution.md),
[textured drawing](0030-textured-blended-drawing.md), and
[combined execution](0039-combined-execution.md).

The public sources are the Vulkan reference for
[layer settings](https://docs.vulkan.org/refpages/latest/refpages/source/VkLayerSettingsCreateInfoEXT.html),
[debug messengers](https://docs.vulkan.org/refpages/latest/refpages/source/VkDebugUtilsMessengerCreateInfoEXT.html),
the [validation layer settings](https://vulkan.lunarg.com/doc/view/latest/linux/khronos_validation_layer.html),
and [depth and stencil aspect copies](https://docs.vulkan.org/spec/latest/chapters/copies.html).
All shader inputs and pixel expectations come from original test fixtures.

## Findings and fixes

| Finding | Fix |
| --- | --- |
| Enabling a layer through the environment did not make warnings fail a library test. | A context now owns a debug messenger and a counter. Any warning or error fails strict validation after teardown. The test runner also fails on a reported message, including messages from worker threads and nested tests. |
| The submit-time synchronization setting produced a deprecation warning with layer version 1.4.363. An older reference layer rejected the replacement setting. | Select the full-validation setting for that version and later versions. Use the submit-time setting for earlier versions. |
| GPU instrumentation reported that device creation had omitted required shader atomics, memory-model features, and scalar block layout. | GPU mode checks support and explicitly enables vertex and fragment stores and atomics, `vulkanMemoryModel`, `vulkanMemoryModelDeviceScope`, and `scalarBlockLayout`. |
| Running core and GPU-assisted checks together produced a configuration warning. | Run core plus synchronization checks in one pass and GPU-assisted checks in another pass. |
| GPU instrumentation requested ray and mesh checks although these execution paths use neither. | Configure those two checks off. Descriptor, device-address, shader, and indirect-buffer checks stay enabled. |
| GPU-assisted validation reported 186 instances of `VUID-vkCmdCopyBufferToImage-pRegions-07931` in the depth/stencil raster test. | Enable the advertised `VK_EXT_depth_range_unrestricted` device extension for raw floating-depth copies. Preserve depth and stencil bytes, including floating values outside the unit range. |

The complete synchronization pass reported no missing barriers, incorrect image
layouts, hazards between submissions, or descriptor misuse. Existing barriers
were sufficient for the exercised paths.

## Floating-depth investigation

The reported depth data was already inside the unit range. Inspection of each
source depth plane found no invalid values. An added queue wait did not change
the result. A reduced upload with every depth value equal to `0.75` reproduced
six messages after exact depth and stencil readback passed. The message offsets
advanced by twenty bytes and included offsets between four-byte depth values.
This is evidence of a layer addressing defect, not an image-layout or barrier
failure. Layer versions 1.4.357 and 1.4.363 both reported it.

Vulkan specifies separate buffer planes for depth and stencil aspect copies.
The depth plane of `D32_SFLOAT_S8_UINT` uses four bytes per texel, and the
stencil plane uses one byte per texel. The library already follows that rule.

The device now enables unrestricted depth when it is advertised. This makes
the raw floating-depth transfer contract accept values outside `[0, 1]` under
the public copy rules. It also avoids the erroneous unit-range diagnostic.
This is a capability choice and a layer workaround. It does not repair the
layer's address calculation or change the library's plane packing. No messages
are filtered and no validation check is disabled to exclude this finding.

`depth_stencil_upload_keeps_planes_and_offset_exact` copies a 64 by 64 image
from a nonzero arena offset. It checks normalized `0.75` values, a second set
containing `-0.5` and `1.5`, and an independent stencil plane against exact byte
readback. The surrounding source bytes are distinct from the depth plane. The
normalized case failed strict GPU validation before device extension selection
changed and passes afterward.

The driver also printed a conformance notice directly to standard error. This
notice describes the driver build and does not come from the validation
messenger. Logs retain it alongside test output.

## Reproduce the checks

Provide a writable `CARGO_TARGET_DIR`, shader tools, the validation layer, and
the external translator through `NOVENA_SHADOWBOX_PATH`. Run:

```sh
python3 scripts/check-validation.py
```

The runner executes all workspace tests, including ignored GPU tests and
translator checks, in both modes. It keeps complete logs in the configured
build directory. It runs every test binary after a failure and returns failure
if either pass reports a warning, an error, or a failed test. Message duplicate
limits are disabled. The all-feature mode rejects an absent translator instead
of allowing translator tests to skip.

`--features vulkan` runs the backend checks without translator integration.
GPU CI uses this selection for both modes and also runs the native window
checks. `--mode sync` selects core and synchronization checks. `--mode gpu`
selects GPU-assisted checks. GPU mode requires the instrumentation features;
an unavailable device fails setup instead of falling back to CPU execution.

`VULKAN_VALIDATION=sync` and `VULKAN_VALIDATION=gpu` also enable strict checking
for individual tests and applications. The callback covers instance creation,
device execution, and teardown. It prints full messages and never unwinds into
Vulkan. Informational startup messages are outside the warning and error gate.

The subprocess probes inject one warning and one error through a live debug
messenger. Both child processes fail with the retained message count. Expected
probe diagnostics remain captured by their parent test.

The complete default and all-feature test suites pass. Both strict passes with
layer version 1.4.363 return zero validation messages, including ignored GPU and
translator checks. Formatting and strict default and all-feature Clippy pass.
The failure probes also pass with reference layer version 1.4.357.

## Confidence and limits

Synchronization validation covers the exercised Vulkan dependencies. GPU
instrumentation checks shader accesses where the layer supports instrumentation.
Neither pass proves correctness for commands, formats, or shader interfaces
that the synthetic tests do not exercise. No guest behaviour is inferred from
these checks.

Devices without the unrestricted-depth extension retain the unit-range copy
constraint. The raw floating-depth regression requires that extension. Native
window CI remains a separate presentation check; the local full-suite run used
the native headless presentation test.
