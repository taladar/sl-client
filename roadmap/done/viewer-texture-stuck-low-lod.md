---
id: viewer-texture-stuck-low-lod
title: A texture can stay stuck at a low-res prefix and never upgrade to a finer level that exists
topic: viewer
status: bugs
origin: noticed while live-testing viewer-avatar-state-dump-replay (2026-08-06)
---

Context: [context/viewer.md](../context/viewer.md).

Observed in the live viewer: an in-world sign (and, generally, some prim
textures) rendered **blurry across several frames** even though a sharper
resolution of that texture is available and could be fetched. The texture is
stuck at a low-LOD prefix and never upgrades to the finer level.

This is **separate** from the two grow-path fixes made under
[[viewer-avatar-state-dump-replay]] (which stopped a texture being *dropped*
when a grow-fetch fails, and stopped re-fetching an already-complete
codestream). Here the texture is not dropped — it just never sharpens.

The `sl-texture` store already documents one instance of this failure mode
(`store.rs`, `ensure_codestream`): the per-LOD byte *estimate* prefix decodes
cleanly to a lower resolution, OpenJPEG returns success, so the decode-error
fallback never fires and a "full-res" image is silently stuck at a reduced size.
That was addressed for a **full-res (discard 0)** target by fetching the whole
codestream. The remaining case to investigate is a texture whose requested
target LOD never rises to full res in the first place — i.e. the render-priority
driver (`drive_render_priority`, P20.2) not raising the texture's target from
the on-screen pixel area, or the upgrade path not being re-triggered once a
nearer / larger view warrants a finer level.

Repro: view a signed/large-texture prim at a distance, then approach — the
texture should sharpen and sometimes does not. Likely reproducible offline via
`--replay` on a bundle that contains a full-resolution texture (the bundle now
carries complete codestreams), by framing the prim at varying distances.
