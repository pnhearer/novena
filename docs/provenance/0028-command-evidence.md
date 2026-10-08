# 0028: evidence for the last command gaps

- Date: 2026-10-08
- Author: Khargoosh
- Covers: signatures 0011, the resolver request manifest, six command contracts, retained draw state and their unit tests

## What was learned

The nine gaps left by [provenance 0026](0026-remaining-command-state.md) are
reviewed individually in [signatures 0011](../signatures/0011-command-evidence.md).
That note links every function's complete block in all three shape reports,
records pointee targets, return shapes, related object families, and the
available order evidence relative to calls already handled.

Six contracts are likely enough for state recording: vertex attribute and
stream count/base bindings, separate sampler reference, fence reference and
two integers, and saved depth processing source/destination plus extent.
They now return Ok while retaining explicit records. The implementation has
24 command-state handlers. ClearTexture and DeviceGetProcAddress remain
unsuccessful among the 168 census-called functions. Polygon offset retains
three full floating-register words for the existing opt-in draw experiment;
its argument count, width and order remain hypotheses.

No new observation resolves array stride, synchronization, saved-state format
or sampler-reference representation. SeparateSampler's readability is not
an obstacle to retaining its bits: the related handle query itself returned
readable values in 20/48 samples in shapes 0003. This supports a likely
opaque reference contract, without proving identity or pointer meaning.

The exact aggregate request set is the 534 requested census rows, copied to
[the manifest](../signatures/0011-resolver-requests.txt) and checked against
all rows, including zero-call names. Provenance 0005 explicitly says that
bootstrap and secondary requests were not distinguished. The exact names
passed to DeviceGetProcAddress alone cannot be recovered. Neither the census
nor shape reports retain a name-to-callable-address mapping. No resolver
handler returns an invented pointer or function-table id.

## How

Review of repository observations only: census 0001; shapes 0001 through
0003; signatures 0001 through 0004, 0006, 0009 draw state and 0010;
provenance 0002 through 0008, 0010, 0011 and 0015. The complete function
blocks and related setters, registration, handle queries, texture storage
queries, sync operations and resolved command families were compared. No
external reference, development kit material, remembered signature or
proprietary implementation was used. Tests and record representations are
original implementation choices using invented objects and values.

The reports aggregate calls. Prior notes supply limited order facts, but
there is no retained call sequence from which to establish draw adjacency,
save/restore pairing or sync lifetime correlation. The follow-up also
corrects the older "counted pointer array" phrasing for vertex state in
signatures 0009: count plus a readable base does not prove an array of object
addresses.

## Implementation choices

The handlers append records to the existing recording list. Counts and sizes
remain full-width scalars. Recording does not traverse program memory.
The vertex records retain a matching base object's known setter settings in
an owned first_settings snapshot. Further elements are unknown without an explicit host experiment contract. Zero count,
unknown base and mismatched family carry no settings. A caller may mutate
its state later without rewriting earlier snapshots. This is a partial
snapshot of novena's own state, not a recovered input-array layout. The
existing draw experiment can also snapshot up to 16 known state objects
using host-configured spacing. Those snapshots have a separate
experiment_settings field and use checked address arithmetic. They preserve
the earlier topology, vertex-format and indexed-draw paths without treating
host spacing as observed object layout. Drawing can use the known base
settings at count one; an unresolved multi-element binding stays unsupported.

Sampler references have a separate opaque record, without asserting a handle.
Fence records do not mark a sync signalled or wait on it. Save and restore
have distinct records and do not read/write storage or associate a texture.
CPU submission skips these records. Vulkan drawing consumes vertex state
snapshots under the explicit experiment contract. Sampler, fence and saved
depth records remain inert on both paths. Return-register clearing follows the existing recorder convention,
not an observed successful-return contract.

Texture clear remains raw because its input-record roles and unusual return
classification are open. Polygon offset preserves the existing raw three-register recording and
opt-in execution hypothesis documented in
[depth and raster provenance](0028-depth-raster.md). Three constant
floating-register values cannot settle argument count, width and order.
The resolver's signature is firm but requires host-supplied program-callable
addresses, absent from the current host contract. The existing name-to-id
lookup is not a substitute for a callable pointer.

## Original recording validation

Unit tests cover all six added handlers with invented values wider than the
samples, command order and untouched trailing registers. Counted bindings
exercise actual setters, snapshot mutation, zero count, unknown objects,
and mismatched families. Each new handler is checked directly with a memory
callback counter to ensure it does not follow references; this excludes the
independent shape observer that samples readable registers before dispatch.
Each also crosses Begin/End boundaries in two independent command buffers.
Submission tests consume the records and verify no program-memory writes,
texture changes, pool changes or sync-object changes. The census coverage
test requires exactly the open texture clear and resolver to remain gaps.

The request manifest test compares all 534 names and their order directly
with the census and checks that each has a known function id. Documentation
checks retain signature coverage and report privacy checks.

Checks completed:

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace`: passed, 41 unit tests and four documentation tests; exit code 0.
- `cargo test --workspace --features vulkan -- --skip gpu::tests::clear_and_readback --skip synthetic_translations_match_novena_push_constants`: passed, 46 unit tests and four documentation tests; exit code 0. The Vulkan loader was pointed at an absent ICD file so CPU fallback was tested. Two readback tests were explicitly excluded and GPU-only ignored tests were left ignored. The optional external translator check was excluded as well.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed, exit code 0.
- `git diff --check`: passed.

GPU-only tests were skipped on the original recording check host, which had
no GPU. These checks
establish retained state, fallback behavior and build compatibility, not GPU
execution. Temporary files and dependency caches for the final checks were
kept within the checkout.

## Integrated verification

The recording contracts share one vertex-binding record with the bounded
draw experiment. The census check now leaves only texture clear and the
resolver unsuccessful. It also checks the nine follow-up rows against the
original census counts. The request manifest remains the exact 534-name
aggregate set.

Checks on the integrated implementation:

- Default workspace tests: 42 unit tests and four document tests passed.
- All-feature workspace tests with ignored tests included and one test
  thread: 64 unit tests and 23 integration tests passed. No tests were
  ignored or skipped. This includes translated drawing, indexed drawing,
  depth, stencil, rasterizer state, arena memory, presentation and persistent
  pipeline cache tests on a discrete Vulkan device.
- The translated arena test matched all 156 synthetic GPU cases.
- Default and all-feature clippy checks covered all workspace targets with
  warnings denied.
- Clippy also checked both generated translator test packages with warnings
  denied.
- Formatting and whitespace checks passed.
- The six preceding feature commits and this recording integration were
  audited for private paths, checkout names and platform names in prose.
  Identifiers and public source references remain literal evidence.

The implementation and conflict resolution used the repository's signatures,
shapes, census and provenance notes. No new external source was needed.
