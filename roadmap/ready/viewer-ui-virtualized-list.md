---
id: viewer-ui-virtualized-list
title: Virtualized (windowed-recycling) list
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

A virtualized list with **windowed row recycling**. Bevy's `ListBox` spawns one
entity per row, so a 10k-item inventory ([[viewer-inventory-folder-tree]]) would
mean 10k taffy nodes; recycle a small window of row entities as the viewport
scrolls instead. This is DIY and is **the main technical unknown** of the UI
cluster — no prior art at that scale in Bevy — so it is isolated in its own
task.

Scope here is the **flat list**; the folder **tree** variant (inventory,
folder-view) is a later extension. Any long-list panel (radar, people list,
inventory, chat history at scale) consumes this.

Reference (Firestorm, read-only): `llfolderview`, `llscrolllistctrl`
(virtualized scrolling behaviour, not the layout model).
