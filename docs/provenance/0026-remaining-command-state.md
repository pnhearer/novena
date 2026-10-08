# 0026: remaining command state

- Date: 2026-10-08
- Author: Khargoosh
- Covers: signatures 0010, `api/state_commands.rs`, `StateCommand`, recording dispatch and queue handling

## What was learned

Comparison of census 0001 with the API handlers found 25 command functions
that only used the raw fallback and still returned `Unimplemented`, plus one
resolver with no handler. The comparison counts call status, not just the
presence of a handler. All 26 entries, including their census counts, are in
[signatures 0010](../signatures/0010-remaining-command-state.md).

Seventeen command entries have firm fields sufficient for retaining state:
six single-object state bindings, three stencil settings, a barrier field,
a tiled-cache action field, two buffer bindings, two descriptor references,
a buffer clear record and three compute dispatch integers. Firm means the
recording boundary only. Full execution contracts are not established.

The other eight command entries retain uncertain pointer layouts, float
arguments, synchronization or saved-state semantics. The resolver needs a
host-provided callable address, not an invented integer result.

## How

Review of the existing observations only: census 0001, shapes 0001 through
0003, signatures 0001 through 0009, and provenance notes 0004 through 0012.
No new program run, external source, development kit material or remembered
signature was used. The shape files were compared by register class, first
pointee words, before/after change counts and result classes. Existing notes
supply the limited order facts. The aggregate files cannot establish a new
per-call sequence or identity correlation.

The command buffer pointee changes during recording, while the single-state
input pointees have no recorded changes. Scalar positions separate the new
settings from the repeated trailing register shapes. Wide resource values
are retained without resolving or reading them. The separate sampler value
is readable and remains unresolved. ClearTexture's all-zero leading words
do not establish a color record. PolygonOffsetClamp's d2 is 0x7f7fffff,
correcting the earlier statement that its float registers were all zero.

The implementation representation is novena's own design. It appends explicit
state records in the existing recording list and snapshots only previously
known setter settings for the six object bindings. It preserves all scalar
bits because integer widths are open. The queue deliberately skips these
records. Return-register clearing follows the existing command recorder and
is not asserted as observed behavior of the original implementation.

Unit tests use invented objects and values. They cover every new handler,
full-width scalar retention, record order, snapshots before and after a setter
change, unknown and mismatched objects, recording lifetimes, independent
command buffers, and the remaining census gap list. A host-backed submission
test checks that recorded state does not write memory or run rendering.

## Confidence and open questions

- Firm: observed register kinds, command buffer changes and input pointee classifications, with the sample limits listed in signatures 0010.
- Likely: argument role labels, pool-backed addresses, descriptor handles and reference to the state family named by the function.
- Open: integer and float widths, signedness, enumeration meanings, packed layouts, array strides, return contracts, resource identity correlations and execution semantics.
- Barrier x2 pointee changes can reflect aliasing of recording memory. They do not establish a standalone output argument or justify a query-style write.
- A snapshot of novena's known state is an implementation choice. No claim is made about how the original implementation copies or references state objects.

Validation uses CPU tests, formatting and clippy with warnings denied. GPU-only
tests are skipped because this machine has no GPU. No new run of an observed
program or visual result is claimed.
