---
id: viewer-texture-anisotropic-filtering-missing
title: Face textures are filtered without anisotropy, so oblique texels blur
topic: viewer
status: bugs
origin: seen while verifying viewer-sculpt-sphere-fixture-divergence (2026-09-16)
refs: [viewer-sculpt-sphere-fixture-divergence]
---

Context: [context/viewer.md](../context/viewer.md).

In `sl-crosscheck --scenario catalogue --look-at pbr-box --look-from 3
--look-above 4`, the `sculpt-sphere`'s checker edges are visibly **soft** here
and **crisp** in Firestorm, although both viewers now build the same geometry
with the same texture coordinates. The boxes in the same frame, seen nearly
face-on, are equally sharp in both.

The sphere is where the two texture axes disagree: V runs pole to pole over
half the circumference, U once around, so a texel is about twice as long in
one screen direction as in the other. Isotropic trilinear filtering picks the
mip level for the longer one and blurs the other; anisotropic filtering does
not.

The reference filters anisotropically by default: `RenderAnisotropic` defaults
to `true` in `settings.xml` and sets `LLImageGL::sGlobalUseAnisotropic`
(`llappviewer.cpp`), with a settings listener to change it live. This viewer
sets no `anisotropy_clamp` on any sampler: `materials.rs`,
`legacy_materials.rs` and `bump.rs` all build theirs from
`ImageSamplerDescriptor::linear()`.

## To find out

- Whether anisotropy alone closes the gap on the sphere (set a clamp on the
  face samplers and re-run the cross-check above).
- The anisotropy level the reference ends up with
  (`TFO_ANISOTROPIC` → `GL_TEXTURE_MAX_ANISOTROPY` from the driver's maximum?)
  and whether wgpu's `anisotropy_clamp` constraints (all filters linear) hold
  for every face sampler.
- Wiring `RenderAnisotropic` as a preference, as the reference exposes it.
