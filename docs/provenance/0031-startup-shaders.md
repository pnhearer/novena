# 0031: startup shader loading

- Date: 2026-10-09
- Author: Khargoosh
- Covers: startup cache loading, registration lookups, owned translation requests,
  saved graphics recipes and startup pipeline scheduling

## Sources and scope

The guest memory boundary remains signatures 0007 through 0009 and provenance
[0021](0021-program-layout.md). Pipeline state and shader interfaces remain
[0026](0026-persistent-pipelines.md), [0028](0028-vertex-decoding.md),
[0029](0029-uniform-banks.md) and [0030](0030-textured-blended-drawing.md).
No guest layout, instruction encoding or enum mapping is added.

This is original host cache and scheduling work. The translator's public cache
signatures supply identity, generation, translation options, graphics state and
subgroup metadata. An original CPU experiment calls its cache writer, loads the
result through an instance, and then checks that its reader accepts a runtime
append. No decoder, emitter, optimizer or external cache adapter implementation
was copied. No container, application artifact or recorded external cache was
used. Synthetic instruction bytes retain the established evidence in provenance
0023 and 0025.

## Cache contract

Construction loads a private host-owned directory into a memory index. Each
64-character lowercase hexadecimal identity names a `.sbc` record. The envelope
has an eight-byte version marker, identity, SHA-256 payload checksum, one
subgroup-requirement byte and little-endian SPIR-V words. Module payloads require
at least the five-word header and its magic value. Reads accept bounded regular
files; directory entries that are symlinks are skipped. The per-record limit is
64 MiB and the loaded index limit is 256 MiB. Temporary files are ignored.

The host translator adapter computes the exact identity using its public cache
function. It must include the code hash, canonical header, target generation,
options, translator version and state relevant to that stage. Returning no key
reports required missing state and produces no substitute translation. The
library neither guesses a key nor derives translation state from opaque data.
Adapters that only implement the earlier translation trait remain usable without
startup caching. Their default cache hook reports that identity is unavailable.

Registration looks up the loaded index before submitting any translation.
Successful loaded records return ready owned results. Misses capture owned bytes
and context and use the same bounded worker implementation as pipeline creation.
Duplicate pending requests share completion. Queue overflow retains bytes for a
later draw to retry without guest reads. Pending work skips the draw according
to the existing executor policy. Failed work remains visible in the request.
Replacing program records replaces their handles and clears old late-read
records. Completion never follows guest pointers or overwrites a newer object.

Successful misses update the memory index and publish an entire compatible
record. Writers use exclusive temporary files, file flush, atomic replacement
and directory flush. Concurrent writers cannot expose partial payloads. Cache
I/O errors are collected as diagnostics and preserve successful translation.
Corrupt input is a miss and fresh translation repairs the record. Checksums
check integrity, not the semantics or authenticity of a module.

Statistics report loaded records, registration hits and misses, missing-state
queries, successful translations, measured translation nanoseconds, rejected
queue submissions, invalid records, pending worker requests and queued startup
pipelines. Deferred draw retries do not inflate registration counts.

## Pipeline scheduling

A translated module does not establish complete pipeline state. Hosts can
supply fully interpreted pipeline recipes at construction. The draw executor
also remembers successful recipes, including all translation identities, vertex
input, topology, depth/stencil, raster, attachment and blend state and storage
mode. Recipes contain no shader bytes, object addresses or filesystem paths.
Publication runs on background workers and uses a versioned `.recipe` file.

Construction reopens the persistent driver pipeline cache before queuing known
recipes. Missing stages, invalid state and unsupported subgroup requirements
produce diagnostics. Queue-full recipes remain available for later scheduling.
The first draw uses the same pipeline pool and key, so a completed startup
request is reused. Driver synchronization and persistence remain the existing
pipeline service's responsibility. This executor supports the existing vertex
and fragment stage pair. Other stage combinations remain unsupported.

The subgroup requirement is retained on memory and disk hits. The existing
executor has no required-subgroup-size contract, so it refuses such pipeline
recipes and draws instead of silently discarding that requirement.

## Initial validation and CPU measurements

CPU tests cover restart hits, exact metadata retention, malformed and partial
records, checksummed invalid metadata, oversized and nonregular entries,
concurrent publication, unavailable cache storage, generation separation,
missing state, duplicate work, replacement ownership, queue overflow, failure,
panic recovery and recipe reopening with pipeline-queue retry.

A synthetic CPU draw-readiness experiment uses the production worker service
for translation and pipeline scheduling. Its original translator has an 8 ms
cost for each of two programs and its simulated pipeline compiler has a 2 ms
cost. The measured time begins before cache construction and ends when the first
synthetic draw can obtain a ready pipeline. Both runs include pipeline creation;
the warm run queues it at startup and invokes no translation. These are CPU
scheduling measurements, not GPU frame timings or estimates of real shader
translation throughput.

The external compatibility experiment additionally measures actual translation
of an original minimal program and time from instance construction to module
readiness. It checks AOT import, zero translation calls on a warm registration,
a runtime miss, acceptance of the appended record by the translator's own
reader and restart reuse. During initial validation, the configured translator
exposed an earlier cache interface, so that specific check skipped with a reason.
The same check passed against a cache-interface source snapshot generated inside
the test output directory. Existing CPU translator checks ran against the
configured checkout. No external checkout was modified.

The initial build container had no GPU, so its GPU checks were left skipped.
The subsequent integration and real GPU startup measurements are recorded in
[0032](0032-startup-cache-validation.md).

Five isolated samples gave these median measurements. Each warm run avoided
all translation calls for the programs in that experiment.

| CPU experiment | Cold readiness | Warm readiness | Translation time avoided |
| --- | ---: | ---: | ---: |
| Simulated first draw, two programs | 30.240 ms | 2.301 ms | 16.170 ms |
| Actual translator, minimal module | 9.672 ms | 3.012 ms | 0.647 ms |

Readiness includes construction, scheduling and publication costs. The difference
between readiness times is not all translation work. The second experiment
measures module readiness, since the initial container could not execute a GPU draw.

Formatting, default and all-feature workspace clippy with warnings denied,
default and all-feature workspace tests, and isolated cache-interface clippy and
tests completed with exit code 0. The default library suite passed 49 tests and
its documentation consistency suite passed four. The all-feature library suite
passed 71 tests with two GPU checks ignored, and documentation consistency again
passed four. All nine new all-feature CPU cache tests passed. Existing GPU-only
integration checks remained ignored. The optional cache-interface wrapper
reported its older-checkout skip explicitly; the isolated snapshot proof passed.
