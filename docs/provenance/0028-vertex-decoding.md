# 0028: topology and vertex decoding

- Date: 2026-10-08
- Author: Khargoosh
- Covers: drawing steps 2 and 3, host topology and format tables, counted state
  snapshots, multiple streams, input reflection, graphics cache keys, and
  headless pixel proofs

## Stored observations

This work rechecks existing reports. It does not add a real-program capture.
The permitted guest evidence is signatures 0002, 0003, 0004, and 0009, the
census, and the three stored shape reports:

- [Shapes 0001](../shapes/0001-program-a-startup.txt)
- [Shapes 0002](../shapes/0002-program-a-startup-pointees.txt)
- [Shapes 0003](../shapes/0003-program-a-startup-answers.txt)

All three reports agree on the fields below. Function names omit the common
prefix. The value lists that end in `...` are truncated observations.

| Field | Stored observation | What it establishes |
| --- | --- | --- |
| `DrawArrays` primitive, first, count | 5, 0, 4 | One observed token and argument shape |
| `DrawArraysInstanced` primitive, first, count | 5, 0, 4 | The same primitive token in another draw form |
| `DrawElementsBaseVertex` primitive and count | 4, counts 3 and 6 | A second observed token, used in indexed draws |
| `VertexAttribStateSetFormat` format | `0x2e 0x16 0x22 0xa 0x25 0x14 ...`, range `0xa` through `0x2e` | Multiple tokens, without widths or numeric interpretation |
| `VertexAttribStateSetFormat` offset | `0 0x10 0x20 0x30 0x40 8 ...` | A separate offset argument |
| `VertexAttribStateSetStreamIndex` index | 0, 1, 2, 3 | Four observed stream selections |
| `VertexStreamStateSetStride` stride | `0x18 0x20 8 0x10 0xc 0x50 ...`, range 0 through `0x50` | A stride argument, including zero |
| `VertexStreamStateSetDivisor` divisor | 0, 1 | Two values, without instance semantics |
| `BindVertexAttribState` count | 1, 2, 5 | Counted attributes |
| `BindVertexStreamState` count | 1 | No observed spacing between stream objects |

The census establishes that these calls occur. It does not identify their
effects. Signatures 0004 connects the first stream-state word with a stride
value. It does not establish an object size. The format and stream-index
setters change the first attribute-state word in shapes 0003, but these
classifications do not decode its bits.

The primitive 0 entries in signatures 0009 were transcription errors. They
are corrected to 5 on this date. No stored report supports primitive 0.
The comparison is repeatable with:

```sh
python3 scripts/check-drawing.py --observations
```

## Topology mappings

