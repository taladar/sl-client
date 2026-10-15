---
id: viewer-build-general-sale-clickaction
title: General tab — for-sale, click action, show in search, locked
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-object-menu-custom-verbs,
       viewer-edit-permission-gating, viewer-object-mark-copy-key,
       gridspec-object-edit]
---

Context: [context/viewer.md](../context/viewer.md).

The build floater General tab's commerce/behaviour block, all absent
from `sl-viewer-edit/src/edit_params.rs`: the **For Sale**
checkbox with the sale-type combo (Copy / Contents / Original) and the
L$ price field; the **click-action** combo (Touch / Sit / Buy / Pay /
Open / Zoom / Ignore / None); the **Show in search** checkbox; and the
Object-tab **Locked** checkbox (the owner move-mask flag — no "locked"
handling exists in edit_params / edit_tool / gizmos today).

The wire paths already exist unused in sl-proto:
`Command::SetObjectForSale` (`sl-proto/src/command.rs:1462`),
`Command::SetObjectClickAction` (`command.rs:1547`),
`Command::SetObjectIncludeInSearch` (`command.rs:1637`), and the
permission-mask update covers Locked. So this task is UI plus wiring.
Greying under insufficient permissions belongs to
[[viewer-edit-permission-gating]]; the pie-menu side of custom click
verbs is [[viewer-object-menu-custom-verbs]].

Body extras: the FS **Copy Keys** button (`btnCopyKeys` — copy the
selection's object UUIDs; the pie-menu/marking side is
[[viewer-object-mark-copy-key]]) and the **export permission** checkbox
(`checkbox allow export`, OpenSim/FS grids only, low priority — no
export perm exists anywhere in edit_params).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L1185-1310,
1550), `indra/newview/llpanelpermissions.cpp`.

Measured ([[gridspec-object-edit]], `book/src/gridspec/building.md`
§ Editing): Second Life answers a click action and the search flag with a
full `ObjectUpdate`; **OpenSim stores each and tells nobody**, so the
controls must show what was set without waiting for an echo. "Locked" is
the owner mask's move bit, which an owner may clear on Second Life and
which OpenSim leaves alone whatever is sent — on OpenSim the checkbox has
nothing to write.
