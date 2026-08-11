---
id: viewer-texture-stuck-low-lod
title: A texture can stay stuck at a low-res prefix and never upgrade to a finer level that exists
topic: viewer
status: done
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

## Resolution (2026-08-11)

Root cause confirmed in `sl-texture`: two compounding bugs. (a)
`ensure_codestream` forced a complete-codestream fetch only for **discard 0**
(`target.is_full()`); every intermediate managed level used the `1/8`-rate byte
*estimate*, which under-fetches a resolution-progressive codestream for a
poorly-compressing texture (a sign with sharp text). (b) `decode_j2c` stamped
the decoded image with the **requested** discard level regardless of the
resolution OpenJPEG actually reconstructed, so a short prefix that decoded
*successfully* to a coarser image was labelled finer than its pixels — which
permanently satisfied both the manager's `desired == current` no-op guard and
the store's `is_at_least_as_fine_as` early-out. The non-determinism came from a
transient `GetTexture` 503 / reset during the grow-to-full: `ensure_codestream`
returns the short prefix on a fetch error (`current.covered() > 0`), which then
decoded coarse and mislabelled.

Fix (`sl-texture/src/store.rs`): a new `achieved_discard` derives the level the
pixels actually hold from the decoded dimensions vs the J2C header native size;
`upgrade` now (1) escalates the estimate→full-bound refetch for **any** level
that decoded coarser than requested (not just discard 0), and (2) labels the
image with the achieved level so no downstream check believes it holds a finer
level than it does. Unit tests: `achieved_discard_counts_halvings_from_native`
(pure) and `under_fetched_upgrade_escalates_and_labels_honestly` (end-to-end,
`encode` feature) — a high-frequency 256² image served truncated then whole,
asserting the store escalates and reaches the honest target level.
