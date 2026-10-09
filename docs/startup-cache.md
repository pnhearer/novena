# Startup shader caches

Rust hosts can construct an instance with `Instance::with_startup_cache` or
`Instance::with_host_startup_cache`. Pass a `StartupCacheConfig` containing a
private cache directory and an `Arc<dyn ShaderTranslator>`. Construction loads
compatible translation records and starts bounded background workers. The
hosted constructor also opens the persistent graphics pipeline cache and queues
known pipeline recipes.

The translator adapter must implement `translation_cache_key` using its public
runtime cache identity function. Return `None` for required missing state. Its
`translate_for_context` hook must honor the same generation, options and state,
and retain subgroup requirements. The earlier `translate` hook alone supports
ordinary translation but cannot identify AOT records. The original compatibility
fixture in the translator integration tests demonstrates all three hooks.

Set translation inputs in `StartupCacheConfig::context`. A host can use
`set_shader_translation_context` before subsequent registrations to select a
different variant. Existing requests retain their captured inputs. Register a
program again when its translation state changes. Generation, geometry input
and tessellation values are explicit host adapter choices, not guest enum facts.

Registration returns immediately for an indexed hit. A miss queues owned bytes
for translation and publication. Queue overflow retains bytes and a later draw
retries. Pending translation or pipeline work skips that draw under the existing
executor policy. Worker completion cannot replace a newer registration.

AOT modules alone do not describe vertex input, topology or fixed pipeline
state. Supply `PipelineRecipe` values in `StartupCacheConfig::pipelines` when
those choices are known. Successful draws save equivalent recipes in the cache
directory for the next run. Construction queues supported recipes using the
persistent driver cache. Completed startup requests are reused by ordinary draws.
Avoid replacing the graphics pipeline service after construction if its startup
requests should remain available.

Use `startup_cache_stats` and `take_startup_cache_diagnostics` outside submission
to inspect load failures, hit and miss counts, missing state and translation
time. Graphics persistence statistics remain available through the existing
instance methods. Statistics count each registration lookup; deferred draw
retries do not add hits or misses. Shader bytes are never included in diagnostics.

Cache loading and writing failures preserve runtime compilation. The directory
must be private and host-owned. Records are checked for corruption, but the
cache is not an authenticated source of executable modules. The current graphics
executor accepts its existing vertex/fragment pair and declines modules that
require an unsupported subgroup-size contract.

See [the provenance and CPU measurements](provenance/0031-startup-shaders.md).
