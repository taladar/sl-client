---
id: viewer-prim-linking
title: Prim linking & unlinking
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection-core, viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Link a selection set ([[viewer-object-selection-core]]) into a linkset,
unlink, and reorder; enforce link limits and permissions. The link / unlink
commands are driven from input **actions** ([[viewer-input-action-map]])
(`Ctrl+L` / `Ctrl+Shift+L` in the reference).

**Selection order matters.** The `ObjectLink` message's block order is the
link order, and the **last-selected** object becomes the linkset **root**
(the reference sends the selection `SEND_ONLY_ROOTS` with the roots in
selection order; the simulator makes the final block the parent). So "select
the parts, select the intended root last, link" is the muscle-memory
workflow every builder relies on — and link numbers (`llGetLinkNumber`,
which scripts depend on) are assigned from that same order, so the UI must
preserve the [`SelectionSet`]'s insertion order exactly as the clicks
happened (it already keeps the primary = last-selected; linking must not
re-sort it, e.g. into id order). Unlink keeps the parts selected so a
wrongly ordered link is immediately re-linkable the other way around.

The **Edit Linked Parts** mode named here already shipped as the build
floater's toggle ([[viewer-object-edit-floater-shell]]); this task is the
wire half (link / unlink / order) plus its Build-menu entries.

Reference (Firestorm, read-only): `llselectmgr` `sendLink` / `sendDelink`
(`ObjectLink`, `ObjectDelink`), `LLSelectMgr::sendListToRegions` ordering.

## Done

`src/edit_link.rs` (`EditLinkPlugin`). The wire side already existed
(`Command::LinkObjects` / `DelinkObjects` → `session.link_objects` /
`delink_objects`, `local_ids[0]` = root); this is the viewer half.

**Root ordering.** The reference packs its selected roots
most-recently-selected first (`LLObjectSelection::addNode` prepends;
`sendListToRegions` walks front-to-back) and both the Second Life simulator
and OpenSim (`HandleObjectLink` → `parentprimid = ObjectData[0]`) make the
**first** `ObjectLink` block the new root — so the last-selected object
becomes the linkset root. Our [`SelectionSet`] keeps the primary
(last-selected) **last**, so `link_order` is just the set **reversed**
(primary first); it never re-sorts, so re-clicking the intended root last is
enough to change which prim wins. Unit-tested.

**Unlink names every prim.** `unlink_selection` sends an `ObjectDelink` with
every prim of each selected linkset (`ObjectState::linkset_members`), matching
the reference's `SEND_INDIVIDUALS`; a root-only delink would leave OpenSim
re-linking the orphans into a fresh set (`SceneGraph::DelinkObjects`). The
selection is left in place so a wrongly ordered link is re-linkable at once.

**Enable gates** mirror `enableLinkObjects` / `enableUnlinkObjects`: link
needs whole-linkset (not edit-linked-parts) mode, ≥ 2 roots, and one
modifiable object; unlink needs one modifiable object (attachments never enter
the set, so no extra guard). Properties-less nodes count as modifiable
(optimistic; the reply lands within a frame or two, the simulator is the final
arbiter). The per-linkset prim **limit** (root + 255 children) is enforced at
link time only, exactly as the reference (`enableLinkObjects` omits it,
`linkObjects` checks it).

**Driving.** `Ctrl+L` / `Ctrl+Shift+L` follow the existing `Ctrl+B`
build-tools chord pattern (a direct keyboard handler, gated on edit mode + the
world owning the keyboard) rather than the movement/camera action map, which
resolves held single keys — chord command accelerators are not that axis. The
Build menu grew **Link** / **Unlink** entries (greyed via `enabled_when`
`can-link` / `can-unlink`); both the chords and the menu picks funnel through
the one `drive_link_unlink` system.
