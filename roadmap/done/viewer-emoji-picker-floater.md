---
id: viewer-emoji-picker-floater
title: Emoji picker floater
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-emoji-input
blocked_by: [viewer-emoji-data, viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

The full emoji-picker floater: a grouped, searchable grid of emoji (from
[[viewer-emoji-data]]) in a floater ([[viewer-ui-floater-basic]]); selecting one
inserts its glyph into the focused text field.

Reference (Firestorm, read-only): `llfloateremojipicker`.

## Done (2026-07-20)

`sl-client-bevy-viewer/src/emoji_picker.rs` — `EmojiPickerPlugin` stands up a
floater (toggled `Ctrl+E`) that is a pure **assembly of landed widgets**, no new
primitive: the reusable search field ([[viewer-ui-search-field]]) narrows the
grid, the reusable tab strip ([[viewer-ui-tab-widget]]) is a row of nine
group-icon tabs selecting the shown [`Group`], and the **virtualized list**
([[viewer-ui-virtualized-list]]) backs the grid as one pooled *row of
`GRID_COLUMNS` cells* per list row — so the whole ~1800-glyph dataset from
[[viewer-emoji-data]] (`sl-emoji`) costs the viewport, not the item count. A
blank search shows the active group; a non-blank one shows `sl_emoji::search`
across every group.

**How a chosen glyph reaches a field.** The picker owns no field to type into
(its search box only filters), so it remembers in `EmojiTarget` the last
`EditableText` **outside the picker** to hold focus, and a cell press inserts
the glyph there through the field's own parley driver
(`insert_or_replace_selection`) — at the caret, replacing any selection,
grapheme-/IME-correct, not a raw `set_text`. Focusing the search box, a group
tab or the grid never disturbs that remembered target, and a focus *clear*
leaves it in place.

**Skin tones.** A six-swatch row re-casts every tone-bearing glyph in the grid
(and the inserted one) to the chosen Fitzpatrick tone via
`Emoji::with_skin_tone`; a toneless glyph is left alone. One tone at a time, as
`sl-emoji` models it. A hover **preview line** names the hovered glyph and its
short-code.

Constructible without wiring: the novel layout (a grid-cell sample, the swatch
row, the preview line) is registered as the `emoji-picker` specimen swept by
`crate::ui_test`; the filtering, tone maths, row-count and the real parley
insert are unit-tested. Floater title localized via `emoji-picker-title`
(en/ja/ar/pl).

**Deliberately scoped out (their own tasks / follow-ups).** The field-side emoji
**button** that opens the picker anchored to a chat/IM field is
[[viewer-ui-text-input-emoji]] — until it lands, `Ctrl+E` is the opener and "the
last focused field" is the target. The inline `:shortcode:` completer is
[[viewer-emoji-colon-autocomplete]]. A *recently-used* section (the reference's
first tab) would need persistence and is left as a follow-up.
