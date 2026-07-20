---
id: viewer-ui-search-field
title: Reusable search-field widget (single-line field + clear button)
topic: viewer
status: done
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

## Done (2026-07-20)

`sl-client-bevy-viewer/src/ui_search.rs` —
`spawn_search_field(commands, parent, &SearchFieldSpec) -> SearchFieldHandle`
composes the single-line [[viewer-ui-text-input-widget]] field (spawned **bare**
and **filling**, via two new `TextInputSpec` knobs `decorated` / `fill`) inside
a bordered box, with an optional leading 🔍 glyph, a trailing `×` clear button
shown only while the field holds a term, an optional placeholder shown while
empty, and `Escape`-to-clear on the focused field. Direction-neutral by
construction (a `crate::ui::row`, so glyph and clear button mirror ends under
RTL).

`SearchFieldPlugin` owns the runtime (clear/placeholder visibility, escape); the
skin classes are the generic `sk-search-field` / `sk-search-clear` (renamed from
the menu-specific ones). A specimen is registered in `ELEMENTS` and swept by
`crate::ui_test`; behaviour (clear + placeholder toggling, escape) is
unit-tested.

`crate::menu_search` is migrated onto it — the proving consumer — keeping only
the menu-specific parts (mirroring the term into `MenuFilter`, and swallowing
the field's own press so clicking it does not dismiss the menu it filtered). Net
a deletion: the bespoke box, clear button and toggle systems are gone.

**One deliberate scope note.** The placeholder is an *absolute* overlay, which
taffy folds into the slot's `content_size` in a way the content-overflow harness
reads as an overflow even though the box is sized correctly and the overlay is
clipped — so the placeholder is left **off the swept specimen** (its behaviour
is covered by unit tests and the two live consumers) while the glyph and clear
button are swept. `Escape`-clear is now scoped to the **focused** field (correct
for more than one search box on screen), a slight change from the menu's old
clear-when-unfocused.
