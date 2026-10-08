//! Optional translator experiments. Provenance: docs/provenance/0024-gmem-flat-merge.md.

#![cfg(feature = "vulkan")]

#[path = "support/shadowbox.rs"]
mod support;
use support::run;

#[test]
fn synthetic_translations_match_novena_push_constants() {
    run("synthetic_translations_match_novena_push_constants");
}

#[test]
#[ignore = "requires Shadowbox, a Vulkan GPU, 1 GiB coherent device-address memory and spirv-val"]
fn shadowbox_programs_execute_in_novena_arena() {
    run("shadowbox_programs_execute_in_novena_arena");
}
