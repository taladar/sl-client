---
id: viewer-inventory-open-and-properties
title: Item Open (per-type preview) + Properties floater
topic: viewer
status: done
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

Shipped 2026-07-22 (inventory_properties module):

- **Properties** — the Vintage skin's legacy single-window floater
  (llfloaterproperties): editable name / description (Enter commits),
  creator / owner (names resolved via the avatar name cache, requested
  when missing), acquired date (UTC, civil-calendar formatter), the
  read-only "You can" mask, live group-share / everyone-copy /
  next-owner M-C-T toggles and the For Sale block (type cycle +
  price), all written back via UpdateInventoryItem.
- **Open** — per-type previews behind a new `can-open` gate: notecard
  reader (FetchAsset + sl-notecard decode), texture / snapshot preview
  (shared texture pipeline into a UI image), About Landmark (new tiny
  landmark-asset parser; region id + position + Teleport), animation
  preview (Play in world / Stop). Sound stays un-openable (no audio
  backend yet — viewer-audio-backend), gesture preview and Play
  Locally likewise deferred; Show on Map stays greyed pending the
  world map task.
- **Copy Asset UUID** — live via the system clipboard, gated
  `can-copy-uuid` (full owner permissions).
