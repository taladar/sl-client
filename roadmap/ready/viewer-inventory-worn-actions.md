---
id: viewer-inventory-worn-actions
title: Worn-item detach / take-off actions
topic: viewer
status: ready
origin: split from viewer-inventory-outfit-tab (2026-07) — the tab views shipped,
  the mutating actions did not
---

Context: [context/viewer.md](../context/viewer.md).

The **detach / take-off** actions on the Worn tab
([[viewer-inventory-outfit-tab]], done): remove a worn wearable or detach an
attachment straight from a worn-item row. The tab already lists the current
outfit (COF contents, with an `AgentWearables` fallback); this task is the
mutating actions on those rows.

The wire side exists on `Session` (`set_wearing`, `remove_attachment` /
`detach_attachment_into_inventory`); this task wires a worn-row affordance to
them. Naturally folds into the inventory context menu
([[viewer-inventory-context-actions]]) if that lands first.

Reference (Firestorm, read-only): `llappearancemgr`, `llwearableitemslist`.

## Operation set (2026-07-18)

- **Item wearing:** **wear** (replace), **add** (wear alongside), **detach /
  take off**.
- **Folder / outfit:** **add to current outfit** and **remove from current
  outfit** on a folder (or an outfit folder).

All map to `Session` appearance / attachment methods (`set_wearing`,
`rez_attachment`, `remove_attachment`, COF link add / remove); this task is the
row affordances and command wiring.

## Attach points (2026-07-18)

**Attach** is not one action: an object attaches to a **specific attachment
point** (or **HUD attachment point**) — "Attach To ▸" / "Attach To HUD ▸"
submenus in the reference viewer — and "Add" attaches without detaching what is
already on that point. Wire the point choice through to the attach command.
