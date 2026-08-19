---
id: viewer-object-wear-attach
title: Wear / Add / Attach To an in-world object from the object menu
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-object-context-menu, api-g1,
  viewer-object-menu-reorder-when-implemented,
  viewer-inventory-attach-to-point]
---

Context: [context/viewer.md](../context/viewer.md).

The reference object menu can attach a picked in-world object directly
to the avatar: **Wear** (`Object.AttachToAvatar` — take and attach,
replacing whatever sits at the default/last point), **Add**
(`Object.AttachAddToAvatar` — attach without replacing), and the
paired **Attach To ▸** / **Attach To HUD ▸** submenus, which the viewer
fills at runtime from the attachment-point list, marking points that are
already occupied. Our object pie in
`sl-client-bevy-viewer/src/object_menu.rs` keeps `wear` as an
UNIMPLEMENTED placeholder, and its ATTACH_PIE pins only the static Bento
"Ext. Skeleton" tree while the plain and HUD point lists stay empty.

The wire side already exists: `ObjectAttach` and the attachment flows
landed with [[api-g1]], and the rez+attach paths were exercised in the
missing-out batches. Scope: dispatch Wear/Add on the picked linkset
root; fill the two attach-point sub-pies at open time from `sl-avatar`'s
attachment-point table (marking filled points like the reference does);
and honour the reference's `Object.EnableWear` gates — the target must
not already be an attachment, and the agent needs copy/transfer
permission as applicable. Pie addresses stay put per the pinned tables;
the runtime-list mechanics land here rather than waiting for the
deferred re-lay in [[viewer-object-menu-reorder-when-implemented]]. The
per-point list UI can share structure with the inventory-side tables
from [[viewer-inventory-attach-to-point]].

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_object.xml`,
`menu_pie_object.xml`; `indra/newview/llviewermenu.cpp`
(`Object.AttachToAvatar`, `Object.EnableWear`),
`indra/newview/llagentwearables.cpp`.
