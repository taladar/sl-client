---
id: viewer-p35-1
title: Detect HUD
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 35 — HUD attachments
refs: [viewer-p16-1, viewer-p16-2, viewer-p17-2, viewer-p20-2, viewer-p21-1, viewer-p21-2, viewer-p35-2]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** An attachment worn on a HUD point (raw ids `31`..=`38`,
`LLVOVolume::isHUDAttachment`'s verbatim range) is now **classified** as one and
**routed out of the world scene** onto a dedicated screen-space layer — and only
for the agent's own avatar. Nothing draws it yet: [[viewer-p35-2]] adds the
orthographic HUD camera that renders that layer.

Before this, a HUD attachment resolved *no* parent at all — [[viewer-p16-2]]
builds the per-avatar attachment-point nodes off skeleton joints, and a HUD
point hangs off the pseudo-joint `mScreen`, which is not in the skeleton — so
[[viewer-p16-1]] held it pending forever and its geometry sat in the world at
the raw attach-local transform. Live on OpenSim that is a prim floating at the
region corner.

New module `hud.rs`:

- `setup_hud_screen` spawns the **HUD screen** (the `mScreen` equivalent,
  carrying the single Second Life → Bevy basis change so the subtree below stays
  in Second Life space like every other attachment subtree) plus one node per
  HUD point at its fixed `avatar_lad.xml` offset — the screen-space mirror of
  [[viewer-p16-2]]'s body nodes. The offsets come from a new
  `AvatarAssetLibrary::hud_attachment_points()`, exactly the points the body
  table omits.
- The screen `Propagate`s `RenderLayers::layer(1)` down its hierarchy (Bevy's
  `HierarchyPropagatePlugin`, ordered before
  `VisibilitySystems::CheckVisibility`), so every entity of a routed attachment
  — its object entity, geometry holder and each face, including faces spawned
  much later when the mesh decodes — lands on the HUD layer. The world (fly)
  camera and the reflection-probe capture cameras are all on the default layer,
  so none of them draws a HUD.
- `adopt_pending_attachments` routes a HUD-point attachment to the node for its
  point when the wearer is the agent itself, and **hides** it otherwise. That is
  reference behaviour, not a shortcut: `LLVOAvatar::initAttachmentPoints`
  creates the HUD joints for `isSelf()` alone, so another avatar's HUD never
  attaches and never renders there. The wearer is resolved through a new
  `AvatarState::agent_of`; an attachment that arrives before its avatar object
  is retried rather than taken for a stranger's.
- A **rigged mesh worn on a HUD** is built as static geometry in the HUD's own
  space instead of taking [[viewer-p17-2]]'s skinned path, which would parent
  its submeshes to the wearer's in-world body root and drag the HUD straight
  back into the world. The reference viewer warns the user outright that this is
  unsupported (`RiggedMeshAttachedToHUD`); the viewer logs the same and renders
  it unrigged.
- The [[viewer-p20-2]] pixel-area pass would rank a HUD by its distance from the
  world camera — meaningless in screen space, and it would discard its textures
  ([[viewer-p21-1]]) and coarsen its geometry ([[viewer-p21-2]]) to nothing. It
  now gives HUD faces the reference's treatment instead: full-screen pixel area
  (`LLVOVolume::updateTextureVirtualSize`), the finest LOD (`calcLOD`'s
  `cur_detail = 3`), and a new `HUD_BOOST_PRIORITY` above the avatar / sky
  boosts (`LLGLTexture::BOOST_HUD`).

Unit-tested in `hud.rs`: the `31`..=`38` classification, the HUD layer being
distinct from the world layer, the `avatar_lad.xml` offsets landing in screen
space under the basis change, and — the load-bearing one — a Bevy `App`
asserting the render layer really does propagate to a subtree parented *after*
the screen was spawned, which is the only way an attachment ever joins it.

**Verified live on OpenSim.** A cube rezzed and attached to `HudCenter` via
`sl-repl-tokio` (`rez_object`, then `attach_object <local_id> hudcenter`;
OpenSim confirms with `[ATTACHMENTS MODULE]: Updating asset for attachment …,
attachpoint 35`) is picked up by the viewer as
`routed own HUD attachment circuit#1/… to HUD point 35` and no longer appears
anywhere in the world.
