---
id: viewer-notecard-preview-ignores-unsaved-text
title: The notecard's View Items preview shows the loaded text, not what you typed
topic: viewer
status: done
origin: seen live-checking the keyed notecard editor on the local grid
  (2026-09-06)
refs: [viewer-notecard-editor, viewer-lsl-editor-widget]
---

Context: [context/viewer.md](../context/viewer.md).

An editable notecard offers a **View Items** toggle that swaps the plain edit
field for the rich read-only reader, so embedded items stay clickable until the
inline-box editor widget lands. The reader was built **once**, from the notecard
as it arrived (`populate_editor` → `spawn_reader_block`), and the toggle only
flipped which of the two is displayed.

So the preview showed the notecard as **loaded or last saved** — never what the
resident had typed since. On a brand-new notecard that read as an empty preview
beside a field with text in it, which looks like the preview is broken.

## Fix

A new per-window pass, `refresh_notecard_preview`, brings a **shown** preview up
to the edit buffer: it reconciles the buffer against the window's baseline with
[`sl_notecard::Notecard::with_edited_text`] — the same call the Save button
makes — and rebuilds the reader's body from the result. So an item deleted from
the text is gone from the preview, a duplicated marker shows twice, and an item
dropped in since the load appears as its own clickable box.

The toggle carries the **edit-buffer text the reader was last built from**
(`NotecardViewToggle::shown_text`), and the rebuild is paid for a *change*, not
for a flip: a flip with nothing typed since compares two strings and does
nothing, keeping the reader's scroll and the build-once/update-on-change rule
the floaters are held to. A preview nobody is looking at is not built at all,
and because the pass is chained after the drop ingest, a drop onto a *shown*
preview lands in it the same frame rather than waiting for a flip.

The flip also **drops focus** from the field it hides, so keystrokes aimed at an
invisible editor cannot go on editing behind the preview (and cannot rebuild the
preview per keystroke).

Five regression tests in `edit_notecard.rs` (`tests::preview`) drive the real
path — open, asset arrives, type, press the toggle: the typed text shows, the
loaded text does not, an empty notecard previews what was typed into it, a
dropped item appears as a box, an unchanged flip reuses the same body entity,
and the hidden field gives up focus. With the refresh disabled the three
content tests fail, so they pin the defect rather than the code.

## How to verify

Open a modifiable notecard, type a line, press **View Items** without saving:
the preview must show the typed line. Drag an inventory item in, toggle again:
the item must appear as a clickable box in the preview.
