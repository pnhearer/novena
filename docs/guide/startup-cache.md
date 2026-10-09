# Startup shader cache

The startup cache indexes translated shaders before the host registers
programs. It also loads saved graphics recipes and queues their pipelines when
a Vulkan backend is available. It contains translated output, never original
shader bytes. Use a private host-owned directory and keep it outside the source
tree.

Create a `StartupCacheConfig` with that directory. Select translation context,
worker count, and queue capacity, then pass the config and an
`Arc<dyn ShaderTranslator>` to `Instance::with_startup_cache` or the unsafe
`Instance::with_host_startup_cache`. Construction enables translation.
The hosted constructor has the same lifetime and concurrency requirements as
`with_host`. `configure_startup_cache` requires exclusive access and must run
outside submission.

## Translator contract

Implement `translation_cache_key` and `translate_for_context` in addition
to `translate`. The key must be the translator's exact public cache identity,
including code, canonical header, target generation, translator version,
options, and stage-relevant state. The library does not invent a key.

Return `Ok(None)` if required state is missing. This increments
`state_missing` and produces no substitute translation. The default trait
hook reports identity unavailable, so a translator implementing only
`translate` can use the ordinary hook but cannot populate the startup cache.

Preserve subgroup requirements in `TranslatedShader`. The graphics path
currently rejects a result that requires subgroup size 32. Changing context
affects future registrations; existing owned requests retain their context.
Register a program again when its translation inputs change.

## Registration and completion

Registration checks the in-memory index first. A hit needs no worker translation
or disk read. A miss owns the bounded shader bytes and queues work. Queue-full
registrations retain bytes for a later retry without rereading program memory.
Completion cannot replace a newer registration's result.

Translation workers write records and publish results. A draw may see pending
translation or pipeline compilation and skip for that submission. Resubmit
after completion. The frame path does not wait for compilation or do cache
file I/O.

## Files and limits

A 64-character lowercase hexadecimal identity names a `.sbc` record.
Records have a version marker, identity, SHA-256 payload checksum, subgroup
metadata, and little-endian SPIR-V words. Loads check regular-file bounds,
identity, checksum, and the SPIR-V header. This is not full module validation.
Symlink directory entries and temporary records are ignored.

Each record is limited to 64 MiB. The loaded index is limited to 256 MiB.
Invalid records produce bounded diagnostics rather than valid hits.
Writes use a unique temporary file followed by rename. Saved graphics recipes
include shader keys and complete pipeline state. Construction schedules recipes
whose stages are loaded and supported; bounded queue pressure can delay them.

## Observing reuse

Read `startup_cache_stats` for loaded entries, hits, misses, missing state,
translations, summed worker translation time, queue pressure, invalid records,
queued startup pipelines, and pending requests.
`take_startup_cache_diagnostics` returns and clears accumulated messages.
Use `graphics_pending_count`, `graphics_cache_stats`,
`graphics_persistence_stats`, and `take_graphics_cache_diagnostics`
to distinguish translation reuse from pipeline scheduling and driver-cache loading.

For the external translator interoperability check and the fresh-process timing
test, follow [building and testing](building.md). Provenance records exact
pixels, restart reuse, and two measured sets of 40 cold/warm pairs.
Those results describe one synthetic workload and host. They do not guarantee
a speedup for all workloads or prove that a driver performed no compilation.

Source: `startup_cache.rs`, `workers.rs`, and the cache methods in
`instance.rs`. Evidence:
[startup contract](../provenance/0031-startup-shaders.md) and
[validation and measurements](../provenance/0032-startup-cache-validation.md).
