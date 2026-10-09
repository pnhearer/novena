# Signatures 0011: the eight remaining commands and resolver

This revisits the unresolved rows in [signatures 0010](0010-remaining-command-state.md).
Only the existing observations and their earlier interpretations are used.
Implementation choices and checks are in [provenance 0028](../provenance/0028-command-evidence.md).
Names below omit the three-letter prefix.

## Evidence boundary

The census is a four-minute run; the three shape reports are separate shorter
runs. Their totals differ. Each report samples at most the first 2,048 calls
per function. The tables below link the complete blocks, including every
register, trailing pointee, return classification and change count. Shapes
0001 has registers only; 0002 and 0003 add the first 64 bytes of readable
pointees. No address values, object identities, nested pointees, array strides
or call sequence survive these reports.

[Provenance 0007](../provenance/0007-first-signatures.md) describes matching
object addresses across setters and judging leftovers against surrounding
calls. That is a description of the original analysis, not a retained trace.
[Signatures 0003](0003-objects.md) supplies two relative-order facts relevant
here: sampler handle queries use ids previously registered, and texture
storage-size results are later supplied to SaveZCullData. Neither establishes
that a particular sampler query feeds a particular binding or which texture
feeds a particular save. [Provenance 0005](../provenance/0005-first-census.md)
establishes name request, non-null address return, then invocation of that
address. These are the available order facts. There is no recoverable order
between any of the eight commands and a particular resolved binding, draw,
dispatch, barrier, wait, clear, submission, or save/restore partner. Equal
counts and matching aggregate values do not prove adjacency or identity.

All eight commands have x0 as a readable command buffer. Shapes 0002 and
0003 show recording changes there, rather than in their proposed input
objects. No changed input word is reported. Most return x0 classes remain
address-shaped and cannot distinguish a void return from an object return.
The implementations use the existing zeroed result-register convention;
this is an implementation choice, not a recovered return contract.

## Complete shape blocks

Each cell gives that report's call total and links its entire function block.
ClearTexture has six samples and returns per report; DeviceGetProcAddress has
1,070; the other seven commands have 2,048 samples and returns per report.

