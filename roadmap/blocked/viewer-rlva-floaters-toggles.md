---
id: viewer-rlva-floaters-toggles
title: "RLVa UI: console, restrictions/strings/locks floaters + toggles"
topic: viewer
status: blocked
origin: main-menu survey (2026-07-23)
blocked_by: [viewer-rlv-restriction-state]
refs: [viewer-rlv-command-parser, viewer-rlv-notify, viewer-rlv-queries]
---

Context: [context/viewer.md](../context/viewer.md).

The `viewer-rlv-*` tasks build the RLV *engine* (parser, enforcement,
queries, state). This task is the user-facing RLVa control surface —
Firestorm's whole RLVa top-level menu:

- **Console…** (`rlv_console`): type RLV commands at the viewer, see
  responses — the debugging/authoring tool.
- **Restrictions…** (`rlv_behaviours`): live list of active restrictions
  grouped by source object, reading the restriction registry
  ([[viewer-rlv-restriction-state]]).
- **Strings…** (`rlv_strings`): the editable response-strings table
  (customise the canned texts RLVa emits).
- **Locks…** (`rlv_locks`): inspector for attachment/wearable locks.
- Behaviour toggles: Allow OOC Chat, Show Filtered Chat, Show Redirected
  Chat Typing, Split Long Redirected Chat, Allow Temporary Attachments,
  Forbid Give to #RLV, Wear Replaces Unlocked, and the Debug submenu.

Scope: the four floaters, the RLVa top-level menu with its toggles
(settings-backed, consumed by the enforcement layers), all gated on RLV
being enabled.

Reference (Firestorm, read-only): `menu_viewer.xml` RLVa menu
(~L2835-3053), `rlvfloater*` sources, `RestrainedLove*`/`RLVa*`
settings.

Builds on: the restriction-state registry (its data model powers the
Restrictions floater); the toggles thread into the enforcement tasks.
