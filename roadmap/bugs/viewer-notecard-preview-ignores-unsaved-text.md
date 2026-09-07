---
id: viewer-notecard-preview-ignores-unsaved-text
title: The notecard's View Items preview shows the loaded text, not what you typed
topic: viewer
status: bugs
origin: seen live-checking the keyed notecard editor on the local grid
  (2026-09-06)
refs: [viewer-notecard-editor, viewer-lsl-editor-widget]
---

Context: [context/viewer.md](../context/viewer.md).

An editable notecard offers a **View Items** toggle that swaps the plain edit
field for the rich read-only reader, so embedded items stay clickable until the
inline-box editor widget lands. The reader is built **once**, from the notecard
as it arrived (`populate_editor` → `spawn_reader_block`), and the toggle only
flips which of the two is displayed.

So the preview shows the notecard as **loaded or last saved** — never what the
resident has typed since. On a brand-new notecard that reads as an empty
preview beside a field with text in it, which looks like the preview is broken.

## What it should do

Rebuild the reader from the current edit buffer when the toggle switches to it
(reconciling the embedded-item markers the way a save does, via
`sl_notecard::Notecard::with_edited_text`, so an item dropped since the load
appears in the preview too). That is a small, self-contained change in
`edit_notecard.rs`, and it stays correct when
[[viewer-lsl-editor-widget]] eventually replaces the toggle with one editor
that draws items inline.

## How to verify

Open a modifiable notecard, type a line, press **View Items** without saving:
the preview must show the typed line. Drag an inventory item in, toggle again:
the item must appear as a clickable box in the preview.
