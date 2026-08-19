---
id: viewer-pie-wire-ready-placeholders
title: Wire the pie placeholder slices whose features already exist
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-object-context-menu, viewer-avatar-context-menu,
  viewer-attachment-context-menu, viewer-land-context-menu,
  viewer-world-pie-menu-reactions, viewer-remove-all-attachments]
---

Context: [context/viewer.md](../context/viewer.md).

Several pie-menu slices still sit on the UNIMPLEMENTED sentinel although
the feature behind each of them landed long ago; every one is the
module-documented "one `when` edit, address unchanged" plus a small
dispatch arm. The parity audit found these ready-to-wire slices: object
and land pie **Create** (`build` in `object_menu.rs` / `land_menu.rs`) →
open the Build Tools floater in Create-tool mode (`edit_create.rs`,
[[viewer-prim-creation]] done); avatar-self **Groups** (`groups` in
`avatar_menu.rs`) → the People panel Groups tab (`people.rs`,
[[viewer-social-groups]] done); avatar-self / attachment-self Appearance
▸ **Edit Shape** (`edit-shape`) → the wearable editor on the worn shape
(`edit_wearable.rs`, [[viewer-appearance-editor-bodyparts]] done);
avatar-self **Take Off ▸ Clothes ▸** — the seven declared layer slices
(`takeoff-*`) → the existing take-off path (`inventory_actions.rs`
`take_off_set`, [[viewer-inventory-worn-actions]] done); the
avatar-self **Detach All** (`detach-all`) → detach-all, which exists
at the API level ([[idiomatic-p3-03]]) and is the same surface
[[viewer-remove-all-attachments]] wires from the menu bar;
attachment-self **Show in Inventory** (`show-in-inventory`) → the
show-in-main action ([[viewer-inventory-show-in-main-from-worn-recent]]
done); avatar-other / attachment-other **Zoom In** (`zoom-in`) → the
third-person focus-on-target camera (`camera.rs` `FocusTarget::Point`,
[[viewer-camera-focus-on-object]] done).

All addresses stay put per the pinned per-menu address tables; the
committed `*_keeps_every_address` tests' enable assertions must be
updated in the same commits ([[viewer-world-pie-menu-reactions]] pins
the dispatch tables).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_pie_object.xml`,
`menu_pie_avatar_self.xml`, `menu_pie_avatar_other.xml`,
`menu_pie_attachment_self.xml`, `menu_pie_land.xml`;
`indra/newview/llviewermenu.cpp`.
