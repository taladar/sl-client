---
id: viewer-prim-linking
title: Prim linking & unlinking
topic: viewer
status: ready
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
