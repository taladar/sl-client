---
id: viewer-inventory-context-actions
title: Inventory context actions + drag-and-drop
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-inventory-ui
blocked_by: [viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

The mutating actions on an inventory item: the context menu — **wear**, **rez**,
**give**, **delete**, **rename** — plus **drag-and-drop** (move items between
folders, drop onto an object or the world). Hangs off the folder tree
([[viewer-inventory-folder-tree]]) rows and wires each action to the existing
inventory / rez commands.

The inventory protocol and model already exist; this task is the context menu,
the drag-and-drop plumbing, and the command wiring.

Reference (Firestorm, read-only): `llinventorybridge`,
`llinventoryfunctions`.

Builds on: [[viewer-inventory-folder-tree]] and the `inventory.rs` model.
