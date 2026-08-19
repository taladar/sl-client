---
id: viewer-avatar-complexity-limit
title: Avatar complexity limiting (jellydoll)
topic: viewer
status: done
origin: render-feature gap analysis vs Firestorm (2026-07); split from viewer-avatar-impostors
refs: [viewer-quick-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

Cap over-heavy avatars so a single griefer-built avatar cannot sink the frame
rate. Score each avatar's render cost (triangles, textures, attachments) and,
past a budget, draw it as a flat "jellydoll" silhouette rather than its real
(attachment-heavy) geometry.

Firestorm drives this from `RenderAvatarMaxComplexity` (the budget) and
`RenderAvatarComplexityMode` (how the cap is applied). Needs a complexity metric
per avatar and the fallback jellydoll render.

Scope: the complexity score, the jellydoll fallback render, the budget
threshold, and the user controls — including a per-avatar "always render fully /
never" override. Relates to the R22 avatar-render work and pairs with the
impostor selection ([[viewer-avatar-impostors-billboard]]).

Quick Preferences: `RenderAvatarMaxComplexity` is a reached-for-hourly knob, so
when the budget setting lands add a default entry for it in the Quick
Preferences panel ([[viewer-quick-preferences]]) — a line in `default_entries()`
(`quick_preferences.rs`) plus a Fluent label. The panel binds by setting key, so
the entry needs only the key, range and label (it was left out of the panel's
first version precisely because the setting did not exist yet).

Reference (Firestorm, read-only): the `llvoavatar` complexity path,
`RenderAvatarMaxComplexity` / `RenderAvatarComplexityMode`.

Builds on: the avatar rendering (P12–P18) and the coarse / interest distance
tracking already in `avatars.rs`.

## Built

`avatar_complexity.rs` holds the whole feature: the score, the decision, and the
jellydoll render.

**The score is the reference's, on purpose.** Residents quote their ARC to each
other and compare it against a shared idea of what is polite to wear, so a
number that meant something different here would be worse than useless. The
constants, the multipliers and the order they apply in are ported verbatim from
`LLVOAvatar::calculateUpdateRenderComplexity` / `LLVOVolume::getRenderCost`: 200
per visible baked body region, then per worn (non-HUD) linkset
`max(5·triangles, 2)` through the per-face and per-prim multipliers (planar,
animated texture ×4, alpha ×4, invisiprim ×1.2, glow ×1.5, bump ×1.25, shiny
×1.6, rigged ×1.2, flexi ×5), plus the additive charges (light 500, media face
1500, particles by burst size, animesh) and `256 + 16·(w+h)/128` per **unique**
texture in the linkset, the whole linkset clamped at a million.

**Triangles come from the asset, not from what is on screen.** The reference's
radius-weighted estimate spreads a prim's four level-of-detail counts over the
annuli each is displayed in, and for a small attachment the *coarse* levels
dominate. Scoring the geometry this viewer happens to be drawing would therefore
both diverge from every other viewer and make the number wobble as the camera
moves. So a mesh's per-level counts are estimated from its asset header's block
byte sizes (`MeshManager::header`, newly retained — the header was previously
discarded after the fetch), and a prim's from the new
`sl_prim::lod_triangle_counts`, a port of `LLVolume::getLoDTriangleCounts` that
generates only the profile ring and the extrusion path and multiplies out the
swept grid.

**Scoring is debounced and budgeted.** An avatar is re-scored only when
something it is made of changed — an attachment arrived, moved or left (chased
up the parent chain by `ObjectState::wearer_of`), or its appearance changed —
at most once a second and at most four avatars a frame. An avatar whose score
was waiting on an asset (a mesh header, a texture's real dimensions) is
re-scored when *that* asset decodes, so the number converges as the crowd rezzes
instead of being wrong for the session.

**The jellydoll** hides every attachment — including the rigged faces that hang
off the wearer's body root rather than off the attachment object, which a naive
"hide the object" would miss — and paints the system body flat grey and unlit.
Hidden geometry is not extracted, so it is not skinned, batched or drawn; that
is where the frame time comes back, and attachments are nearly all of it. The
base regions are forced visible and the hair hidden, exactly as the reference's
`updateMeshVisibility` does, because otherwise a mesh-body wearer would vanish
outright: their system body is baked invisible *because* a mesh body covers it,
and we just hid the mesh body. The previous visibility of everything hidden is
remembered and restored exactly, so a face another pass deliberately hid stays
hidden when the avatar is drawn in full again.

**Controls.** Preferences ▸ Graphics ▸ Avatar complexity (budget, how friends
are treated, attachment-area limit); a Quick Preferences budget slider — the
panel you can open mid-lag, which is the whole point; and a per-avatar
**Render >** sub-pie (Fully / Normally / Never) on the other-avatar pie's
`More >`. The radar grew a sortable **Cost** column showing each avatar's ARC,
dimmed for an avatar the viewer is refusing to draw in full — "who is making
this region unusable" is the question the number exists to answer.

Default: **off** (`RenderAvatarMaxComplexity` 0), as the reference ships it.
The limit hides people, so it is opt-in.

## Divergences

- **Presence and animation survive.** The reference stops a jellydoll's
  animations and forces a stand, worth it there because its animation system
  runs on the CPU per avatar. Ours poses on the GPU, so a jellied avatar keeps
  moving like a person for free, and keeps its name tag, radar row and minimap
  dot.
- **Transparency** is judged from the face's tint alpha rather than from whether
  the face landed in the alpha draw pool, so an opaque tint over a texture that
  merely carries an alpha channel is not charged the ×4.
- **Attachment surface area** is the plain square-metre area of each prim's
  scaled bounding box, not the reference's unit-volume surface area × largest
  scale axis. Ours is the more literal answer to "how much screen can this
  smear over" — and it catches the single enormous alpha sheet the reference's
  measure lets through (a 64 m sheet scores 384 there, under its own 1000
  default).
- **An animated object's** streaming-cost term uses its finest level's triangle
  estimate rather than the reference's charged-versus-allowed refinement.
- **Animesh is exempt for free**, as with the friends-only filter: a control
  avatar is an ordinary mesh object here and never reaches the per-avatar
  decision, which is the reference's `!isControlAvatar()` by construction.
- **No impostor.** The reference renders a jellydoll through its impostor
  buffer; ours draws the real (cheap) base body flat. Billboard impostors are
  [[viewer-avatar-impostors-billboard]].

## Verification

Unit-tested: the body-region floor; the multipliers compounding to the
reference's product; a texture charged once per linkset however many faces use
it, with the reference's resolution term; a missing mesh header charged the
fallback and recorded as pending, then costing far more once the header lands;
the level estimator's backfill and metadata discount, and the radius weighting
being coarse-level-dominated for a small attachment and fine-level-dominated for
a huge one; the decision's full priority order (self, override, mode, budget,
area, and "unlimited means unlimited"); both friend modes short-circuiting the
budget; the stored mode numbering round-tripping so a Firestorm value ports
across. Plus the pie address table pinning the three new Render slices, and
`sl-prim`'s per-level counts (a box costs the same at every level, a round
profile more and rising, and the estimate stays within a small factor of the
real tessellation).

