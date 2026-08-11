---
id: viewer-mesh-hair-not-rendering
title: Some worn mesh hair does not render (visible in Firestorm)
topic: viewer
status: bugs
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

An avatar's worn **mesh hair** that Firestorm renders is **not rendered at all**
in our viewer (observed live on aditi — one avatar's hair simply missing while
the rest of the avatar draws).

Investigate why that specific worn mesh (hair attachment) is skipped: candidates
— its mesh LOD/asset never fetched or decoded (a decode error dropping the
submesh), all its faces classified fully transparent (an alpha-mode /
transparent-material misclassification hiding it), or a rigged-attachment bind
that silently drops it. Identify the hair asset id live, decode it offline, and
check the face materials / decode path. Distinct from the fully-transparent
box-shell animesh issue but worth cross-checking the transparent-face policy.

## Investigation (2026-08-11) — ranked candidates (needs the live hair asset)

Which one fires can only be settled by decoding the *specific* hair asset
offline; the transparency hypothesis is code-refuted (see below).

1. **Zero-vertex / empty submeshes silently dropped (strongest).**
   `build_rigged_submeshes` (`objects.rs`) skips every submesh that has no
   geometry — `has_geometry()` is false when `no_geometry` is set or the
   positions vec is empty (`sl-mesh/src/decode.rs`). If the hair's finest block
   decodes to zero vertices, *every* submesh is skipped and no entity spawns →
   hair entirely missing. HEAD commit `e33f3a6b` ("stop zero-vertex meshes
   flooding the GPU allocator") confirms zero-vertex decodes genuinely occur, so
   a hair-specific decode gap in `sl-mesh` is plausible. **Check the finest-LOD
   submesh vertex counts offline first.**
2. **Failed finest upgrade → built from an empty coarse block.** A worn rigged
   mesh whose worn status was unknown when far starts managed and is bumped via
   `MeshManager::upgrade_to_finest`; if `store.set_lod(FINEST)` fails, the build
   uses the coarse `entry.mesh()`, and if that block is empty, case 1 applies.
   `apply_rigged_attachments` also skips while `lod_change_inflight`, so a
   never-completing upgrade strands it. (This class is now less likely after the
   2026-08-11 `GetMesh` failure-edge retry, which re-issues a failed fetch, but
   a failed *LOD change* is not on that retry path — cross-check.)
3. **All rig joints unresolved → collapsed onto the pelvis.** `objects.rs`: any
   `skin.joint_names` entry the skeleton lacks falls back to the pelvis; if the
   hair rig's joint names don't match, the hair collapses inside the body and
   reads as "missing". Emits `"rigged mesh {key}: N/M joint(s) unresolved, bound
   to pelvis"` — grep the live log.
4. **Wearer never resolves.** `apply_rigged_attachments` `continue`s if
   `avatars.wearer_of(scoped)` is `None`; normally resolves (the body renders),
   unless the hair's parent chain doesn't reach the avatar. Lower likelihood.
5. **Transparency policy is NOT the cause (code-refuted).** Rigged faces are
   built `TextureAlpha::Blend`, so a transparent hair texture alpha-blends soft
   (correct), never mask-clips; `build_rigged_submeshes` never skips a face for
   being transparent, only for empty geometry. A viewer-side "classified
   transparent → hidden" drop would need the authored TE tint alpha to be 0, but
   Firestorm renders the hair, so it is not authored-transparent.

### Live diagnostic

Identify the hair asset id via the pick tool / `SL_VIEWER_LOG_AVATAR_FACES=1`
(`log_rigged_face`), watch for the `"N/M joint(s) unresolved, bound to pelvis"`
and `"bound rigged mesh … to its skeleton"` log lines, then decode the cached
`~/.cache/sl-client-bevy-viewer/meshcache/<uuid>.mesh` offline and check the
finest-LOD submesh vertex counts (candidate 1) before anything else.
