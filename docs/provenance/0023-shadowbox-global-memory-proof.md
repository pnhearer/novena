# 0023: Shadowbox global memory GPU proof

- Date: 2026-10-07
- Author: Khargoosh
- Covers: `tests/shadowbox/global_memory.rs`, the Shadowbox dev dependency, and
  the flat memory contract shared by Shadowbox and Novena

## What was learned

The integration test feeds project-authored Maxwell words to `shadowbox::translate`
with default translation options. It creates the resulting compute pipeline through
Novena's `Context::create_global_shader_module`, allocates `ARENA_SIZE` bytes through
`GlobalMemory::new`, and uses `GlobalMemory::push_delta` before every dispatch.
The arena is 1 GiB. The two 128-byte pools have guest bases `0x10000` and `0x10080`.
Input pointers contain those guest addresses, without a host-address conversion.

Each of 156 cases checks all 256 pool bytes after a shader-write to host-read
barrier and queue completion. The cases cover:

- LDG and STG U8, S8, U16, S16, B32, B64, and B128, with A32 and A64
  addressing and instruction offsets -16, 0, and 16. Narrow loads also write
  their complete register to check sign or zero extension. Sentinel bytes around
  narrow stores must remain unchanged, and vector components have distinct values.
- A B64 load that replaces its address register pair with a pointer stored in
  memory. A B128 load follows that pointer into the other pool and stores the
  result back into the first pool.
- U32 and S32 ATOM ADD, MIN, MAX, AND, OR, XOR, and EXCH, with exact old-value
  readback. RED covers the same operations except EXCH. Both address widths and
  positive and negative offsets execute. MIN and MAX inputs distinguish signed
  from unsigned comparison, and ADD checks wrapping overflow.
- Packed U32 CAS success and failure, including the returned old value.
- RED ADD from 64 workgroups contending on one word, with the exact final sum.

The normal test checks all 156 translated modules for one push-constant block
with exactly one uint64 member at Shadowbox's published byte offset. It also
checks a compute local size of one and compares Shadowbox's offset and size
constants with Novena's pipeline range. The GPU test validates each module with
`spirv-val --target-env vulkan1.2` before creating its Vulkan pipeline.

## How

These are original synthetic programs and data. No game shader, capture, SDK,
or proprietary implementation was used. Instruction fields come from the
MIT-licensed envytools table `envydis/gm107.c`, at revision
`f102b82381f3f11cee113d16374c87091db039d9`. These are public encoding facts,
not copied disassembler code.

| Encoding fact | envytools gm107.c rows |
| --- | --- |
| Data, address, and atomic source registers at bits 0, 8, and 20 | [203-218](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L203-L218) |
| MOV32I immediate at bits 20..51 and opcode | [249](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L249), [2170](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L2170) |
| Unconditional predicate bits | [408-412](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L408-L412) |
| Signed LDG/STG byte offset and memory address operand | [316,331](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L316-L331) |
| STG and LDG size codes | [583-590](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L583-L590), [642-650](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L642-L650) |
| LDG/STG families and E at bit 45 | [1898-1899](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L1898-L1899) |
| Signed atomic offset and address operand | [321,339-341](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L321-L341) |
| ATOM operations and U32/S32 type codes | [673-694](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L673-L694) |
| RED operations and type codes | [718-737](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L718-L737) |
| CAS, ATOM, and RED families, E at bit 48 | [1897-1905](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L1897-L1905) |
| EXIT and NOP | [1925](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L1925), [2057](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L2057) |
| Control word and bundle position | [2174-2182](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L2174-L2182), [2215-2216](https://github.com/envytools/envytools/blob/f102b82381f3f11cee113d16374c87091db039d9/envydis/gm107.c#L2215-L2216) |

Packed CAS uses adjacent compare and replacement registers, as documented in
Shadowbox's `docs/design/global-memory-flat.md`, citing Mesa
`src/nouveau/compiler/nak/sm50.rs:2741-2760`. The compute header is an original
zeroed record selecting Shadowbox's compute stage. The test uses its public
translation API rather than reproducing its emitter or decoder.

The dependency points to the requested sibling checkout at
the directory named by `NOVENA_SHADOWBOX_PATH`. Verification uses its
`gmem-flat` revision `57bb5cf7b87cc5e73f0a87c6c9f441bf9f0eae14`.
2026-10-07: [Merge note 0024](0024-gmem-flat-merge.md) supersedes the original
workspace dependency arrangement. Shadowbox is now a dev-dependency only in
an isolated test package. Novena builds without that checkout, and the wrapper
reports `SKIP` when it is absent. `NOVENA_SHADOWBOX_PATH` can select another
crate directory. The synthetic programs and GPU assertions are unchanged.

## Repeat the proof

From the Novena worktree, with the Shadowbox checkout above available:

```sh
mkdir -p target/tmp
export TMPDIR="$PWD/target/tmp"
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features vulkan -- -D warnings
cargo test
cargo test --features vulkan -- --include-ignored
cargo test --features vulkan --test shadowbox_global_memory -- --ignored --nocapture
cargo test --features vulkan --test global_memory -- --ignored --nocapture
```

The GPU test is explicitly ignored in routine tests because it requires a Vulkan
GPU, narrow integer and storage features, 1 GiB of coherent device-address memory,
and `spirv-val`. With the Shadowbox checkout present, running it explicitly
fails if any GPU or shader-tool requirement is unavailable.
It never treats an unavailable GPU as a successful case.

## Confidence and open questions

The test asserts an 8-byte uint64 delta at offset 0, `delta = host_base - guest_base`
with wrapping subtraction, and delta divisibility by 16. It uses the production
helper with a pipeline layout containing only that range. No extra root push
constant or descriptor supplies the test addresses.

This proves the translator and allocator contract through direct Vulkan compute
submission. Novena's production draw and dispatch executor remains unfinished.
It does not establish undefined misaligned access behavior, unsupported atomic
forms, or Maxwell cache coherence for racing ordinary loads and stores.

## Verification result

On 2026-10-07, AMD Radeon RX 9070 XT with RADV 26.2.4 executed all 156
Shadowbox cases with exact pool-byte matches. Every translated module passed
SPIR-V validation. The normal translation contract test also passed.

`cargo fmt --all -- --check` and strict all-target clippy passed with default
features and with `vulkan`. Default tests passed 35 tests.
`cargo test --features vulkan -- --include-ignored` passed all 43 tests, including
the existing public API pool-address test and both GLSL global-memory tests.
There were no ignored tests in that explicit Vulkan run.

The contracts agree on offset 0, size 8, alignment, and `host_base - guest_base`.
No implementation correction was needed in either worktree. Shadowbox was
read-only throughout this integration work.

The project-profile packet and terminal receipt commands reported that this
worktree is not registered. No profile was changed or candidate promoted.
