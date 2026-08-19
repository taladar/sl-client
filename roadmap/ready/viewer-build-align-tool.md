---
id: viewer-build-align-tool
title: Object align tool (QToolAlign) — bbox align / pack
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-object-edit-floater-shell, viewer-transform-gizmos,
       viewer-object-selection-core, viewer-attachment-align,
       viewer-avatar-alignment-tools]
---

Context: [context/viewer.md](../context/viewer.md).

Qarl's align tool, shipped by Firestorm as the build floater's sixth
edit radio ("Align", `radio align` in `floater_tools.xml`). With
several objects selected it computes the selection's axis-aligned
bounding box, renders six axis manipulator handles on the selection
bound, and one click aligns all selected objects' bounding boxes flush
along the picked axis/direction; dragging further packs them together
(and distributes), committing the moves as object updates.

We have nothing like it: the edit-floater shell, gizmos, and selection
core exist, and planar *texture* align exists
(`sl-client-bevy-viewer/src/edit_texture_align.rs`), but there is no
object-align tool. Implementation needs the selection set plus a
MultipleObjectUpdate batch move. Note this is distinct from
[[viewer-attachment-align]] (aligning worn attachments) and
[[viewer-avatar-alignment-tools]] (rotating the own avatar) — this one
is an object-editing tool inside the build floater's tool row.

Reference (Firestorm, read-only): `indra/newview/qtoolalign.cpp`,
`indra/newview/qtoolalign.h`,
`indra/newview/skins/default/xui/en/floater_tools.xml` (radio align,
~L267).
