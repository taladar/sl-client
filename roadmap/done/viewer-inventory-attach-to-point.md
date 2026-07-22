---
id: viewer-inventory-attach-to-point
title: "Attach To ▸ / Attach To HUD ▸ attachment-point submenus"
topic: viewer
status: done
origin: split from viewer-inventory-context-actions /
  viewer-inventory-worn-actions (2026-07-21) — shipped greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions, viewer-attachment-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory item context menu's **"Attach To ▸"** and **"Attach To HUD ▸"**
submenus: attach an object to a **chosen** attachment point (or HUD point)
instead of the default slot, and "Add" vs replace per point. The plain
Wear / Add / Detach entries are live (`inventory_actions.rs`); these two
entries sit in their reference places gated on `UNIMPLEMENTED`.

The wire side exists: `Command::RezAttachment` takes an `AttachmentPoint` and
an `AttachmentMode` — this task is the submenu listing the named points
(`menu_inventory.xml`'s `Attach To` / `Attach To HUD` branches, populated from
the attachment-point table) and passing the choice through.

Reference (Firestorm, read-only): `llinventorybridge.cpp`
(`populateAttachmentMenu` / the `Attach To` branches),
`llviewerjointattachment`.
