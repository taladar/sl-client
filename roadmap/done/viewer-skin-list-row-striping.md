---
id: viewer-skin-list-row-striping
title: Scroll-list rows — striping, hover and a selection the skin owns
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-table-widget, viewer-ui-virtualized-list, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Every scroll list in the reference viewer has four row states, and names a
colour for each one:

| State | Reference colour | Vintage |
| --- | --- | --- |
| ordinary row | `ScrollBgWriteableColor` | `#c8cfcc` |
| every other row | `ScrollBGStripeColor` | `#b8bfbb` |
| hovered row | `ScrollHoveredColor` | `#bec3c3` |
| selected row | `ScrollSelectedBGColor` | `#8d90c2` |
| disabled row | `ScrollDisabledColor` (text) | `#00000040` |

Ours has one. `ui_table.rs` `apply_table_selection_highlight` writes
`SELECTED_ROW_BACKGROUND` (a hardcoded `srgba(0.24, 0.34, 0.52, 0.55)`) for a
selected row and `Color::NONE` for every other, and `virtual_list.rs` adds
nothing. There is no striping and no hover row anywhere in the viewer.

Striping is not decoration on a list a hundred rows long — it is how the eye
keeps a row's cells together across a wide table, which is most of what an
inventory list, a radar and a region-top-objects list are.

## What to do

- Add the four row roles to the token vocabulary
  (`--list-row-bg`, `--list-row-stripe`, `--list-row-hover`,
  `--list-row-selected-bg`, `--list-row-selected-text`), alongside `--list-bg`
  from [[viewer-skin-light-surface-roles]].
- Give `virtual_list` / `ui_table` rows an even/odd distinction the cascade can
  see. The rows are recycled as the list scrolls, so parity must follow the
  **data index**, not the slot — a stripe that walks up the list as you scroll
  is worse than no stripe. `virtual_list.rs` already keeps `VirtualRow.index`
  for exactly this kind of question; note the module's own warning that
  `first + slot` is the mapping that does *not* work.
- Add the hover state on the same mechanism as the rest of
  [[viewer-skin-widget-state-classes]], rather than a fifth per-frame paint.
- Keep the `TableSelectionMode::None` escape hatch: a consumer that owns its
  row backgrounds (the transcript bands, for one) must keep them.

## Done when

A list with selection shows stripes that stay put while scrolling, a hover
row, and a selected row whose text colour the skin chose; the flat skins look
as they do now; and a test asserts stripe parity is by data index across a
scroll.

## Done (2026-09-23)

Five tokens, one class, one system, and four rules in an order that is the
whole mechanism.

**The stripe is stamped from the item, not the row.** `stripe_virtual_rows`
lives in `virtual_list.rs`, so *every* virtualised list gets it — the tables,
the inventory tree, an object's contents, the RLVa panes — and it reads
`VirtualRow.index`, the answer to the modular slot↔item mapping. The engine
does parse `:nth-child(even)`, which is why it is worth saying why it is not
used: a recycled row's position among its siblings is its **pool slot**, so a
`:nth-child` stripe would stay put while the items moved under it and the bands
would crawl as the list scrolled. Even indices carry the stripe, matching
`LLScrollListCtrl::draw`'s `mDrawStripes && (line % 2 == 0)`.

It scans every pooled row every frame rather than watching
`Changed<VirtualRow>`, and that is not laziness: a consumer inserts its
`ClassList` from a `Commands` in its own `Added<VirtualRow>` pass, which lands
**after** the bind that would have been the only notification — a row that
missed it would stay unstriped for its whole life. `set_state_class` guards the
write, so a settled list costs a compare per row.

**The order is the feature.** Stripe, hover and selection are one class or
pseudo-class each compounded with the row's own — (0,2,0) every time — so
specificity separates none of them and the file's order decides which paints a
row that is all three. They sit in `common.css`'s state block in that order,
after the resting rule that is now `var(--list-row-bg)`.

**`:hover` needed a component no row had, and the cascade test could not see
it.** `bevy_picking`'s `Hovered` is **opt-in** — its own docs say "typically, a
simple hoverable entity or widget will have this component added to it", and
nothing adds it for you — and `bevy_flair` reads exactly two things: `Hovered`,
and the legacy `Interaction` that `bevy_ui`'s `Button` requires. So the rule
parsed, resolved and painted **never**, in the live viewer, while
`a_rows_states_resolve_stripe_then_hover_then_selection` passed — because a
headless test inserts `Hovered(true)` itself, and so proves the rule while
staying blind to whether anything supplies the state. The first live look found
it in seconds; nothing else would have.

