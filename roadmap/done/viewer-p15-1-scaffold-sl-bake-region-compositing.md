---
id: viewer-p15-1
title: Scaffold sl-bake + region compositing
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 15 — Client-side baking (`sl-bake`, the OpenSim/legacy path)
---

Context: [context/viewer.md](../context/viewer.md).

The server-published path (Phase 14) covers *other* avatars on both grids, and
our *own* avatar on SL. It does **not** cover our own avatar on OpenSim (and any
grid without server bake): those grids expect the *client* to composite the bake
from wearable layers (legacy `UploadBakedTexture`). Without it our own avatar is
an untextured cloud. This phase composites the bake ourselves, primarily for our
own avatar and as the fallback whenever a baked slot is absent / default.

**P15.1. Scaffold `sl-bake` + region compositing.** New pure crate
(scaffold like P12.1; `sl-texture` dep with `default-features = false`). Given
the ordered per-region layers (skin → tattoo → clothing → alpha mask) as
decoded `DecodedImage`s + their params (tint colour, alpha, tex-gen),
composite each bake region (head/upper/lower/eyes/skirt/hair) into a baked
RGBA. Alpha layers carve the alpha channel. Tests over synthetic layers.
`cargo test -p sl-bake`. Done: `BakeRegion` (`region.rs`, mapped to the
`sl_proto::avatar_texture` baked slots) plus a `composite.rs` layer engine —
`Layer` (`LayerKind` Base/Blend/AlphaMask + tint/opacity/`TexGen`/invert
builders, optional image for a solid fill) and `composite_region`, which walks
the stack over a transparent canvas (base writes all channels, blend is
source-over, alpha-mask carves dest alpha — grey masks read via luminance,
4-component masks via their alpha), bilinearly resampling each layer to the
bake size. `BakedImage::to_decoded_image` feeds the composite into the
texture-consuming paths for P15.3. 17 unit tests over synthetic layers.
