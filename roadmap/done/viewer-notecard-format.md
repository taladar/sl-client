---
id: viewer-notecard-format
title: Notecard format — a pure crate (sl-notecard)
topic: viewer
status: done
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

A pure crate (`sl-notecard`: no Bevy, no I/O, plain `cargo test`) that decodes
and encodes the **Linden text** notecard asset — mirroring `sl-lsl`,
`sl-prim` and the other format crates. **Self-contained: its only dependency is
`sl-types`**, so it can be tested, fuzzed and reused (a CLI, an inventory tool,
a bulk exporter) with no session and no grid.

Today `sl-asset` treats a notecard as an **opaque blob**: `AssetType::Notecard`
exists, the asset fetches and caches fine, and the create/update flow has a
conformance case — but **nothing parses the body**, so the structure inside is
invisible.

And there is structure, which is the whole point: a notecard is *not* plain
text. The asset is a versioned Linden-text container carrying the text
**plus embedded inventory items** — the landmarks, objects and other notecards a
resident drops into the body, which the viewer renders inline as clickable
items. The text references them positionally, so the decoder must reproduce both
the prose and where each item sits in it, and the encoder must round-trip a
notecard it did not create without corrupting items it does not understand.

Scope:

- Decode and encode the container (version header, text body, the embedded-item
  table and the in-text references to it), preserving unknown or future fields
  rather than silently dropping them — this is somebody's inventory.
- Model an embedded item as the inventory item it is (id, asset id, type, name,
  **permissions** — copying a notecard copies its contents, so the permission
  bits are not decoration).
- Round-trip tests against real notecards from a live grid, since the format is
  defined by what the simulator accepts rather than by a specification.

Consumed by [[viewer-notecard-editor]] (which renders the text with inline items
and saves it back). Keep the two apart: one is a format, the other is a widget,
and only one of them needs a GPU.
