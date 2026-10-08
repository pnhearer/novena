//! Optional translated first draw proof. Provenance: 0027.
#![cfg(feature = "shadowbox")]
#[path = "support/shadowbox.rs"]
mod support;

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, the flat arena and spirv-val"]
fn first_draw_executes_translated_triangle() {
    support::run("first_draw_executes_translated_triangle");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan and spirv-val"]
fn primitive_topologies_read_back_pixels() {
    support::run("primitive_topologies_read_back_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, glslangValidator and spirv-val"]
fn vertex_formats_read_back_pixels() {
    support::run("vertex_formats_read_back_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, the flat arena and spirv-val"]
fn uniform_banks_colour_two_draws() {
    support::run("uniform_banks_colour_two_draws");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, glslangValidator and spirv-val"]
fn uniform_banks_with_strip_and_normalized_attribute() {
    support::run("uniform_banks_with_strip_and_normalized_attribute");
}
