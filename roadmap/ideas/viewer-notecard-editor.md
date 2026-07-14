---
id: viewer-notecard-editor
title: Notecard viewer & editor (rich text with embedded items)
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-framework, viewer-inventory-ui, viewer-notecard-format]
---

Context: [context/viewer.md](../context/viewer.md).

Open, read, edit and save notecards. Easy to mistake for "a text box", which is
why it is worth stating plainly: **a notecard is not plain text.** The asset is
Linden text carrying **embedded inventory items** — drop a landmark, an object
or another notecard into the body and it sits *inline* in the text as a
clickable item — and the viewer linkifies URLs and SLURLs in the prose around
them. Notecards are how SL ships instructions, landmark packs and freebies, so
this is a load-bearing reader, not a nicety.

## What exists, and the two gaps

Present: `AssetType::Notecard`, the `UpdateNotecardAgentInventory` cap, asset
fetch and the create/update flow (there is a conformance case,
`test-notecard-create-update`).

Missing:

- **The format itself is never parsed** — split out as
  [[viewer-notecard-format]] (a pure `sl-notecard` crate), because a format
  decoder has no business living in a widget.
- **`UpdateNotecardTaskInventory`** — we have the *agent-inventory* cap but not
  the one for a notecard living **inside a prim**, which is exactly where most
  read-me notecards are. Add it alongside the existing script-task cap.

## The editor: the same widget problem, one step harder

The script editor ([[viewer-lsl-script-editor]]) already has to solve
"Bevy 0.19's editable text is `parley::PlainEditor`, so it takes **one style for
the whole buffer** and cannot colour a range". A notecard needs that *and*
**non-text objects inline in the flow** (the item icons).

The good news is that parley already models this: it has **inline boxes**
precisely for embedding arbitrary boxes in a text layout. So the fork that gives
the script editor per-range brushes is the same fork that gives the notecard
editor inline items — **one rich-text widget, two consumers.** Do not grow a
second editor; do make sure whoever builds the first one knows the second is
coming, because "per-range colour" and "inline boxes plus per-range colour" are
different designs.

Also needed: dropping an inventory item into the body (drag-and-drop from
[[viewer-inventory-ui]]), clicking an embedded item to open/wear/save it,
clickable URLs and SLURLs in the text ([[viewer-url-linkification]]), and the
usual save/permissions path. Note the embedded items carry their own
permissions — copying a notecard copies its contents, so the item-permission
rules matter and should not be quietly ignored.

Reference (Firestorm, read-only): `llpreviewnotecard`, `llviewertexteditor`
(the embedded-item machinery — items are represented as private-use characters
in the text and resolved through an embedded-item table), `llfloaternotecard`.

Deps: [[viewer-ui-framework]] (the floater and the text widget),
[[viewer-inventory-ui]] (drag-drop and opening an embedded item).
