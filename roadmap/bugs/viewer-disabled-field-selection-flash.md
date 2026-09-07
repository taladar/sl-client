---
id: viewer-disabled-field-selection-flash
title: A disabled text field flashes a selection, and cannot be copied from
topic: viewer
status: bugs
origin: seen live-checking the item-properties price field on the local grid
  (2026-09-07)
refs: [viewer-ui-text-input-widget, viewer-inventory-open-and-properties]
---

Context: [context/viewer.md](../context/viewer.md).

Double-clicking a **disabled** text field (the item-properties price while the
item is not for sale) shows a selection for a frame or two and then discards
it. Two things are wrong.

## The flash

`crate::ui_text_input` greys a disabled field
(`reflect_disabled_text_color`) and separately **drops input focus** from it
(`drop_focus_from_disabled_field`). The press still reaches the widget and
selects, and the focus drop lands a frame later — so the user sees a selection
appear and vanish. Whatever the answer to the second half below, the field
should never accept a gesture it is about to undo: refuse it at the press, or
keep what it grants.

## Copying from a disabled field

A disabled field's text is still worth **reading**, and often worth copying —
an id, a price, a name the user wants to paste elsewhere. Selecting for copy is
not editing, so a disabled field could keep selection, caret movement and
`Ctrl+C` while refusing every mutation (typing, paste, delete, drag-drop).

That is a change of stance rather than a bug fix, so it wants a deliberate
decision:

- The **reference** disables its line editors outright — a greyed
  `LLLineEditor` takes no focus and cannot be selected — so a faithful viewer
  would keep the current behaviour and only kill the flash.
- The **surrounding platform** (and most modern UI toolkits) do let a
  read-only control be selected and copied, and the user asked for it here.

Suggested shape if it is taken: a `TextInputSpec`-level distinction between
**disabled** (inert, greyed, as now) and **read-only** (greyed, selectable,
copyable, never mutated), with the item-properties price using read-only.
Bevy's `EditableText` would need its mutation paths gated on that flag rather
than on focus.

## How to verify

Double-click a disabled field: no selection may appear at all (flash fixed), or
— if the read-only stance is taken — a selection must appear, survive, and
`Ctrl+C` must put the text on the clipboard while typing changes nothing.
