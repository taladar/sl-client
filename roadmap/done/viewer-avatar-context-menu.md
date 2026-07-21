---
id: viewer-avatar-context-menu
title: Avatar context / pie menu entries (self + others)
topic: viewer
status: done
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-radial-menu]
refs: [viewer-ui-context-menu, viewer-object-context-menu, viewer-social-profiles, viewer-avatar-radar]
---

Context: [context/viewer.md](../context/viewer.md).

The **entries** offered when an avatar is the pick target (right-click an
avatar, its name tag, or a radar / people-list row), and the dispatch of each.
The two entry trees **differ**:

- **Own avatar:** Stand up / Sit, Appearance / outfit, My Profile, Groups /
  Friends, Gestures, Take off / detach, … (self-directed actions).
- **Another avatar:** Profile ([[viewer-social-profiles]]), IM / Call, Add
  friend, Pay, **Share** (give an inventory item — the wire path is done), Block
  / mute, Report, and the moderation actions where permitted.

Rendered by either the radial ([[viewer-ui-radial-menu]]) or the line
([[viewer-ui-context-menu]]) widget, mirroring [[viewer-object-context-menu]];
this task is the entry tree and its dispatch, reading the pick / selected
avatar.

Reference (Firestorm, read-only): `menu_attachment_self.xml` /
`menu_attachment_other.xml`, `menu_avatar_self.xml` / `menu_avatar_other.xml`.

## Done (2026-07-21)

Landed as `src/avatar_menu.rs` (the two entry trees + dispatch) plus a reusable
avatar-pick identity in `src/avatars.rs`. Opened as a **pie** (the line
presentation is deferred to when a domain wants it).

**The two trees.** `AVATAR_SELF_PIE` and `AVATAR_OTHER_PIE`, one per pick target
exactly as the reference (`menu_pie_avatar_self.xml` / `_other.xml`). Which one
opens is chosen at pick time by comparing the picked agent to
`SlIdentity::agent_id`. Both pin their whole address table in a committed
regression test (`{self,other}_avatar_pie_keeps_every_address`), per the pie
doctrine — moving an entry is now a loud diff.

**Wired vs. placeholder.** Most reference actions are features this viewer does
not have yet (profile, pay, report, outfit editing, the moderation powers);
those sit **in their reference compass positions but disabled**, gated on the
never-supplied `UNIMPLEMENTED` sentinel, so the menu shape (the muscle memory)
is laid down now and each slice lights up when its feature lands (one `when`
edit, address unchanged). Wired for real: **IM** (→ `OpenConversation`),
**Stand Up / Sit Down** (→ `Command::Stand` / `SitOnGround`, each enabled only
in the state it applies to via `SELF_SITTING` / `SELF_STANDING`), **Mute** (→
`Command::Mute`), **Add as Friend** (→ `Command::OfferFriendship`, disabled when
already a friend via `TARGET_NOT_FRIEND`).

**Reusable avatar identity (`AvatarPickTarget { agent }`).** Placed on every
pickable piece of an avatar — the placeholder sphere, each rigged body part, the
floating name tag, and the pick collider below — so a pick resolves to an agent
through one component regardless of what it hit. Picking works two ways, as the
reference does (name tag *or* the avatar itself): the name tag through the UI
hover map, the body through an on-demand `MeshRayCast` (no mesh-picking backend
is installed, matching `hud_pick`/`ground`). This same component is what a
future **inventory drag-and-drop onto an avatar** reads to find its drop target,
which is why the identity lives on the entities.

**Body picking needs a collider — `MeshRayCast` can't hit the skinned mesh.** A
skinned mesh is ray-tested against its *bind* pose (a T-pose at the origin), not
where it is drawn, so the body is unhittable directly. So each rigged body
carries an invisible box collider (`fit_avatar_pick_colliders` in
`src/avatars.rs`) sized from the **posed skeleton** every frame: height from the
joints' vertical span (shape- *and* pose-adaptive), width/depth fixed at the
reference's `DEFAULT_AGENT_WIDTH` / `DEFAULT_AGENT_DEPTH` so it hugs the torso
rather than the arm span. Two subtleties paid for in review: `MeshRayCast`'s
query reads `Aabb` non-optionally and `calculate_bounds` skips
`NoFrustumCulling` meshes, so the collider must *omit* `NoFrustumCulling` (the
body parts keep it, and are correctly never picked); and the collider is a child
of the body root, whose transform carries the whole SL→Bevy basis change, so its
subtree is **Second Life space** (Z up, X forward, Y left) — the fit measures
the joints' `z` for height, not `y`. The box is a coarse stand-in, not
silhouette-accurate → mesh-accurate picking is
[[viewer-avatar-mesh-accurate-pick]].

**Occlusion order: UI, then HUD attachments, then world** (the reference's
order). `pointer_over_blocking_ui` (shared with `hud_pick`) was also fixed to
only count a hover entry as occluding if it is a real UI node with positive area
— a phantom zero-area hover entry had been reporting the whole world as blocked,
which silently broke *both* this pick and left-click touch. `pointer_over_hud`
(factored out of `hud_pick`) casts the HUD ray so a worn HUD occludes the avatar
behind it.

**Opens on a right-*click*, not the press.** This viewer binds a right-**drag**
to camera free-look, so the menu opens on the right-button release of a click
(negligible travel, `RIGHT_CLICK_DRAG_SLOP`) — a look-drag never pops a menu.

**Debug aids (kept, split).** A cursor-following pick inspector
(`update_pick_inspector`, behind `SL_VIEWER_DEBUG_PICK`) shows live what a
pick would hit (UI/HUD verdicts, nearest world hit, resolved avatar pick);
separately, `SL_VIEWER_DEBUG_PICK_BOX` draws the pick collider as a
translucent box so its fit can be eyeballed. Both off by default.

### Deliberate departures from the reference

- **Eight slots, not nine.** The reference self pie overflows to nine top-level
  slices; ours is a hard eight. The ninth (`Textures`, a debug dump) is folded
  into the `Appearance >` sub-pie, so all eight compass positions still match.
- **No `More >`.** `crate::pie_menu` rules out nameless overflow by
  construction. Where the reference's *first* level is itself `More >` (the
  other-avatar south slice) the slice is kept in place and filled from that
  overflow's own first level; the deep debug tails are not reproduced (see
  below).

### Deferred (not reproduced as dead slices)

The reference's deep enumerations are runtime lists or pure debug and were left
for later rather than declared as dozens of disabled leaves: the per-attachment-
point `Detach >` / `HUD >` lists (self), the nested clothing overflow
(undershirt/underpants/tattoo/physics/alpha/all-clothes), the impostor `Display`
modes, `Derender`, and the debug `Textures`/`Dump XML`/`Reset` tails of the
other-avatar `More >`. The attachment (worn-object) pies
(`menu_pie_attachment_*`) are a separate target and are not this task. Also
deferred: the line-menu presentation of these same entries, and opening the pie
from a radar / People-list row (only the world name tag / body triggers it
today).

Follow-ups filed: [[viewer-avatar-menu-reorder-when-implemented]] (re-lay both
pies by meaning once most actions exist) and
[[viewer-avatar-mesh-accurate-pick]] (silhouette-accurate picking to replace the
box). Still worth filing: wire `Groups` (self) to the Conversations groups tab;
the attachment context menu ([[viewer-hud-context-menu]] covers HUDs).
