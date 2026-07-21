---
id: viewer-inventory-open-and-properties
title: Item Open (per-type preview) + Properties floater
topic: viewer
status: ready
origin: split from viewer-inventory-context-actions (2026-07-21) — shipped
  greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions]
---

Context: [context/viewer.md](../context/viewer.md).

The context menu's **"Open"** and **"Properties"** entries (and the per-type
open variants the reference gives them):

- **Open** — a per-type preview floater: notecard reader, texture / snapshot
  preview, animation preview + play, sound play, landmark "About Landmark" /
  Show on Map, gesture preview. Each is its own small floater; the asset
  fetch paths all exist (`sl-client-bevy` asset managers).
- **Properties** — the item-properties floater: name / description editing,
  creator / owner, the permission checkboxes (next-owner modify / copy /
  transfer), sale settings — writing back via `UpdateInventoryItem`.
- The **Copy Asset UUID** entry (creator/full-perm gated) belongs here too.

Big enough to split per preview type when picked up; the menu entries are
already declared in their reference places gated on `UNIMPLEMENTED`
(`inventory_actions.rs`).

Reference (Firestorm, read-only): `llpreview*.cpp` (notecard / texture /
anim / sound), `llfloaterproperties.cpp`.
