# 0037: deferred execution beyond direct drawing

- Date: 2026-10-09
- Covers: indirect dispatch and drawing, instance ranges, counter reports,
  render predicates, buffer transfers and decoded texture clears.

## Evidence

Only the retained signatures, shapes, census and provenance notes informed
the guest interpretation. Public Vulkan documentation informed execution.
No new observation of a guest program was performed.

[Signatures 0002](../signatures/0002-command-buffer.md) records the likely
three-count DispatchCompute shape, the likely DrawArraysInstanced shape,
the likely ClearBuffer address, size and value, and ReportCounter's selector
and destination. [Signatures 0009](../signatures/0009-draw-state.md) leaves the
last two instancing arguments and instance semantics open.
[Signatures 0010](../signatures/0010-remaining-command-state.md) records 10,422
dispatches and 15,633 buffer fills. It explicitly leaves execution semantics
unresolved. The census records 10,421 array instancing calls and 62,525 reports.

The census requested DispatchComputeIndirect, DrawArraysIndirect,
DrawElementsIndirect, DrawElementsInstanced, ResetCounter, SetRenderEnable,
SetRenderEnableConditional and CopyBufferToBuffer but records zero calls to
them. Their names establish availability of requested entry points, without
establishing signatures, tokens, layouts or effects.

[Signatures 0006](../signatures/0006-copies.md) leaves copy records unresolved.
[Signatures 0011](../signatures/0011-command-evidence.md) retains the six
ClearTexture calls and their complete shapes. The clear pointers and unusual
return classification remain unresolved.

## Explicit hypotheses

Execution requires a CommandContract selected by a Rust host. Without it,
these commands remain recorded and submission reports unsupported execution.
Recording alone never reads indirect parameters or writes a counter report.

| Command | Hypothesis after the command object |
|---|---|
| DispatchCompute | Three unsigned group counts in axis order. |
| DispatchComputeIndirect | Arena address of three little-endian 32-bit group counts. |
| DrawArraysInstanced | Primitive, first vertex, vertex count, first instance, instance count. |
| DrawElementsInstanced | Primitive, index type, index count, index address, signed 32-bit base vertex, first instance, instance count. |
| DrawArraysIndirect | Primitive, arena address of a Vulkan-style four-word indirect record. |
| DrawElementsIndirect | Primitive, index type, index base address, arena address of a Vulkan-style five-word indexed indirect record. |
| ClearBuffer | Arena address, byte length, repeated 32-bit value. |
| CopyBufferToBuffer | Source address, destination address, byte length, zero flags. |
| ResetCounter | Host-selected occlusion token. Starts a new accumulation interval. |
| ReportCounter | Host-selected counter token, arena destination for a 16-byte report. |
| SetRenderEnable | Boolean draw enable. |
| SetRenderEnableConditional | Arena address of a 32-bit predicate, host-selected zero or nonzero mode. |
| ClearTexture | Seven opaque argument words passed to a host decoder. |

The report contains an accumulated visibility result followed by a device
timestamp, each as a little-endian 64-bit word. Timestamp-only reports use
zero in the first word. This layout continues the old 16-byte report
hypothesis. Device timestamps replace the old fabricated recording-time
sequence. Counter selectors, timestamp units and conversion to guest time
remain unknown. The implementation uses the queue's supported timestamp
precision and makes no claim about comparison across device timestamp wrap.