The Vulkan destinations come from
[VkPrimitiveTopology](https://docs.vulkan.org/refpages/latest/refpages/source/VkPrimitiveTopology.html).
Lists assemble independent groups of three vertices, strips share consecutive
edges, and fans share the first vertex. These are public Vulkan facts.

`FirstDrawContract::topologies` explicitly selects each raw token. There is
no built-in guest enum table. The proof selects these mappings:

| Raw token | Host choice | Vulkan destination | Evidence and confidence |
| --- | --- | --- | --- |
| 4 | `TriangleList` | `VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST` | Guest hypothesis T1. Counts 3 and 6 are consistent with lists but do not prove them. The synthetic host proof passes. |
| 5 | `TriangleStrip` | `VK_PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP` | Guest hypothesis T2. Count 4 permits a strip or fan and does not distinguish them. The synthetic host proof passes. |
| `0xf006` | `TriangleFan` | `VK_PRIMITIVE_TOPOLOGY_TRIANGLE_FAN` | Original synthetic token. No proposed guest value. The synthetic host proof passes. |

T1 and T2 require a real-program confirmation. Keep the program, state, first
vertex, and controlled vertex bytes fixed. Change only the primitive token.
Use six asymmetric vertices whose list, strip, and fan coverage differs.
Compare readback against all three independent assembly predictions.
Confirm 4 only if independent triples win. Confirm 5 only if consecutive
shared-edge triangles win. A fan result would reject T2.
Indexed use of 4 also needs an independently established index interpretation.
This implementation repeats 4 through the non-indexed path to isolate topology.

The fan token exercises a Vulkan destination, not a guest claim. Establish a
guest fan token by finding a real-program call, repeating the asymmetric test,
and observing triangles that share the first vertex. No restart behavior or
winding convention is inferred. Culling and restart remain disabled here.

## Format mappings

The formats and byte layouts come from
[VkFormat](https://docs.vulkan.org/refpages/latest/refpages/source/VkFormat.html).
The fetch and conversion rules come from
[fixed-function vertex processing](https://docs.vulkan.org/spec/latest/chapters/fxvertex.html).
The host format names below have these Vulkan meanings. Each raw token in
the table is synthetic and has no proposed guest counterpart.

| Synthetic token | Host format | Vulkan destination | Bytes | Component alignment |
| --- | --- | --- | --- | --- |
| `0xf100` | `Float` | `VK_FORMAT_R32_SFLOAT` | 4 | 4 |
| `0xf101` | `Float2` | `VK_FORMAT_R32G32_SFLOAT` | 8 | 4 |
| `0xf102` | `Float3` | `VK_FORMAT_R32G32B32_SFLOAT` | 12 | 4 |
| `0xf103` | `Float4` | `VK_FORMAT_R32G32B32A32_SFLOAT` | 16 | 4 |
| `0xf104` | `Half2` | `VK_FORMAT_R16G16_SFLOAT` | 4 | 2 |
| `0xf105` | `Half4` | `VK_FORMAT_R16G16B16A16_SFLOAT` | 8 | 2 |
| `0xf106` | `Unorm8x4` | `VK_FORMAT_R8G8B8A8_UNORM` | 4 | 1 |
| `0xf107` | `Snorm8x4` | `VK_FORMAT_R8G8B8A8_SNORM` | 4 | 1 |
| `0xf108` | `Unorm16x2` | `VK_FORMAT_R16G16_UNORM` | 4 | 2 |
| `0xf109` | `Unorm16x4` | `VK_FORMAT_R16G16B16A16_UNORM` | 8 | 2 |
| `0xf10a` | `Snorm16x2` | `VK_FORMAT_R16G16_SNORM` | 4 | 2 |
| `0xf10b` | `Snorm16x4` | `VK_FORMAT_R16G16B16A16_SNORM` | 8 | 2 |

Every row has a pixel case in `vertex_formats_read_back_pixels` and a
`VertexFormat` match arm in `gpu::graphics`. The earlier synthetic token
`0xf002` still selects `Float4` in the original and translated-input proofs.
These mappings establish host behavior only. The format proof explicitly
rejects the observed value `0x2e` when the host has not mapped it.

The guest hypothesis family F1 is that an observed format token selects a
component count, numeric representation, and fetch width. The reports do not
pair any of their tokens with a specific row. Treat each proposed pairing
as unconfirmed. For each token, use a second attribute with controlled bytes
and a shader whose color depends on every component. Change the token alone.
Compare against these host cases, then shorten the bound range by one byte
at the predicted last fetch. Confirm a pairing only when both decoded color
and fetch width agree.

For float rows, test distinct components, negative values, and missing-component
defaults. For half rows, use distinguishable 16-bit float encodings. For
normalized rows, test both endpoints and interior values. Signed rows also
need negative values and the most negative encoded integer. These probes
separate the supported hypotheses. A result outside them needs a new format
and new provenance, not a guessed table entry.

## State and interface assumptions

The host contract also makes these choices. None is a new guest observation.

| Choice | Basis | Real-program confirmation |
| --- | --- | --- |
| Counted objects at `base + index * host_stride` | Signatures 0004 suggests direct stream objects at count one. The test supplies spacing 32 for attributes and 16 for streams. | Observe setter object addresses and the binding base with count greater than one. Confirm the element identities and spacing without treating later words as another object. |
| Attribute array index equals shader location | Explicit experiment assumption, checked against reflected locations | Give two attributes different controlled effects, exchange their array positions, and identify which shader input changes. |
| Stream state array index equals buffer stream index | Explicit experiment assumption using the recorded stream selector | Change one buffer binding, then one state element. Confirm that only the selected attribute changes. |
| Stride and offset are bytes | Public Vulkan fetch rule and explicit host choice | Keep bytes fixed, vary each argument alone, and find the first byte selected in the resulting color or position. |
| Zero stride repeats one element | Public Vulkan fetch rule and explicit host choice | Draw distinct vertices with varying second-attribute data. Set only the second stream's stride to zero and inspect whether its value becomes constant. |
| Divisor zero uses per-vertex input | Explicit host choice. Observed 0 and 1 do not establish semantics. | Vary the divisor alone across multiple instances with distinguishable attribute data. Nonzero divisors remain unsupported here. |

Counts above one require an explicit nonzero host object spacing. Missing
objects, unknown tokens, duplicate token entries, and unsupported state return
`Unimplemented`. The count cap is 16, an implementation bound. Count zero
produces an empty binding and cannot satisfy this experiment's shader inputs.
Later setters cannot change a captured binding.

The executor preserves buffers by stream index. It checks each active
attribute's last byte using first vertex, count, stride, offset, and format
width. Every fetched address and nonzero stride must meet the component
alignment. Short or misaligned ranges return `BadArgument`.

Original source shaders follow the public
[Vulkan vertex-input tutorial](https://docs.vulkan.org/tutorial/latest/04_Vertex_buffers/00_Vertex_input_description.html).
Reflection accepts consecutive float scalar or vector inputs starting at
location zero, matching float varying locations and widths, and one float4
fragment output at location zero. Builtin vertex output blocks are skipped.
Integer inputs, interface arrays, component decorations, fragment builtin
outputs, descriptors, and mismatched varyings remain unsupported.
Format availability and vertex-input device limits are checked before creation.

The pipeline key includes topology, each active stream index and stride, and
each attribute's stream, format, and offset, plus complete shader words.
Dynamic rectangles, buffer contents, addresses, and host object spacing stay
outside the key. Those choices do not change the compiled pipeline.
The bounded background workers and private disk cache from provenance 0027
remain active. A cold pipeline skips that draw while the worker compiles it.
Explicit retry uses the complete vertex input and topology key. Attachment
handling and completion follow provenance 0027.

## Headless experiments

The synthetic instruction encodings, shader headers, and translator boundary
use the public Mesa and envytools facts already recorded in
[provenance 0027](0027-drawing.md). The topology proof reuses those original
programs. The second-input variant loads four components at generic address
`0x90` into registers four through seven and writes register four to position W
at `0x7c`. Its header declares both inputs. No real-program shader or vertex
content enters any test. The selected translator remains read-only.

`primitive_topologies_read_back_pixels` draws the same asymmetric arrangement
with each token, using both four and six vertices, a nonzero first vertex,
offset, and padded stride. A separate CPU geometric oracle checks covered and
uncovered pixels away from edge ties. List, strip, and fan images must differ.
Twelve draw recordings create three pipelines. Three cold submissions queue
compilation. Fresh recordings follow worker completion. The ready
submissions produce twelve cache hits. A translated
second attribute in stream one sets position W to two, halving displacement
from the viewport center. Two further draws pass readback with one miss and
two ready cache hits after a cold submission. A one-byte-short second stream is rejected.

`vertex_formats_read_back_pixels` compiles original source shaders with
`glslangValidator` and validates them with `spirv-val`. A fixture adapter
supplies those words through `ShaderTranslator`; this matrix tests fetching,
format conversion, and varying linkage, not instruction translation.
The separate second-input proof above exercises actual instruction translation.

Each format row runs with interleaved data, a separate stream, and a
zero-stride separate stream. Positions and attribute values are original.
Padding contains poison bytes. First vertex is one. Attribute offsets include
one-byte and two-byte alignment. Streams cover indices zero through three.
The fragment exposes every fetched component through an affine color transform,
including negative signed values and missing-component defaults. A one-unit
color tolerance permits normalized attachment rounding.

The matrix has 36 configurations, each submitted twice for 72 successful
readbacks and 72 cache hits. Each configuration first queues compilation and
submits fresh recordings after the worker finishes. Each configuration also rejects a range shortened
by one byte. Both attributes retain captured state after later setters.
Additional checks reject a short position stream, misaligned pool addresses,
a missing buffer, nonzero divisor, duplicate format tokens, and unspecified
counted layouts. Missing buffers also check that recordings do not inherit
previous stream bindings. Every successful case compares canonical target
bytes with presentation readback.

## Verification

Run the narrow proof with a selected translator crate:

```sh
python3 scripts/check-drawing.py
```

`NOVENA_SHADOWBOX_PATH` must be set. The script runs the available drawing proofs,
checks formatting, and lints their isolated test package. It rejects skips,
validation errors, and nonzero exit codes. Full output stays in the ignored
`target/drawing.log`. The stored-observation mode needs no GPU or translator.

The following checks completed with exit code zero on 2026-10-08:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/drawing.rs
python3 scripts/check-drawing.py
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
```

The translator was configured. All three drawing proofs ran on the headless
Vulkan path and passed. Default tests passed 41 workspace tests. The
translated triangle also passed persistent-cache restart and corruption
recovery. The topology and format matrices passed canonical and presentation
readback, cache-key checks, and fetch bounds. No drawing proof skipped.
Cargo output, cache files, source-shader compilation output, and driver cache
files remained under the checkout's ignored `target` directory.
