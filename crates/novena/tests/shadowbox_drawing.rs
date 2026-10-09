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

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, the flat arena and spirv-val"]
fn indexed_draw_executes_translated_triangle() {
    support::run("indexed_draw_executes_translated_triangle");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, the flat arena and spirv-val"]
fn depth_stencil_and_raster_pixels() {
    support::run("depth_stencil_and_raster_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, glslangValidator and spirv-val"]
fn indexed_strip_uniform_depth_pixels() {
    support::run("indexed_strip_uniform_depth_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, glslangValidator and spirv-val"]
fn textured_checkerboard_and_blend_pixels() {
    support::run("textured_checkerboard_and_blend_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan and spirv-val"]
fn translated_texture_pixels() {
    support::run("translated_texture_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, glslangValidator and spirv-val"]
fn multiple_target_blend_pixels() {
    support::run("multiple_target_blend_pixels");
}

#[test]
#[ignore = "requires NOVENA_SHADOWBOX_PATH, Vulkan, glslangValidator and spirv-val"]
fn textured_uniform_banks_and_persistence() {
    support::run("textured_uniform_banks_and_persistence");
}

#[test]
#[ignore = "requires the translator and a Vulkan GPU; fresh-process timing"]
fn cold_and_warm_first_draw() {
    support::run("startup::cold_and_warm_first_draw");
}
