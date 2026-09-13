---
id: viewer-disabled-field-selection-flash
title: A disabled text field flashes a selection, and cannot be copied from
topic: viewer
status: done
origin: seen live-checking the item-properties price field on the local grid
  (2026-09-07)
points: 3
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

## What was done (2026-09-13)

Both halves, the suggested shape taken: a field now has **three** stances
rather than two, and one system enforces the two uneditable ones.

`ReadOnlyField` (`sl-viewer-ui-widgets/src/ui_text_input.rs`) is the new marker,
spawned by `TextInputSpec::read_only` or inserted and removed later like
`InteractionDisabled` itself. Greyed the same way (one
`reflect_uneditable_text_color` now paints both stances — what the user needs to
see at a glance is *you cannot type here*, and which stance it is shows itself
the moment they try to select), but it keeps focus, caret, selection and
`Ctrl+C`, and refuses only what would change the text.

The enforcement is **not** at the gesture. `bevy_ui_widgets`' press and drag
observers are global observers, so nothing this widget owns can pre-empt them:
a press on a disabled field will queue a `MoveToPoint` or a `SelectWordAtPoint`
whatever we do. It is the **queue** that is filtered instead —
`refuse_edits_an_uneditable_field_must_not_take` runs in `PostUpdate` after the
overwrite rewrite and before `EditableTextSystems`, dropping from each
uneditable field's `pending_edits` what it may not take. Each `TextEdit` is
classified by `edit_effect` into one of three:

- `Mutates` (insert, paste, cut, delete, IME commit, a non-empty preedit) —
  refused by both stances.
- `Reads` (caret moves, every selection, `Copy`) — refused by a disabled field,
  taken by a read-only one.
- `Clears` (`CollapseSelection`, an empty preedit) — always taken, or a
  selection made before the field was disabled would be stranded on it.

Every variant is named rather than swept up by a wildcard, so a new upstream
`TextEdit` has to be classified deliberately.

The flash dies because the selection never reaches the buffer, a frame *earlier*
than the old behaviour ended it: before, the edit applied, was painted, and was
undone only when the focus drop reached `on_focus_lost` the frame after.

The item-properties price field
(`sl-viewer-inventory/src/inventory_properties.rs`) is read-only rather than
disabled while the item is not for sale: the number is still the price the item
last carried, and the owner may well want to copy it.
The commit path is unchanged — it already applies a typed price only to an item
that is for sale.

Also added: a `text-input-read-only` gallery specimen (so the read-only look is
swept across every script, direction, scale and font size), its contract row
with a `CLICK_TAKES_THE_CARET_READ_ONLY` probe (the one reaction that separates
the two greyed stances — a disabled field refuses the caret, a read-only one
takes and keeps it), and read-only / disabled rows on the `F8` demo panel for
the by-hand check.

## How it was verified

- `ui_text_input::tests::an_uneditable_field_takes_only_the_edits_it_may` —
  the classification table itself, over every `TextEdit` variant.
- `ui_text_input::typed_tests::a_disabled_field_never_flashes_a_selection` —
  the bug, under real pointer events. The flash lasted a single frame, so the
  test samples after **every** frame the double-click is made of; sampling once
  at the end passes on the broken build, which is how the first version of this
  test was found to be worthless. Fails on the old behaviour at the first
  press, passes now.
- `ui_text_input::typed_tests::a_read_only_field_selects_and_copies_but_never_changes`
  — a double-click's selection survives, the field keeps focus, typing /
  backspace / delete / `Ctrl+V` change nothing and leave the selection standing,
  and `Ctrl+A` `Ctrl+C` puts the text on the clipboard. Fails on the old
  behaviour (the paste lands).
- `inventory_properties` `the_price_field_is_dead_until_the_item_is_for_sale`,
  updated: the not-for-sale item's price is read-only, the for-sale one's live.
- The whole `sl-client-bevy-viewer` lib suite (316 tests), including the
  registry sweeps and the contract sweep that the new element and its probe
  join.

Not machine-checked: the *look* of the greyed read-only specimen in the gallery
(the grey is the constant disabled fields already use).
