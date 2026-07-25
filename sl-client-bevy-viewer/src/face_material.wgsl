// The Second Life face material shader (the `SlFaceExt` extension of Bevy's
// `StandardMaterial`). Phase 0: a pass-through that reproduces `StandardMaterial`
// forward shading exactly, so migrating every face onto `FaceMaterial` changes no
// pixels. Later phases add, here: (1) re-sampling the normal / metallic-roughness
// / emissive maps at their own per-map UV transforms, and (2) a legacy
// Blinn-Phong specular lobe over the reused PBR lighting.
//
// The material runs NON-bindless (the extension is not bindless, which forces the
// base non-bindless too), so `pbr_input_from_standard_material` reads plain
// `@binding(0)` material data and the extension's own bindings (100+) are plain
// uniforms/textures.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Sample the base `StandardMaterial` (base-colour at its folded UV transform,
    // plus normal / metallic-roughness / emissive at the same transform for now)
    // and build the PBR lighting input.
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Alpha mask / cutoff, exactly as `StandardMaterial` does.
    pbr_input.material.base_color =
        pbr_functions::alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    // Reuse `StandardMaterial`'s metallic-roughness PBR lighting and post
    // processing (fog etc.) unchanged.
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
