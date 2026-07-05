# sl-bake

Pure **client-side avatar bake compositing** for Second Life / OpenSim clients:
given the ordered per-region wearable layers as decoded textures plus their
parameters, it composites each avatar bake region (head / upper body / lower
body / eyes / skirt / hair) into a single baked RGBA image.

This is the *legacy / OpenSim* baking path. Modern Second Life bakes on the
server and publishes the result (the viewer just fetches the baked UUIDs); but
OpenSim — and any grid without server-side baking — expects the **client** to
composite the bake from the avatar's worn wearable layers. Without it our own
avatar there is an untextured cloud. This crate is that compositor; sourcing and
decoding the layer textures, and rendering / uploading the result, live in the
runtime crates.

Like its `sl-avatar` / `sl-mesh` / `sl-sculpt` / `sl-texture` siblings it is
**Bevy-free and I/O-free**: it never fetches or decodes, taking already-decoded
`sl_texture::DecodedImage` layers and returning a plain RGBA8 `BakedImage`. The
caller sources the decoded layer textures from the shared `sl-texture`
`TextureStore`.

The model follows the reference viewer's `LLTexLayerSet` compositing,
reimplemented idiomatically rather than copied: a region is a stack of layers
composited bottom-to-top over a transparent canvas —

- a **base** layer (the skin) writes all RGBA channels, giving the region its
  opaque foundation;
- **blend** layers (tattoo, clothing) are alpha-composited *over* what is below,
  each modulated by a tint colour and an overall opacity;
- **alpha-mask** layers (alpha wearables) carve the destination alpha channel so
  the underlying body is hidden where the mask is opaque.

Each layer texture is bilinearly resampled to the bake resolution, so the input
`DecodedImage`s need not share a size. Tint is an RGBA multiply whose alpha is
the layer's opacity; a layer with no image is a solid tint fill.

## Usage

```rust
use sl_bake::{composite_region, BakeRegion, Layer};

fn bake(layers: &[Layer]) {
    let baked = composite_region(BakeRegion::UpperBody, 512, layers);
    // baked.pixels is size*size*4 RGBA8, ready to upload or drape as a material.
    let _ = baked;
}
```

`BakeRegion` names the six base-body bakes and maps to the `sl_proto`
`avatar_texture` baked-slot index. Assembling the per-region `Layer` lists from
the agent's worn wearables (P15.2) and rendering / publishing the composite
(P15.3 / P15.4) is the runtime crates' job.
