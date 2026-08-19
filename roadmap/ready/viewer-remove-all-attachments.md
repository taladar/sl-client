---
id: viewer-remove-all-attachments
title: Detach All + per-attachment-point detach submenus
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-inventory-worn-actions, viewer-attachment-context-menu,
  viewer-inventory-attach-to-point]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's Avatar ▸ Take off ▸ Detach All
(`Self.RemoveAllAttachments`, menu_viewer.xml L302–319) detaches every
worn attachment at once, and the dynamic "Detach" / "HUD" submenus
(menu names "Avatar Detach" / "Avatar Detach HUD", populated at
runtime in `llviewermenu.cpp`) list each occupied attachment point for
one-click detach of whatever is worn there.

We can detach a specific worn object (`attachment_menu.rs` "detach" →
Command::DetachObjects, [[viewer-attachment-context-menu]] done; the
inventory worn-item actions, [[viewer-inventory-worn-actions]] done)
and attach to a chosen point ([[viewer-inventory-attach-to-point]]
done), but have no detach-all action and no per-point listing (grep
for RemoveAllAttachments / detach_all comes up empty). Scope: a
detach-all command batching Command::DetachObjects over the worn set,
with the reference's enable gate (only enabled while something is
worn), plus per-attachment-point submenus built from the live
attachment map — shared between the top menu's Take off ▸ Detach /
HUD submenus and the avatar self pie menu.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_viewer.xml` L302–319,
`indra/newview/llviewermenu.cpp` (handle_detach_all, the
"Avatar Detach" / "Avatar Detach HUD" dynamic menu population).
