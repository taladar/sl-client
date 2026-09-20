---
id: viewer-audit-decoded-texture-uploaders
title: Eight independent DecodedTexture uploaders each re-decide colour space
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

Done (2026-09-20): there is one uploader. `sl_client_bevy::upload_pixels` is the
only place a **world** texture becomes a Bevy `Image` — a UI overlay or a render
target that deliberately wants another sampler still builds its own and says so
there — and `upload_decoded` is its `DecodedImage` front. What varied by
convention is now a typed argument, `TextureUpload`:

- **Colour space** is the argument's to state — `TextureUpload::COLOR` for a
  picture (sRGB, GPU-decoded on sample), `TextureUpload::DATA` for pixels a
  shader must read verbatim. The `ColorSpace` doc carries what the mistake costs
  in both directions, with the two defects it has actually caused here (the
  skewed sea wavelets, the clouds in one quadrant).
- **The address mode is not a parameter**, and deliberately so — see below.
- **Filtering** is one flag, `TextureUpload::crisp()`, for the one texture whose
  texels are meant to be seen individually (the R22 UV-diagnostic grid).

The eight forks are gone: `to_bevy_image`, `build_prim_image`,
`build_linear_image`, `build_srgb_image`, `build_pbr_image`,
`water_normal_image`, `cloud_noise_image` and `build_particle_image` no longer
exist, and neither do the three hand-rolled `Image::new` + sampler blocks in
`bump.rs`, `terrain.rs` and `particles.rs`'s default sprite. Where a role's
choice had a reason worth keeping, the reason moved to a named constant that
states it once — `legacy_materials::NORMAL_MAP_UPLOAD` / `SPECULAR_MAP_UPLOAD`,
`water::WAVE_NORMAL_UPLOAD`, `sky::CLOUD_NOISE_UPLOAD`, `materials`'s
`pbr_map_upload(srgb)` — so a call site names the role and the uploader performs
it.

Three ways this differs from the scope as written, each on purpose:

- **It lives in `sl-client-bevy`, not `sl-viewer-kit`.** The scope said the kit,
  but `to_bevy_image` — the function the audit blames for the forks — is in
  `sl-client-bevy`, which sits *below* the kit. A parameterised uploader one
  layer up would have left the hardcoded one underneath, still the path of least
  resistance for the next caller. Replacing it where it lives removes the cause;
  every crate that forked it already depended on that crate.
- **No `AddressMode` parameter.** Every texture this viewer uploads repeats:
  that is a universal rule with a declared, per-geometry exception
  (`render_scene::SamplerMayClamp`), and `render_test::sampler_violations`
  enforces it scene-wide. No upload site today wants anything else, so an
  address-mode argument would be an API with no caller — see
  [[idiomatic-audit-dead-forward-api]]. The rule is stated on `upload_pixels`
  and pinned by a test instead. This also corrected a comment that had drifted:
  `build_particle_image` claimed it clamped like the reference's `TAM_CLAMP`
  while in fact inheriting `to_bevy_image`'s repeat. The behaviour was right (a
  billboard quad's UVs span exactly `[0, 1]`, so the two are indistinguishable)
  and only the comment was wrong — which is the shape of defect the audit
  predicted.
- **No `ColorSpace` field on `DecodedImage`.** The "worth doing in the same
  pass" half cannot be made true. Colour space is a property of the **slot a
  texture is bound to**, not of the decoded pixels: `materials.rs` caches its
  images under `(TextureKey, srgb)` precisely because the *same*
  `Arc<DecodedTexture>` is uploaded both ways, and `decode_j2c` — where the
  field would be set — has no idea what the codestream it is decoding will be
  used for. A field there would be a lie on the main path and would make the
  convention harder to see, not mechanically enforceable. The typed argument at
  the upload site is where that decision can be honest, and that is what landed.

Pinned by three tests on the one implementation
(`sl-client-bevy/src/textures.rs`): the colour space picks the texture format,
every upload repeats on every axis whatever else it asks for, and `crisp`
changes the filter without touching the colour space.

There were eight independent `DecodedTexture -> Image` uploaders:
`sl-viewer-world-api/src/lib.rs:6521`,
`sl-viewer-world-objects/src/legacy_materials.rs:213` and `:239`,
`materials.rs:700`, `bump.rs:347`, `sl-viewer-world-scene/src/water.rs:583`,
`sky.rs:1351`, `particles.rs:1213`,
`sl-viewer-world-objects/src/textures.rs:1713`.

They existed because the shared `sl_client_bevy::to_bevy_image`
(`sl-client-bevy/src/textures.rs:31`) hardcoded `Rgba8UnormSrgb` plus `Repeat`,
so every consumer needing linear or clamp forked it — and each fork re-decided
colour space and address mode **by convention**. This is the known
normal-maps-must-be-linear trap, now replicated eight ways.

Scope: one parameterised `upload_decoded(decoded, ColorSpace, AddressMode)` in
`sl-viewer-kit`, so the choice is a typed argument rather than a convention.

Worth doing in the same pass: `DecodedImage` (`sl-texture/src/decode.rs:18`)
records `components`, `discard_level`, `min_alpha` and `max_alpha` but **not**
whether its pixels are sRGB colour or linear data. A `ColorSpace` field set at
the decode site would make the project's own rule mechanically enforceable
instead of a convention each uploader re-derives.
