---
id: viewer-render-rig-glow-pass
title: Run the glow pass in the headless readback rig
topic: viewer
status: ready
origin: viewer-render-context-matrix (2026-11-08) — glow over water could not
  be built
points: 3
refs: [viewer-render-context-matrix, viewer-glow-bloom, viewer-render-readback-tier]
---

Context: [context/testing.md](../context/testing.md).

The readback rig renders with `ViewerRenderPlugins::bare()`
(`sl-client-bevy-viewer/src/viewer_plugins.rs`): the material pipelines,
the probes, the transparency ordering and the waterline split, and none of
the post-processes, because those read the environment and the settings
store. So the SL glow pass (`sl-viewer-world-scene/src/glow.rs`, the port
of `generateGlow` + `combineGlow`) never runs under test. A glowing face
reaches the readback frame only as its surface colour, and nothing checks
the halo it should cast or that the alpha glow mask survives each material
path.

- Add the glow pass to the bare stack (or as an opt-in part a cell can ask
  for), fed with fixed `RenderGlow*` values rather than the settings store,
  so a capture is still reproducible.
- It orders after the tone-map pass, which the rig also leaves out today
  (`render_stage.rs` pins `Tonemapping::None` to read markers as exact
  channels). Decide whether a glow cell runs the tone mapper too, or whether
  the glow combine can run over linear output, and say why in the code.
- Teeth: a `glow-prim` subject (a face with a non-zero glow scalar) must
  light pixels **outside** its own silhouette with the pass on, and must not
  with it off. A non-glowing twin must not halo at all.

Unblocks [[viewer-render-matrix-glow-over-water]].