Live-verified against the local OpenSim with a second avatar, both round trips:
switching the mode to "Only draw friends fully" turns the non-friend into the
flat grey silhouette — their radar row and the own avatar untouched — and
switching back draws them fully shaded again; the pie's **More > Render >
Never** jellies them with reason `Override` and **> Fully** restores them. The
decision log names the reason and the score at every edge.

The first run found one bug, now fixed and pinned by a test. The jelly pass
skipped itself when it had nothing hidden *and* nobody jellied — but an avatar
wearing no attachments hides nothing, so on the very frame it stopped being a
jellydoll both sets were already empty and the pass never got to hand its body
back to the bake materials. It stayed flat grey for good. The early-out now also
asks whether any body is still wearing the silhouette
(`AvatarComplexityModel::has_jelly_work`).

Also learned, and worth knowing before reading a score on this grid: **both
avatars measure 0 on the local OpenSim**, correctly. The body cost counts
*published* baked textures, and neither avatar publishes any — the `sl-repl`
second avatar never bakes (which is also why it renders untextured), and our own
client-side composite is not echoed back to us. The score is therefore only
meaningful against a grid that bakes, so the magnitudes want an aditi run.

Live checks still to do: score magnitudes and the jellydoll of an
attachment-wearing avatar on aditi (including that hiding a *rigged* attachment
really takes its faces, which hang off the body root); a genuinely crowded
region — the frame-time win this feature exists for — and eyeballing a jellied
mesh-body avatar to confirm the forced base regions read as a silhouette rather
than a system body.

The own avatar's own number has no read-out yet: the radar lists nearby avatars,
not yourself, exactly as the reference radar does. Surfacing it is the first
bullet of [[viewer-name-tags-complexity-distance]] (the reference's
`FSTagShowOwnARW`), which this task unblocks.
