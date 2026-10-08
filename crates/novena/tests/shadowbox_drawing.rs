//! Optional translated first draw proof. Provenance: 0027.
#![cfg(feature = "shadowbox")]
#[path = "support/shadowbox.rs"]
mod support;

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, the flat arena and spirv-val"]
fn first_draw_executes_translated_triangle() {
    support::run("first_draw_executes_translated_triangle");
}
