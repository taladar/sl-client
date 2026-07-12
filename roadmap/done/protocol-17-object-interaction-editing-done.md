---
id: protocol-17
title: Object interaction & editing (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**17. Object interaction & editing (done) ✅ — `ObjectGrab`/`ObjectGrabUpdate`/
`ObjectDeGrab` (touch/click), `ObjectAdd` (rez), `ObjectDuplicate`,
`ObjectDelete`/`DeRezObject`, `MultipleObjectUpdate` (move/scale/rotate),
`ObjectName`/`ObjectDescription`/`ObjectFlagUpdate`, plus the single-field edit
messages · 8 pts.** Turns the read-only scene (#16) into an editable one — a
builder/rezzer or object mover, building on #16's object cache (an object is
named by its region-local id). Implemented across the full editing surface:
**touch/click** — `touch_object` (an `ObjectGrab` + immediate `ObjectDeGrab`,
which fires a script's `touch_start`/`touch_end` and the `CLICK_ACTION_*`
behaviours) plus the press-drag-release primitives `grab_object`,
`grab_object_update`, `degrab_object`; **rez** — `rez_object(&PrimShape, …)`
(`ObjectAdd`), with a `PrimShape::cube` constructor carrying the viewer's
default new-prim path/profile quantization (the prim is rezzed exactly at its
position via `BypassRaycast`); **copy/delete** — `duplicate_objects`
(`ObjectDuplicate`), `delete_objects` (`ObjectDelete`, to trash), and
`derez_objects` (`DeRezObject`
with a `DeRezDestination` — take/return/trash/attach/…); **transform** —
`update_object(&ObjectTransform)` (`MultipleObjectUpdate`) plus the convenience
`set_object_position`/`set_object_rotation`/`set_object_scale`, which hand-pack
the variable `Data` blob in the simulator's fixed position→rotation→scale order
(the rotation via LL's `packToVector3` three-float quaternion) and OR the
`Type` bits (position `0x01`, rotation `0x02`, scale `0x04`, link-set `0x08`,
uniform `0x10`); **metadata** — `set_object_name`/`set_object_description`,
`set_object_click_action` (a `ClickAction` enum), `set_object_material` (a
`Material` enum), `set_object_flags` (`ObjectFlagUpdate`: physics/temporary/
phantom), `set_object_group`, `set_object_permissions` (a `PermissionField` mask
selector with set/clear), `set_object_for_sale` (a `SaleType` enum),
`set_object_category`, `set_object_include_in_search`; and **linking** —
`link_objects` (root id first) / `delink_objects`. New value types `PrimShape`,
`ObjectTransform`, `ObjectFlagSettings`, `ClickAction`, `Material`, `SaleType`,
`DeRezDestination`, `PermissionField`. All wired as `Command`/`SlCommand`
variants through both runtimes. Covered by ten `sl-proto` encoding tests (the
`ObjectAdd` cube fields, the `MultipleObjectUpdate` position+rotation `Data`
packing and the scale/uniform/group `Type` byte, touch grab+degrab, name,
delete, derez, permissions, link order, and the single-field setters).
*Live-verified against local OpenSim as the estate owner (the
`rez_edit_object` tokio example): `rez_object` created a cube, which streamed
back as an `ObjectAdded`; `set_object_name` + `set_object_for_sale` were
confirmed by the `ObjectProperties` round-trip (name, sale type Copy, price);
`update_object` moved it +5 m (confirmed by the follow-up `ObjectUpdate`); and a
`DeRezObject` to the Trash folder removed it (`ObjectRemoved`). The
touch/grab/material/click-action/flags/permissions/link ops are unit-tested
only (they need a scripted object or a second observer to see live). **Note:**
`ObjectDelete` is the viewer's god/force-delete path and stock OpenSim has no
handler for it ("Unhandled packet … Ignoring"); the portable delete-to-trash is
`derez_objects` with `DeRezDestination::Trash` and the agent's trash folder id.
Most edit ops need object ownership or build rights, which the sim silently
enforces.*