**And it was never only the rows.** Asked whether this explained another hover
bug, a sweep of every `:hover` rule in `common.css` found **three** dead ones,
not one: the rows, `.sk-tile` (the emoji grid's cells) and `.sk-combo-option` (a
drop-down's rows). The last two are the sharp ones —
[[viewer-skin-panel-state-classes]]
*replaced* working `Pointer<Over>` / `Pointer<Out>` observer pairs with those
rules, so a combo's drop-down and the emoji grid stopped highlighting under the
pointer the day they were "converted", and every test stayed green. The four
that did work — `.sk-button`, `.sk-inline-item`, the texture picker's rows, the
inventory gallery's tiles — all had a `bevy_ui::Button` on them, which is the
only reason anyone ever saw a hover paint.

`skin::stamp_hover_state` fixes all of it on `stamp_focus_ring_class`'s model:
keyed off the classes, in one place, so a widget built next year gets its hover
by carrying the class — which is the whole claim the class makes. It skips
anything already carrying `Interaction`, because that drives the same
pseudo-state and two systems writing one state is a race to nowhere. Three
class constants moved into `skin.rs` (`TABLE_ROW_CLASS`, `INLINE_ITEM_CLASS`,
`COMBO_OPTION_CLASS`), for a reason worth keeping: a crate-local copy of one of
those strings is a class the stamp does not know about.

**`every_hover_rule_has_something_to_hover`** is the guard, and it is the shape
to copy: it reads `common.css` (comments stripped — the file explains every rule
it has, including two it no longer carries), collects the class of every
`:hover` selector, and asserts that set is *exactly* the list the stamp walks.
Both directions — a rule with no stamp paints nothing, a stamp with no rule is
the list rotting into a catch-everything.

**The rule itself is a plain `:hover` and no state system**, which is what
`viewer-skin-widget-state-classes` asks for whenever a selector can reach the
paint — a row's hover really is the pointer's, unlike a menu row's lit state.
It covers a table row and a hand-rolled one alike, which **retired
`.sk-picker-row`**: the texture picker's second class existed only because it
was the one list in the viewer that lit a row under the pointer.

It is deliberately **not** gated on whether the list can be selected, though
the reference gates its own on `mCanSelect`: our `TableSelectionMode::None`
means "the widget does not own this table's selection", not "this table is
inert" — the group members list is `None` and answers a click through its own
observer. The escape hatch the task asked for is kept structurally instead:
every one of these rules is written **compound with a row class**, so a pooled
row that is not a list's row is untouched, and a consumer painting its own row
background was already losing to `.sk-list-row`'s resting rule before this.

**`--list-row-stripe` is transparent in both flat skins, and that is the
reference's own answer**, not a hedge: the default skin's `ScrollBGStripeColor`
is `Transparent` and it is the light classic skins that band their lists. So
nothing in a shipped skin moved except the hover, which every list gains and
which takes the chrome hover a menu row and a combo option already use.

`--list-row-selected-bg` splits the row selection away from `--selection-bg`
(the reference names `ScrollSelectedBGColor` separately) and
`--list-row-selected-text` is `ScrollSelectedFGColor`, the one text role a
selected row moves off the field family for. A **button** in a selected row
keeps the chrome caption, restated at (0,4,0) — the same exception
`.sk-list-surface .sk-button .sk-text` already carried — and a **greyed** cell
in a selected row stays greyed, restated last, because "a greyed row must never
also read as lit" is the state block's own rule.

### Two more defects the live look found

**A selected row does not show the hover, and should not.** Reported as a
surprise, kept as it is: `LLScrollListItem::draw` fills with `select_color` for
a selected item and reaches `hover_color` only in the `else` branch — the two
are mutually exclusive in the reference, and the cascade order here says the
same thing.

**The combo drop-down had lost its background**, and the hover is what made it
visible. `viewer-skin-light-surface-roles` gave `.sk-combo-list` `--list-bg` —
which is a **scrim**, 25% black over whatever panel a list is sitting in — and a
drop-down *floats*, so there was no panel under it and the chrome and the world
read through the options. Nobody could see it until a row under the pointer
gave the eye something opaque to compare against. `--combo-list-bg` is its own
role now, opaque in both skins, which is what the reference does in every skin
it ships (`ComboListBgColor` is `DkGray` in the default, a flat 0.9 grey in
Vintage) and why it names the colour separately from `ScrollBgWriteableColor`
in the first place.

**A state-only rule sticks, which is the second half of the same trap.** With
the emoji cells finally hoverable, the wash never came *off* them: `bevy_flair`
does not revert a property when a rule stops matching, it applies the winning
rule's value and nothing else, so `.sk-tile` — whose **only** rule was the
`:hover` — painted on the way in and had nothing to paint on the way out. A
dense grid makes that obvious within seconds and no test here could see it. The
class has a resting `background-color: transparent` now, and
`every_hover_rule_has_a_resting_rule` asserts every class with a `:hover` rule
has a bare rule too. The engine warns about it in as many words —
`Cannot set property 'background-color' on 'emoji-picker-sample-cell' to None.
You should avoid this by setting a baseline style` — in a log line that scrolls
past, so a test rather than another comment.

**The drop-down was still translucent at 95%.** The first fix gave it the
floater body's own `#…f2`, which is fine for a floater (it has the world behind
it, not text) and not for a surface that lands on top of a panel's labels. Both
skins are fully opaque now and the test demands `alpha >= 1.0` rather than
"> 0.9", with the reason written where the next person will read it.

