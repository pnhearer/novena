//! Optional external cache contract check. Evidence: provenance 0031.
#![cfg(feature = "shadowbox")]
#[path = "support/shadowbox.rs"]
mod support;

#[test]
fn emitted_cache_is_loaded_at_instance_creation_and_runtime_misses_reopen() {
    support::run("emitted_cache_is_loaded_at_instance_creation_and_runtime_misses_reopen");
}
