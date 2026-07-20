---
id: viewer-ui-search-field
title: Reusable search-field widget (single-line field + clear button)
topic: viewer
status: ready
origin: follow-up requested during viewer-ui-text-input-widget (2026-07)
blocked_by: [viewer-ui-text-input-widget]
refs: [viewer-ui-menu-search, viewer-ui-text-input-emoji]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the menu bar's search box ([[viewer-ui-menu-search]]) into its own
reusable **search-field** widget, built on the single-line
[[viewer-ui-text-input-widget]]. Today `crate::menu_search` hand-rolls the whole
thing — a bordered container around an `EditableText`, a circular `×` clear
button revealed only while the field holds a term, `Escape`-to-clear, and a
placeholder — and every other search surface (inventory search, people / groups
search, the map and directory finders) will want exactly the same affordance.
Writing it once is the point of the widget layer.

`spawn_search_field` should compose the single-line field
(`TextInputKind::Line`) with:

- a **clear affordance** — the `×` button shown only while the field is
  non-empty (its visibility driven off the field's value, as `menu_search`'s
  `toggle_clear_button` already does), placed on the **trailing** edge so it
  mirrors under RTL for free (convention 1);
- **`Escape` clears** the term when there is one (and is a no-op otherwise, so a
  stray `Escape` elsewhere still bubbles);
- optional **placeholder** text shown while empty (a general gap in
  `EditableText`, so likely a sibling `Text` toggled on empty, the way the clear
  button is toggled — worth factoring here since every search box needs it);
- a **leading search glyph** (🔍) as the reference viewers show, optional.

Keep it constructible without wiring ([[viewer-ui-widget-scaffold]]): the field
owns its text and exposes the term via `EditableText::value` /
`Changed<EditableText>`; the consumer (the menu's own filter, an inventory
model) reacts to that, and nothing here reaches a session. Register a specimen
in `crate::ui_element::ELEMENTS` so the harness sweeps it, and **migrate
`crate::menu_search` onto it** so the extraction is proven by a real consumer
rather than left speculative.

Reference (Firestorm, read-only): `llsearcheditor`, `llfiltereditor` (the
search / filter line editors with their clear button).