Indirect layouts follow
[dispatch parameters](https://docs.vulkan.org/refpages/latest/refpages/source/VkDispatchIndirectCommand.html),
[array parameters](https://docs.vulkan.org/refpages/latest/refpages/source/VkDrawIndirectCommand.html)
and [indexed parameters](https://docs.vulkan.org/refpages/latest/refpages/source/VkDrawIndexedIndirectCommand.html).
This is a host experiment, not a recovered guest layout. Each command
executes one indirect record. Count-buffer and multi-draw entry points remain
unsupported. Indirect firstInstance requires the public Vulkan device feature.

Vertex divisor zero selects vertex fetches. Divisor one selects instance
fetches, including the first instance. Larger divisors remain unsupported.
The existing host-provided vertex-state spacing and format choices still
apply. Bounds checks cover every vertex or index and every instance fetch.

Compute uses exactly one retained translated compute stage. Explicit
stage/index mappings connect buffer bindings to reflected uniform or storage
descriptors. Descriptor arrays, images and samplers remain unsupported in this
executor. Translation errors and pending compute stages report unsupported
execution instead of substituting output. Graphics retains the existing
background pipeline behavior: a draw with a pending pipeline is skipped. The
readback tests wait for graphics compilation before measuring visibility.

The clear decoder supplies a texture key, four finite float components and a
four-bit channel mask. It must reject any opaque record it cannot interpret.
There is no inferred guest region layout. Execution uses the existing bounded
whole-image color clear path. Existing supported buffer-to-image and
image-to-image copies keep their documented limits.

## Vulkan execution and ordering

The arena now permits indirect reads. Bounds and alignment checks happen
before issuing device commands. Indirect parameters are read from completed
canonical arena storage for validation and then consumed by Vulkan from that
same buffer. Signed index adjustment and firstIndex are each applied once. The original
index base must belong to a live pool. Argument records cannot overlap an
attachment that synchronization may overwrite.

[Indirect dispatch](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdDispatchIndirect.html)
and [indexed indirect drawing](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdDrawIndexedIndirect.html)
execute the recorded work.
[Buffer fills](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdFillBuffer.html)
and copies use transfer commands with explicit memory dependencies.
Overlapping buffer copies, out-of-pool ranges, misaligned records and unknown
tokens fail visibly.

Guest command order spans Vulkan render-pass splits. Transfers, dispatch,
counter reset or report, and predicate changes finish earlier draws. The next
draw restores attachments and retained state. Command boundaries invalidate
cached index bounds because earlier writes may have changed index values. Clears and copies execute even
when drawing is disabled. They preserve the selected targets for later draws.

An occlusion interval measures each completed drawing group with
[begin](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdBeginQuery.html)
and [end](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdEndQuery.html)
inside its Vulkan render pass. Results accumulate across pass splits until
reset. Before reset, the accumulated result is zero. The non-precise query
mode guarantees visibility information, not an exact sample count.
Reports flush earlier draws and
[write a timestamp](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdWriteTimestamp.html).
The timestamp is
[copied to the arena](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdCopyQueryPoolResults.html)
with the wait flag. The accumulated visibility value is written into the
mapped arena after completed readback. Submission downloads the arena through
the host memory callback.

Conditional rendering uses a synchronous arena predicate read after prior
work completes. Its zero/nonzero semantics follow the public
[conditional rendering predicate](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdBeginConditionalRenderingEXT.html).
It does not require that optional device extension. State applies to subsequent
draw commands in the recording. It does not decode an unobserved guest
comparison record.

This implementation chooses completion at command boundaries for correctness.
It does not claim asynchronous report delivery or efficient overlap of compute,
transfers and drawing.

## Original experiments

The synthetic command tests compile original GLSL through glslangValidator and
validate SPIR-V with spirv-val. They require a working Vulkan device and fail
if it is absent. No test assets come from guest software.

Buffer readback covers direct and indirect compute, device-generated group
counts, 32-bit fill values, non-overlapping copies, deferred timestamps, and
guard bytes around each 16-byte report. It checks unknown tokens, invalid
alignment, overlapping copies and allocation ends.

Pixel readback covers two instances, a nonzero first instance, per-instance
vertex input, indirect array drawing with device-generated parameters,
16-bit and 32-bit indexed indirect drawing with nonzero firstIndex and negative
base vertex, and direct indexed instancing. It checks predicate suppression
and inversion, zero-instance draws, instance-range bounds, allocation bases,
attachment aliasing and stale index bounds after a fill.

The same command recording includes compute, fills, copies, predicate changes,
drawing, reports, image copies and a decoded texture clear. Readback checks
both the copied image and the cleared source, and compares canonical host
arena pixels with device pixels. Reports distinguish visible draws from
suppressed draws. Recording tests retain all seven 64-bit arguments and keep
independent command objects separate.

The translator test adapter explicitly disables optional 64-bit floating-point
shader support. The current translator options require that capability field;
the compile diagnostic identified the missing initializer. This is a test
integration choice and adds no guest interpretation.

These experiments establish the chosen host contract. They do not establish
that an unobserved guest layout matches it.

## Verification

- Default workspace tests passed, 64 checks across 11 suites, with exit code
  zero.
- Workspace formatting check passed.
- Workspace clippy passed for default and all features, with all targets and
  warnings denied.
- The full all-feature workspace suite passed with ignored tests included and
  one test thread. All requested GPU and translator checks ran; the final
  process exited with zero.
- The combined test runner reported 148 passing checks across 31 suites,
  including nested translated-shader checks. Nested runners select individual
  cases explicitly; their other cases remain filtered.
- The generated translated-drawing test package also passed all-target clippy
  with warnings denied.
- Whitespace and publication checks passed for every added line and new file.
