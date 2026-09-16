// Terrain texture-splat material: blends a region's four ground ("detail")
// textures by a per-vertex four-component weight (computed on the CPU from the
// ground elevation by the `sl-terrain` crate) and lights the result from the sky
// the way the reference's deferred `softenLight` lights its legacy terrain
// (`sl_client_bevy::sky_lighting`), including the sun's cascaded shadow maps so
// the ground receives shadows cast by avatars, prims and terrain relief.
//
// It reads the shared view + light bind group (group 0) for the sun's direction
// and shadows, and the shared sky-lighting texture for the sky's colours. Advanced
// terrain materials (PBR / normal / specular) remain a deferred non-goal.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    shadows,
    view_transformations::position_world_to_clip,
}
#import sl_client_bevy::sky_lighting::{
    sky_irradiance,
    sky_legacy_diffuse,
    sky_legacy_finish,
    sky_lighting_from_texels,
    sky_lighting_is_resolved,
    sky_surface_light,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var detail0_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var detail0_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var detail1_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var detail1_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var detail2_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var detail2_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var detail3_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var detail3_sampler: sampler;

// The shared sky-lighting texture (`sl_client_bevy::sky_lighting`): texel 0 is
// `(sunlit, mode)`, texel 1 `(amblit, probe_ambiance)`. Read with `textureLoad`, so
// the sampler beside it is only there because a material texture binding comes
// with one.
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var sky_lighting_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var sky_lighting_sampler: sampler;

// The flat light the ground falls back to before any sky has been resolved (a test
// scene with no environment): the texture's seed `sunlit` and `amblit` as a plain
// sun term and ambient.
fn fallback_light(sunlit: vec3<f32>, amblit: vec3<f32>, diffuse: f32, shadow: f32) -> vec3<f32> {
    return amblit + sunlit * (diffuse * shadow);
}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) weights: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) weights: vec4<f32>,
    @location(3) world_position: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.clip_position = position_world_to_clip(world_position.xyz);
    out.world_position = world_position;
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
    out.uv = vertex.uv;
    out.weights = vertex.weights;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Re-normalise the interpolated weights so the blend stays energy-preserving
    // between vertices; fall back to the lowest detail texture if they vanish.
    let clamped = max(in.weights, vec4<f32>(0.0));
    let total = clamped.x + clamped.y + clamped.z + clamped.w;
    let weights = select(vec4<f32>(1.0, 0.0, 0.0, 0.0), clamped / total, total > 0.0001);

    let c0 = textureSample(detail0_texture, detail0_sampler, in.uv);
    let c1 = textureSample(detail1_texture, detail1_sampler, in.uv);
    let c2 = textureSample(detail2_texture, detail2_sampler, in.uv);
    let c3 = textureSample(detail3_texture, detail3_sampler, in.uv);
    let base = c0 * weights.x + c1 * weights.y + c2 * weights.z + c3 * weights.w;

    let normal = normalize(in.world_normal);

    // Direction toward the active sun / moon, taken from the scene's first
    // directional light so the ground tracks the day cycle. Fall back to a fixed
    // overhead-ish direction if (unexpectedly) no directional light is present.
    var sun_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    var shadow = 1.0;
    if (view_bindings::lights.n_directional_lights > 0u) {
        let light = &view_bindings::lights.directional_lights[0];
        sun_dir = (*light).direction_to_light;

        // P24: sample the directional light's cascaded shadow maps so the ground
        // receives shadows. `view_z` is the fragment's depth in view space (what
        // selects the cascade); `clip_position.xy` is the fragment coordinate.
        if (((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            let view_z = dot(vec4<f32>(
                view_bindings::view.view_from_world[0].z,
                view_bindings::view.view_from_world[1].z,
                view_bindings::view.view_from_world[2].z,
                view_bindings::view.view_from_world[3].z,
            ), in.world_position);
            shadow = shadows::fetch_directional_shadow(
                0u,
                in.world_position,
                normal,
                view_z,
                in.clip_position.xy,
            );
        }
    }

    let lighting = sky_lighting_from_texels(
        textureLoad(sky_lighting_texture, vec2<i32>(0, 0), 0),
        textureLoad(sky_lighting_texture, vec2<i32>(1, 0), 0),
    );
    var color: vec3<f32>;
    if (sky_lighting_is_resolved(lighting)) {
        // The reference's legacy branch of `softenLight`: terrain writes its splat
        // to the G-buffer like any other legacy surface and is lit there. The detail
        // textures are uploaded sRGB, so `base` is already the linear albedo the
        // branch converts its G-buffer colour to.
        let light = sky_surface_light(lighting, normal, sun_dir);
        let irradiance = sky_irradiance(lighting, light, normal);
        let diffuse = sky_legacy_diffuse(lighting, light, irradiance, normal, sun_dir, shadow);
        color = sky_legacy_finish(lighting, diffuse.light * base.rgb);
    } else {
        let diffuse = max(dot(normal, sun_dir), 0.0);
        color = base.rgb * fallback_light(lighting.sunlit, lighting.amblit, diffuse, shadow);
    }
    // Alpha carries the SL glow mask (the viewer's `glow` pass): terrain never
    // glows, so it writes 0. The surface is opaque, so this alpha is not a blend
    // factor.
    return vec4<f32>(color, 0.0);
}
