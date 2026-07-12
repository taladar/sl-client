---
id: viewer-p15-4
title: (Optional) Publish the bake
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 15 — Client-side baking (`sl-bake`, the OpenSim/legacy path)
---

Context: [context/viewer.md](../context/viewer.md).

**P15.4. (Optional) Publish the bake.** J2C-**encode** the composited
regions and upload via the existing `UploadBakedTexture` cap so the sim /
other viewers see us. **Needs a J2C encoder** (OpenJPEG encode) — the one
heavy net-new dependency; may slip to a follow-up. Local rendering (P15.3)
does not depend on it. **Done (verified live on OpenSim):** the encoder is a
new `sl-j2c-encode` crate — an in-memory OpenJPEG-C (`openjpeg-sys`, the same
backend `jpeg2k` decodes with) encode of RGBA8 → raw `.j2c` (opaque regions
written RGB, transparency kept as a
fourth component so an alpha-masked bake round-trips), isolated as the only
`unsafe`-FFI crate in the workspace and surfaced through `sl-texture`'s new
`encode` feature as `encode_j2c(&DecodedImage)` (encode→decode round-trip
tested). The viewer's new `bake_publish` module (`OwnBakePublish` +
`drive_bake_publish`) is a one-shot gated on the region advertising
`UploadBakedTexture` (so it is naturally OpenSim-only — Second Life bakes
centrally and never advertises it): once the P15.2 inputs are ready it
composites each region (`composite_own_region`, factored out of
`build_local_bake` so the exact same canonical bytes are draped *and*
uploaded), J2C-encodes it, and uploads the regions **one at a time** (the
`AssetUploaded` reply carries no correlation id, so uploads are serialised
and spread one encode per frame), then advertises the uploaded baked-texture
ids in an `AgentSetAppearance` (`Command::SetAppearance`) so the sim
broadcasts our textured avatar. `CAP_UPLOAD_BAKED_TEXTURE` was promoted to a
public re-export in `sl-client-bevy` (mirroring `CAP_VIEWER_ASSET`). Live on
OpenSim the default outfit uploaded 5 regions
(head/upper/lower/eyes/hair; skirt empty) — the sim accepted every encoded
codestream and returned a fresh asset id per region, and the appearance
published, with the P15.3 local drape unchanged. **Orientation:** the
uploaded bytes are the vertically-flipped composite (the canonical bottom-up
bake orientation SL server bakes are stored in, which is why the P14
fetched-bake drape renders straight), so a real bake and our own upload
agree. **Scope:** the publish carries a *neutral* visual-parameter set —
P15.4 delivers the bake **textures**; publishing the worn **shape** needs
the deferred high-level appearance API (a Phase-14 follow-up note). Verifying
*other* viewers see the result needs a second observer and was not done here;
the sim accepting each upload + the publish is the guarantee.
