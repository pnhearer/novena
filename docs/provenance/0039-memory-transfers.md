# 0039: bounded memory transfers

- Date: 2026-10-09
- Covers: host write notifications, coherent storage adoption, submission range
  completion, and bounded device writeback.

## Evidence

Pool storage, size, address and alias choices continue provenance 0022.
Command meanings continue provenance 0037. No new guest signature, flag value,
layout or command interpretation was inferred. Sources were the retained
signature, shape, census and provenance records, original synthetic experiments,
and public Vulkan documentation.

[Memory properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkMemoryPropertyFlagBits.html)
define host visibility and coherence.
[Synchronization](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
requires completion and memory dependencies before host access to device writes.
[Timeline waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitSemaphores.html)
allow waiting for a selected submission value.
[The shader environment](https://docs.vulkan.org/spec/latest/appendices/spirvenv.html)
defines storage classes, pointer access and descriptor decorations.

The tests use original bytes and shader source. Device tests fail when the
required device or shader tools are absent.

## Host writes

A host enables notifications for a registered pool with
novena_instance_track_pool_writes. It must then report every storage change
with novena_instance_notify_memory_write before submission. Library-originated
host writes are recorded automatically. Device downloads do not become new host
dirtiness. Enabling tracking again preserves existing state.

Dirty host pages are 4096 bytes, clipped to live pool bounds and coalesced.
Contained aliases share the tracking mode and canonical storage. Each canonical
byte is visited only once. A new pool imports its complete initial contents.
Subsequent uploads visit dirty pages only. Callback failure retains unfinished
ranges and leaves a failed upload chunk unchanged.

The existing callback contract cannot observe arbitrary host writes.
Hosts without notifications retain bounded change inspection through callbacks.
Only changed pages are copied into the arena, but inspection still reads all
registered storage. Inspection bytes have a separate counter. The efficient
copied-storage path requires notifications.

## Direct coherent storage

Arena allocation already requires host-visible coherent memory. Coherence
alone does not alias separately owned guest storage.

novena_instance_map_pool preserves pool contents and returns coherent arena
storage for host adoption. The host routes guest memory access and callbacks
through that backing. Aliases share the adoption choice. Submission upload and
download skip callbacks for adopted storage.

The pointer remains allocated until the last alias is released. Before each
mapped access, the host calls novena_instance_wait_pool and serializes that
access with submissions. Writes wait for device readers as well as writers.
Mapped writes must be notified before the next submission to invalidate retained
image state. Adoption preserves the recorded guest storage address.

## Device writes and completion

Fills and buffer copies register destination intervals. Copies also register
source reads. Compute storage bindings register their declared writable ranges;
read-only bindings and modules without storage writes need no download.
Image stores register packed arena extents. Counter reports register 16 bytes,
covering both the host-produced visibility word and the device timestamp.
Downloads use exact intervals, preserving neighboring host bytes even when
they share a dirty upload page.

Every asynchronous arena access retains its submission timeline and value.
A host read waits only for overlapping writers. A host write or release waits
for overlapping readers and writers. Completed access records are retired.
Memory transfer, arena access, and counter execution do not wait for device idle.
Occlusion collection waits for the drawing submission that produced the query.

Bounded command-frame reuse and existing shader-to-host barriers remain.
Compute descriptor resources remain alive through dispatch completion.
Raw external command submissions require caller-managed completion before
arena host access or release.

Shader write extents are conservative bounds. A writable storage descriptor
can download its entire declared range even if the shader changed fewer bytes.
Physical-pointer stores have no descriptor bound and retain full live-pool
writeback. Read-only physical access does not trigger that writeback.
The shaders are not instrumented to discover dynamic store addresses.
This preserves results for existing pointer-based execution while bounding
ordinary declared-resource transfers.

## Measurement

The baseline is revision fd5c745, with only the original synthetic benchmark
added before changing production code. Both measured versions use the same
release feature selection. One pool contains 512 MiB. Each measured
submission changes four host bytes, uploads, submits a four-byte device fill with
a host-read barrier, downloads, and verifies the resulting value. An initial
upload precedes measurement. Each mode has 12 samples, including its first
submission. The complete pool and adjacent bytes are checked outside timing.

Payload bytes count guest-to-arena and arena-to-guest transfer extents.
They exclude staging-buffer copies, initial import and explicit direct host
accesses. The baseline uses two CPU copies per direction through staging.
Tracked uploads retain staging for callback failure recovery. Downloads pass a
borrowed coherent slice directly to the callback.

CPU time uses the submission thread's CPU clock. Elapsed time includes device
completion waits. Worker CPU time and setup are excluded. Medians include all
12 samples. The [raw samples](0039-transfer-samples.csv) retain both clocks.

| Mode | Upload bytes | Download bytes | Median CPU, microseconds | Median elapsed, microseconds |
| --- | ---: | ---: | ---: | ---: |
| Before | 536870912 | 536870912 | 122102.817 | 123010.724 |
| Notifications | 4096 | 4 | 15.676 | 66.049 |
| Direct mapping | 0 | 0 | 22.776 | 84.097 |

The efficient path reduces payload from 1 GiB to 4100 bytes per submission.
At 60 submissions per second that is 246000 payload bytes per second rather
than 60 GiB per second. Direct mapping copies no payload at those boundaries.
These are synthetic transfer costs, not frame-rate measurements. Load and device
scheduling affect elapsed time. Unbounded shader stores and hosts without
notifications retain the costs described above.

After configuring translator and external build and scratch storage through
the environment, reproduce the benchmark and checks with:

    cargo test --release --features vulkan --lib memory_transfer_cost -- --include-ignored --nocapture --test-threads=1
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-features --all-targets -- -D warnings
    cargo test --workspace
    cargo test --workspace --all-features --all-targets -- --include-ignored --test-threads=1
    cargo test --workspace --all-features --doc

Device tests cover a 512 MiB pool, page-boundary writes, alias coalescing and
lifetime, failed callbacks and retries, direct mappings, a delayed writer with
an unrelated readable range, exact query and image writeback, writable storage
descriptors and read-only storage inputs. The host-boundary test checks
notification routing, automatic host writes, exact device downloads, and an
empty submission with no new transfer bytes.

## Verification

Formatting, strict Clippy for default and all-feature builds, default tests,
all-target all-feature tests including every ignored device test, documentation
tests, documentation with warnings denied, and C and C++ header syntax checks
pass. External translator checks ran without skip messages.

The three anchored review cases cover measurement, notified pool tracking, and
a read-only storage input paired with a writable output. Their roles are fixed
at two development cases and one held-out case. Corpus admission and the terminal
receipt could not be stored because this checkout has no registered profile.
No candidate was evaluated or promoted.