| Function | Shapes 0001 calls | Shapes 0002 calls | Shapes 0003 calls |
|---|---:|---:|---:|
| CommandBufferBindVertexAttribState | [112158](../shapes/0001-program-a-startup.txt#L462-L481) | [110476](../shapes/0002-program-a-startup-pointees.txt#L1810-L1908) | [112001](../shapes/0003-program-a-startup-answers.txt#L1810-L1908) |
| CommandBufferBindVertexStreamState | [112158](../shapes/0001-program-a-startup.txt#L502-L521) | [110476](../shapes/0002-program-a-startup-pointees.txt#L1978-L2075) | [112001](../shapes/0003-program-a-startup-answers.txt#L1978-L2075) |
| CommandBufferBindSeparateSampler | [152563](../shapes/0001-program-a-startup.txt#L402-L421) | [150368](../shapes/0002-program-a-startup-pointees.txt#L1626-L1691) | [152413](../shapes/0003-program-a-startup-answers.txt#L1627-L1692) |
| CommandBufferClearTexture | [6](../shapes/0001-program-a-startup.txt#L582-L601) | [6](../shapes/0002-program-a-startup-pointees.txt#L2217-L2286) | [6](../shapes/0003-program-a-startup-answers.txt#L2217-L2286) |
| CommandBufferFenceSync | [8620](../shapes/0001-program-a-startup.txt#L742-L761) | [8495](../shapes/0002-program-a-startup-pointees.txt#L2730-L2801) | [8607](../shapes/0003-program-a-startup-answers.txt#L2730-L2800) |
| CommandBufferSaveZCullData | [56018](../shapes/0001-program-a-startup.txt#L862-L881) | [55199](../shapes/0002-program-a-startup-pointees.txt#L3142-L3223) | [55940](../shapes/0003-program-a-startup-answers.txt#L3141-L3222) |
| CommandBufferRestoreZCullData | [47405](../shapes/0001-program-a-startup.txt#L842-L861) | [46712](../shapes/0002-program-a-startup-pointees.txt#L3100-L3141) | [47339](../shapes/0003-program-a-startup-answers.txt#L3099-L3140) |
| CommandBufferSetPolygonOffsetClamp | [112158](../shapes/0001-program-a-startup.txt#L942-L961) | [110476](../shapes/0002-program-a-startup-pointees.txt#L3441-L3539) | [112001](../shapes/0003-program-a-startup-answers.txt#L3440-L3538) |
| DeviceGetProcAddress | [1070](../shapes/0001-program-a-startup.txt#L1402-L1421) | [1070](../shapes/0002-program-a-startup-pointees.txt#L4968-L5033) | [1070](../shapes/0003-program-a-startup-answers.txt#L4968-L5033) |

## Hypotheses and implemented boundary

Counts here are from [census 0001](../census/0001-program-a-startup.txt).
Every command starts with the command buffer object in x0.

| Function | Census calls | Hypothesis | Confidence | State-only outcome |
|---|---:|---|---|---|
| CommandBufferBindVertexAttribState | 135723 | x1 count, x2 pointer to attribute state storage | likely; open element layout | Retain count and base reference; snapshot known base settings; additional snapshots require explicit host spacing. |
| CommandBufferBindVertexStreamState | 135723 | x1 count, x2 pointer to inline stream state storage | likely; open element stride | Retain count and base reference; snapshot known base settings; additional snapshots require explicit host spacing. |
| CommandBufferBindSeparateSampler | 188184 | x1 stage, x2 index, x3 opaque sampler reference | likely recording; open handle versus pointer | Retain all three values without dereferencing or resolving the reference. |
| CommandBufferClearTexture | 6 | x1 texture, x2 optional view, x3 region, x4 clear value, x5 mask | open | Keep raw recording and Unimplemented; record roles and return contract are insufficiently established. |
| CommandBufferFenceSync | 10423 | x1 sync reference, x2 condition, x3 flags | likely recording; open condition and flags meanings | Retain the reference and two integers; do not signal, wait or mutate sync state. |
| CommandBufferSaveZCullData | 67741 | x1 graphics destination, x2 extent | likely recording; open format and texture association | Retain address and size; do not read or write saved data. |
| CommandBufferRestoreZCullData | 57325 | x1 graphics source, x2 extent | likely recording; open restored state | Retain address and size separately from save; do not restore data. |
| CommandBufferSetPolygonOffsetClamp | 135723 | d0, d1, d2 are three floats, possibly factor, units and clamp | open | Retain three raw floating-register words for the opt-in host experiment; count, width and order remain open. |
| DeviceGetProcAddress | 1070 | x0 device or null, x1 text name, result x0 callable address | firm signature; open address construction | Keep Unimplemented; exact secondary request names and host callable mappings are absent. |

The six successful handlers extend the recording boundary without claiming
new execution contracts. Existing host-configured vertex execution and
polygon-offset hypotheses remain documented in provenance 0028. Their
integer values retain all 64 register bits. Counts
and sizes do not cause program-memory traversal. The opt-in draw experiment
may snapshot up to 16 known state objects using explicit host spacing,
without inferring a memory layout from these reports. A
zero count carries no first-object snapshot. Unknown or mismatched state
objects remain unknown, including when their numeric addresses are readable.

### Vertex attribute and stream bindings

[Signatures 0002](0002-command-buffer.md) already rates both count/pointer
contracts likely. Attribute count is 1, 2 or 5, stream count is 1, and each
pointer has four distinct addresses in all three reports. Attribute x2 has
+0x00 wide in 79/2048, +0x08 through +0x14 other, +0x18 wide in 73/2048,
+0x20 wide in 79/2048 and readable as an address in 1738/2048; +0x28 through
+0x3c are other. Stream x2 has +0x00 in {0xc, 0x10, 0x20, 0x28},
+0x04 through +0x0c zero, +0x10 through +0x18 other, +0x1c zero,
+0x20 wide in 73/2048 and address in 158/2048, and +0x28, +0x30 and
+0x38 address in 158/2048. No word changes during either binding.

[Signatures 0003](0003-objects.md) associates attribute setters with format,
offset and stream index, and stream setters with stride and divisor.
Shapes 0003 shows VertexAttribStateSetFormat changing +0x00 in all 108 samples
and VertexAttribStateSetStreamIndex changing it in 49/108.
VertexStreamStateSetStride changes +0x00 in 44/48 and
VertexStreamStateSetDivisor changes +0x04 in 8/48. These support
state families and packed settings, not element sizes. Stream binding's
leading sizes agree with setter values, supporting the inline reading in
[signatures 0004](0004-pointers.md). The older phrase "counted pointer array"
in [signatures 0009](0009-draw-state.md) does not establish an array of
addresses and is corrected there. Attribute storage has no count-sized
leading address array; a later address at +0x20 is insufficient to invent one.

Both command-buffer +0x00 words change in 2048/2048. Their x3 through x7
and d0 through d7 have matching classes and values, also shared with
PolygonOffsetClamp except its first three floating registers. In particular,
x3 ranges 0..0x16, x4 is readable 3/2048, x5 156/2048, x6 158/2048, and
x7 101/2048. The complete linked blocks preserve these later pointees,
including their addresses and isolated float at +0x24. They are likely
leftovers rather than additional binding arguments. Between reports 0002
and 0003 only totals and x7's +0x10 address count (1 versus 2) differ.
Neither the number of state setters nor binding counts proves which objects
were supplied to a particular call or where its next draw occurs.

The implementation takes an owned copy of settings already retained by
setters for the base address and matching family. Without explicit host
spacing it does not locate further elements. This is useful partial state,
with an explicit first_settings field,
and is not a claim that a whole multi-element array has been decoded.

### Separate sampler binding

x1 is 1 or 5, x2 is 0..3, and x3 is one of five readable values in every
report. All sixteen words behind x3 are other and unchanged. Command-buffer
+0x00 changes 1985/2048 and +0x20 changes 63/2048. Stage and index positions
match the resolved BindSeparateTexture family; x3 is an input reference
whose representation is open. x4 is readable 1985/2048, x5 is 0 or 6,
x6 is 0, 1 or 0x400; x7 is mixed. Floating registers vary without an
established sampler argument. The linked blocks retain all trailing shapes.

The earlier objection in signatures 0002 was that a handle should not be
readable. Shapes 0003's DeviceGetSeparateSamplerHandle result is itself mixed
and readable in 20/48 samples. Its input ids are 0x100..0x12f, agreeing
with SamplerPoolRegisterSampler as described in signatures 0003. Thus
readability alone does not exclude a handle. It also does not prove that the
five binding references came from that query. No per-call identity is kept.
The query, registration and sampler initialization establish a sampler
family, not a pointer layout or descriptor-id conversion. An opaque
reference record is likely under either interpretation. It uses a separate
variant rather than asserting the existing BindHandle interpretation.

Between pointee reports x4's later wide counts differ, and x7 changes from
84 readable and 8 zero samples to 79 readable and 13 zero samples; no
leading argument or sampler pointee changes. No pool, sampler, descriptor
or pointed-to object is modified by the handler or submission.

### Texture clear

There are only six calls in each run, on six command buffers and six texture
addresses. x0 +0x00 changes six times. x1 starts with two zero words,
addresses at +0x08 and +0x10, zero at +0x18, other at +0x1c,
+0x20 in {0x500, 0x780}, other at +0x24, wide at +0x28, address at
+0x30 and zeros at +0x38 and +0x3c. This fits the texture family in
signatures 0003, without correlating a particular initializer or render target.
x2 is zero, x3 and x4 each have two distinct readable values, x5 is 0xf,
x6 is zero and x7 is a constant wide value. The floating registers mix wide,
zero and small values, with no established arguments. The result x0 has two
address-shaped values rather than the six command-buffer values; result x1
has six address-shaped values. No meaning for this difference is established.

x3 has zeros at +0x00 and +0x04, wide at +0x08 and +0x10, zeros at
+0x18 through +0x24, 1 at +0x28, zero at +0x2c, address at +0x30,
3 at +0x38 and zero at +0x3c. x4 has four leading zero words, 1 at
+0x10, zero at +0x14, address at +0x18, 3 at +0x20, zero at +0x24,
address at +0x28, zeros at +0x30 and +0x34, and address at +0x38.
Neither input changes. Reports 0002 and 0003 agree exactly.

[Signatures 0004](0004-pointers.md) groups these pointers with the open
copy records. [Signatures 0006](0006-copies.md) explicitly leaves their
fields undecoded. ClearColor's four-float input and 0xf mask suggest a
possible clear-value/mask pair, but an all-zero leading record can be many
things. Wide words can also hide two adjacent 32-bit fields under the
classification rules in [provenance 0008](../provenance/0008-memory-behind-arguments.md).
Consequently optional view, region and color roles remain open; neither
record is decoded, and this command remains unsuccessful.

### Fence

x1 is a readable sync-like object, x2 is zero and x3 is 1 in every report.
Its +0x00 is an address; +0x08 through +0x1c are zero; +0x20 is wide in
1024/2048; +0x30 and +0x34 are zero. In 0003, +0x28 is an address in
510/2048 and +0x38 is wide in 1021/2048 and address in 255/2048; in 0002
+0x28 and +0x2c are other and +0x38 has no readable addresses. None changes.
The command buffer +0x10 changes 2045/2048 and +0x20 changes 2048/2048.
x4 is readable or null in 1028/1020 samples, x5 is mixed, x6 is 0..2 and
x7 is 0 or 1. Later pointed-to objects and floating values lack a recovered
argument role and are left alone.

Signatures 0002 already rates the reference and two integer fields likely.
Signatures 0003 identifies SyncInitialize, SyncWait, SyncFinalize,
QueueWaitSync and WindowAcquireTexture's sync argument. Census counts are
10449, 15636, 10427, 5215 and 5215 respectively. These establish the related
object family but not a particular fence/wait/submit order, timeout unit,
signalled value or lifetime correlation. [Provenance 0010](../provenance/0010-first-run-on-novena.md)
assigns display pacing to the host. [Provenance 0011](../provenance/0011-running-past-the-first-frame.md)
concerns event and counter polling; it does not establish fence signaling.
The likely recording fields suffice for a fence record, while synchronization
and enum meanings stay open. Neither recording nor submission signals it.

### Saved depth processing data

Save x1 has five distinct wide unreadable values, Restore has three. Neither
has a CPU pointee. Save x2 is 0x100, 0x40a0 or 0x9320; Restore x2 is
0x40a0 or 0x9320 in all reports. TextureGetZCullStorageSize's results
match these sizes and signatures 0003 records later use in SaveZCullData.
This supports a graphics location and extent contract, already rated likely
in signatures 0002. It cannot identify a texture, pool, memory contents,
byte units, save format, or matching save/restore address pair.

Save changes command-buffer +0x00 in 2048/2048. Its x3 is a constant readable
value with zero leading words, wide +0x08 and later addresses. x4 is mixed,
x5 is mixed, x6 is 0 or 0x400 and x7 has two readable addresses, whose +0x08
contains sizes and 2. Save d0 is zero on entry but wide on return, and other
floating registers mix zero, one, sizes and wide values. Restore changes
command-buffer +0x10 in 187/2048 and +0x20 in 2048/2048; its x3..x6 are
2, 0x20, 0x20 and 0x400. Its x7 is a constant readable value with wide
+0x00, sizes at +0x08, zeros through +0x1c, later wide words and 2 at
+0x38. None of these trailing pointees changes. Linked blocks retain every
word. They do not establish an extra texture argument. Restore's pointee
reports differ only in totals; Save also differs in x3 +0x20 address count
(1894 versus 1891) and x4 +0x38 wide count (143 versus 150).

No save-before-restore order is retained. No connection to a particular
resolved render-target, barrier or clear call is established. The two records
preserve their distinct names, locations and sizes in recording order and
perform no storage access or depth-state mutation.

### Polygon offset clamp

All reports show d0 = 0, d1 = 0 and d2 = 0x7f7fffff. Interpreted as a
single-precision bit pattern, the last value is finite and maximal, which
fits a clamp hypothesis but is not a varying float argument. d3 through d5
are wide/mixed; d6/d7 contain viewport-like positive/negative half sizes.
x1 is one of two readable values whose leading +0x00 is 0x14 or 0x16,
+0x04 and +0x08 zero, matching BindPolygonState's input shape. x2 is
0xff, matching resolved stencil masks. These support the leftover reading
in signatures 0010; they do not establish bind-before-offset or mask-before-offset
order. x3..x7 match the vertex-binding tails, including pointees. The only
pointee difference between 0002 and 0003 is the x7 +0x10 address count.
x0 +0x00 changes 2048/2048. Result d0 is zero; x0/x1 are address-shaped.

Signatures 0001's SetDepthRange supports single-precision values elsewhere,
not the float count, width, order or roles here. There is no varying sample
or pointer layout that raises this contract above open. The recorder retains
all three floating-register words without assigning roles. The existing
opt-in drawing contract permits a host-supplied factor, units and clamp
hypothesis as documented in
[depth and raster provenance](../provenance/0028-depth-raster.md). This is
experiment behavior, not a resolved signature.

## Resolver names and callable addresses

The **exact aggregate requested-name set** is all 534 `yes` rows of census
0001, including the 366 zero-call rows. For direct review it is copied, in
census order, to [0011-resolver-requests.txt](0011-resolver-requests.txt).
This is the exact combined bootstrap/secondary set, not a reconstructed
list for DeviceGetProcAddress alone. There are no unknown requested names.
The resolver's own name is one of the requested symbols.

The census has one requested boolean per name and a call count for each
function. It does not retain request counts or identify the requesting
resolver. Provenance 0005 explicitly says that bootstrap versus secondary
requests were not recorded separately. Thus **exactly which names were
supplied to the 1,070 DeviceGetProcAddress calls is unknown**. In particular,
1,070 does not prove two passes over the 534 names or explain the two extra
calls. The two null-device calls cannot be tied to names either.

The signature is firm: x0 is the device in 1,068 samples and null in two;
x1 is readable name text; result x0 is a callable address according to
provenance 0005 and signatures 0003. Its x0 pointee has two leading zeros,
wide words at +0x08/+0x10, 0x902d at +0x18, zero at +0x1c,
0x100 at +0x20, zero at +0x24 and wide words from +0x28 onward.
All sixteen x1 words are other and unchanged, intentionally suppressing text.
x2 is mixed and readable twice, with all-other pointees; x3 is 0 or 1,
x4..x7 zero, and floating registers vary. These do not establish additional
arguments. Reports 0002 and 0003 agree exactly.

[Provenance 0002](../provenance/0002-functions-are-requested-by-name.md) and
[0003](../provenance/0003-function-names.md) originally inferred lookup from
an import and strings; provenance 0005 confirms it with requests followed by
calls through returned addresses. [Provenance 0004](../provenance/0004-call-interface.md)
explains why the library receives registers rather than exposing one native
function per observed signature. [Provenance 0010](../provenance/0010-first-run-on-novena.md)
keeps real callable addresses on the host side. A function-table id is not a
program-callable address; neither is a Rust handler pointer using the host
library's calling convention. The existing host contract supplies memory
callbacks but no callable-address factory or name/address registry.

The names suffice for the existing name-to-id lookup, not for this resolver's
address result. No successful DeviceGetProcAddress handler is added. To
implement it, a host must supply program-callable thunks and their name/address
mapping. Additional observation would identify secondary request names and
unknown-name/null-device behavior. No SDK signature or remembered layout is
used to fill these gaps.

## Related object and resolved-call sources

These complete blocks include setter input shapes, changed state words,
registration ids, queried results and the resolved calls used for comparison.
Earlier interpretations are in signatures 0001 through 0004 and 0009.
The links preserve all three runs rather than selecting a matching sample.

| Related function | Shapes 0001 | Shapes 0002 | Shapes 0003 |
|---|---|---|---|
| VertexAttribStateSetDefaults | [block](../shapes/0001-program-a-startup.txt#L3062-L3081) | [block](../shapes/0002-program-a-startup-pointees.txt#L11384-L11460) | [block](../shapes/0003-program-a-startup-answers.txt#L11386-L11461) |
| VertexAttribStateSetFormat | [block](../shapes/0001-program-a-startup.txt#L3082-L3101) | [block](../shapes/0002-program-a-startup-pointees.txt#L11461-L11525) | [block](../shapes/0003-program-a-startup-answers.txt#L11462-L11526) |
| VertexAttribStateSetStreamIndex | [block](../shapes/0001-program-a-startup.txt#L3102-L3121) | [block](../shapes/0002-program-a-startup-pointees.txt#L11526-L11590) | [block](../shapes/0003-program-a-startup-answers.txt#L11527-L11591) |
| VertexStreamStateSetDefaults | [block](../shapes/0001-program-a-startup.txt#L3122-L3141) | [block](../shapes/0002-program-a-startup-pointees.txt#L11591-L11667) | [block](../shapes/0003-program-a-startup-answers.txt#L11592-L11667) |
| VertexStreamStateSetStride | [block](../shapes/0001-program-a-startup.txt#L3162-L3181) | [block](../shapes/0002-program-a-startup-pointees.txt#L11734-L11798) | [block](../shapes/0003-program-a-startup-answers.txt#L11734-L11798) |
| VertexStreamStateSetDivisor | [block](../shapes/0001-program-a-startup.txt#L3142-L3161) | [block](../shapes/0002-program-a-startup-pointees.txt#L11668-L11733) | [block](../shapes/0003-program-a-startup-answers.txt#L11668-L11733) |
| SamplerPoolRegisterSampler | [block](../shapes/0001-program-a-startup.txt#L2382-L2401) | [block](../shapes/0002-program-a-startup-pointees.txt#L8731-L8834) | [block](../shapes/0003-program-a-startup-answers.txt#L8745-L8848) |
| SamplerInitialize | [block](../shapes/0001-program-a-startup.txt#L2342-L2361) | [block](../shapes/0002-program-a-startup-pointees.txt#L8583-L8683) | [block](../shapes/0003-program-a-startup-answers.txt#L8597-L8697) |
| DeviceGetSeparateSamplerHandle | [block](../shapes/0001-program-a-startup.txt#L1422-L1441) | [block](../shapes/0002-program-a-startup-pointees.txt#L5034-L5126) | [block](../shapes/0003-program-a-startup-answers.txt#L5034-L5126) |
| DeviceGetSeparateTextureHandle | [block](../shapes/0001-program-a-startup.txt#L1442-L1461) | [block](../shapes/0002-program-a-startup-pointees.txt#L5127-L5178) | [block](../shapes/0003-program-a-startup-answers.txt#L5127-L5178) |
| CommandBufferBindSeparateTexture | [block](../shapes/0001-program-a-startup.txt#L422-L441) | [block](../shapes/0002-program-a-startup-pointees.txt#L1692-L1740) | [block](../shapes/0003-program-a-startup-answers.txt#L1693-L1741) |
| CommandBufferBindPolygonState | [block](../shapes/0001-program-a-startup.txt#L362-L381) | [block](../shapes/0002-program-a-startup-pointees.txt#L1429-L1527) | [block](../shapes/0003-program-a-startup-answers.txt#L1430-L1528) |
| CommandBufferSetStencilMask | [block](../shapes/0001-program-a-startup.txt#L1042-L1061) | [block](../shapes/0002-program-a-startup-pointees.txt#L3775-L3859) | [block](../shapes/0003-program-a-startup-answers.txt#L3774-L3858) |
| CommandBufferSetStencilValueMask | [block](../shapes/0001-program-a-startup.txt#L1082-L1101) | [block](../shapes/0002-program-a-startup-pointees.txt#L3945-L4029) | [block](../shapes/0003-program-a-startup-answers.txt#L3944-L4028) |
| CommandBufferClearColor | [block](../shapes/0001-program-a-startup.txt#L542-L561) | [block](../shapes/0002-program-a-startup-pointees.txt#L2135-L2174) | [block](../shapes/0003-program-a-startup-answers.txt#L2135-L2174) |
| CommandBufferSetDepthRange | [block](../shapes/0001-program-a-startup.txt#L882-L901) | [block](../shapes/0002-program-a-startup-pointees.txt#L3224-L3303) | [block](../shapes/0003-program-a-startup-answers.txt#L3223-L3302) |
| TextureGetZCullStorageSize | [block](../shapes/0001-program-a-startup.txt#L2902-L2921) | [block](../shapes/0002-program-a-startup-pointees.txt#L10836-L10882) | [block](../shapes/0003-program-a-startup-answers.txt#L10838-L10884) |
| SyncInitialize | [block](../shapes/0001-program-a-startup.txt#L2422-L2441) | [block](../shapes/0002-program-a-startup-pointees.txt#L8939-L9039) | [block](../shapes/0003-program-a-startup-answers.txt#L8952-L9054) |
| SyncFinalize | [block](../shapes/0001-program-a-startup.txt#L2402-L2421) | [block](../shapes/0002-program-a-startup-pointees.txt#L8835-L8938) | [block](../shapes/0003-program-a-startup-answers.txt#L8849-L8951) |
| SyncWait | [block](../shapes/0001-program-a-startup.txt#L2442-L2461) | [block](../shapes/0002-program-a-startup-pointees.txt#L9040-L9128) | [block](../shapes/0003-program-a-startup-answers.txt#L9055-L9142) |
| QueueWaitSync | [block](../shapes/0001-program-a-startup.txt#L2142-L2161) | [block](../shapes/0002-program-a-startup-pointees.txt#L7736-L7832) | [block](../shapes/0003-program-a-startup-answers.txt#L7750-L7846) |
| WindowAcquireTexture | [block](../shapes/0001-program-a-startup.txt#L3182-L3201) | [block](../shapes/0002-program-a-startup-pointees.txt#L11799-L11903) | [block](../shapes/0003-program-a-startup-answers.txt#L11799-L11903) |

2026-10-09: [Provenance 0037](../provenance/0037-command-execution.md) adds
deferred opaque texture-clear recording and an opt-in host decoder. The six
shape observations still do not establish the clear record layout.
