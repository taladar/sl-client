---
id: viewer-p15-2
title: Wearable layer inputs
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 15 — Client-side baking (`sl-bake`, the OpenSim/legacy path)
---

Context: [context/viewer.md](../context/viewer.md).

**P15.2. Wearable layer inputs.** Read the agent's worn wearables
(`AgentWearables` / the COF), fetch each wearable **asset** (skin / tattoo /
clothing / alpha) to get its layer texture ids + tint (which visual params
colour a layer, e.g. skin tone), and decode the layer textures through the
shared `TextureManager`. Assemble the per-region layer lists `sl-bake` needs.
Done: `sl-proto` gained the per-wearable `TextureEntry` layer-slot constants +
a `LAYER_TEXTURES` name/wearable-type table; `sl-avatar` a `WearableAsset`
parser (the `LLWearable` text format) and a `bakecolor` tint evaluator
(`ColorRamp`/`ColorOp` + `LLTexGlobalColor`/`LLTexParamColor`
`calculateTexLayerColor`, keyed to the three `<global_color>`s); `sl-bake` a
`plan` module — the ordered worn-wearable layers per region (from
`avatar_lad.xml`'s `<layer_set>`) and `region_layers`, which resolves each
planned layer's texture + tint into the compositor's `Layer` list. The
viewer's new `bake_inputs` module drives our own avatar: `RequestWearables` →
fetch each wearable asset over `ViewerAsset` (a `WearableAssetManager`
mirroring the texture/mesh managers) → parse → request its layer textures →
assemble the per-region lists into an `OwnBakeInputs` resource. Live on
OpenSim the default outfit assembles
`head=2 upper=3 lower=3 eyes=1 skirt=0 hair=1`.
**Scope note:** only worn-wearable *texture* layers (skin bodypaint, clothing,
tattoos, alpha masks) plus the solid skin-tone base are modelled — the
reference viewer's procedural cosmetic param-layers (skin shading, make-up,
freckles, bump maps) need a per-param procedural renderer the P15.1 compositor
does not have and are left to a follow-up. Rendering these inputs onto the
body is P15.3.