**The tone swatches answered a click but not the pointer.** Once the cells above
them lit, the strip below reading as dead was the obvious next question. A
swatch is one tile of a small grid and is clicked exactly as a cell is, so it
wears `.sk-tile` beside `.sk-tone-swatch` now and lights the same way; the
swatch class keeps only what is different about it, which is the frame and the
`:checked` ring.

**The emoji specimen could not be skinned at all.** Its cells carried no
`ClassList` and no `Pickable` — `spawn_emoji_picker_specimen` builds its own
static cells rather than going through `spawn_emoji_cell` — so `.sk-tile:hover`
had nothing to select, and the gallery card a skin author would check a dense
grid against was the one thing in the gallery that showed none of the skin.
Both are on it now. (The picker's *preview line* is a separate open bug,
[[viewer-emoji-picker-hover-preview-inert]], and a different mechanism: an
observer, not a pseudo-class. The fix here turns it into a one-look
experiment — see that file.)

### The checks

- `stripe_parity_follows_the_item_across_a_scroll` (in `virtual_list.rs`)
  scrolls by exactly one row — the scroll that re-binds exactly one slot — and
  asserts every row's stripe against its own index before and after, plus that
  the two differ, since otherwise the assertion would hold for a slot-derived
  stripe too.
- `a_rows_states_resolve_stripe_then_hover_then_selection` pins the precedence
  through the real engine under Graphite (`Hovered(true)` is what `:hover`
  reads), and the selected cell's text against an ordinary one's.
- `a_light_list_bands_hovers_and_selects` gives the four roles the measured
  Vintage values through the `light-field.css` fixture and reads them back —
  the only way to see a stripe at all, since no shipped skin bands.
- `every_hover_rule_has_something_to_hover` (in `skin.rs`) is the one that
  matters most, because it is the only one of these a cascade test could not
  have been.
- `a_floating_drop_down_is_not_a_scrim` asserts the property rather than a hex
  value: a drop-down's face is opaque, and is not the token an embedded list
  uses.

A gallery element (`list-row-states`) stacks an ordinary row, a striped one and
the selection over each on a list's face — the element a skin author checks a
row family against, on the checkbox specimen's model. The hover is the one
state it cannot lay out, because it is a real `:hover`: it shows by putting the
pointer on a row, which is what a live gallery is for.

The book's skin chapter gains the five tokens and the row classes. Its "some
widgets are deliberately absent, because their paint depends on state a
selector cannot see" paragraph was **stale** — it still named a tab button, a
radio indicator, a table row selection and a menu entry's greying, every one of
which moved into the cascade over the last four tasks, and the floater title
text with them. Only the pie menu's shader is left.

### Not done

- **The live look.** Nothing here is judged headlessly beyond the resolved
  colours; the one thing worth an eye is that a hovered row in a dozen panels
  that never had one reads as feedback rather than as noise.
- `ScrollDisabledColor`'s *background* (a disabled row's face, as against its
  text, which `.sk-list-surface .sk-disabled-text` already carries) has no
  role. No list in the viewer disables a single row today.
