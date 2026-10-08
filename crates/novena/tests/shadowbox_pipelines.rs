//! Optional translated compute proof. Evidence: provenance 0025.
#![cfg(feature = "shadowbox")]

#[path = "support/shadowbox.rs"]
mod support;

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, a Vulkan GPU, the 1 GiB arena and spirv-val"]
fn compute_pipeline_cache_executes_translated_program() {
    support::run("compute_pipeline_cache_executes_translated_program");
}
