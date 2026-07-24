---
id: viewer-texture-picker-inventory-tree
title: Texture picker — inventory folder tree navigation
topic: viewer
status: done
origin: user request (2026-07-24) while reviewing the build-tool texture picker
blocked_by: [viewer-ui-texture-picker]
refs: [viewer-inventory-open-and-properties]
---

Context: [context/viewer.md](../context/viewer.md).

The texture picker shipped with [[viewer-ui-texture-picker]] lists the
**already-loaded** inventory / library textures as a flat thumbnail grid with a
name search filter — a deliberate stopgap. It does **not** bulk-fetch the
inventory, because a real Second Life inventory is **300 000+ items**: fetching
every folder to populate a flat grid (as an earlier version tried) is
infeasible, and even iterating all loaded items per rebuild does not scale.

The reference's `LLFloaterTexturePicker` instead embeds a small **inventory
folder tree** (an `LLInventoryPanel` filtered to textures / snapshots) that
fetches a folder's contents only when it is **opened** — lazy, so it scales to
any inventory size. This task replaces the flat grid with that tree.

Add a folder-tree pane to the picker (reusing the Everything-tab tree model /
row rendering the main inventory floater already has, filtered to
texture / snapshot items), with the thumbnail grid showing the selected
folder's textures (or keep the flat grid as an alternate "all" view). The
search filter stays. Local-file textures and the bake channels remain their
own follow-ups.

Reference (Firestorm, read-only): `llfloatertexturepicker.cpp` (the embedded
`LLInventoryPanel` + folder view), `llinventorypanel.cpp`.
