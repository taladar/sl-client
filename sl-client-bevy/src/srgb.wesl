// The sRGB decode every colour-texture sample goes through, as one importable
// module (`sl_client_bevy::srgb`).
//
// The reference viewer keeps a fetched texture as plain `GL_RGBA8` and decodes it
// in the shader (`srgb_to_linear`, `class1/environment/srgbF.glsl`) **after** the
// hardware has filtered the encoded bytes. This viewer does the same: a world
// colour texture is uploaded `Rgba8Unorm` (`TextureUpload::WORLD_COLOR`), so a
// sample is the filtered *encoded* value, and the shader that sampled it decodes it
// here. An `Rgba8UnormSrgb` texture would decode each texel first and filter in
// linear light, which turns a black-and-white blend into a lighter grey than the
// reference draws (`viewer-texture-filtering-in-linear-space`).
//
// Alpha is never encoded, so only the colour channels are decoded.

// The reference `srgb_to_linear` (`srgbF.glsl`): the piecewise sRGB curve, per
// channel.
fn srgb_to_linear(cs: vec3<f32>) -> vec3<f32> {
    let low = cs / 12.92;
    let high = pow((cs + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, cs <= vec3<f32>(0.04045));
}

// A colour-texture sample (encoded rgb, plain alpha) with its rgb decoded.
fn decode_srgb_sample(sample: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(srgb_to_linear(sample.rgb), sample.a);
}
