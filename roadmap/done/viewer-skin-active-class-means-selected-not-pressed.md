---
id: viewer-skin-active-class-means-selected-not-pressed
title: .sk-active means "selected", which is not what CSS's :active means, and nothing but relief draws a press
topic: viewer
status: done
origin: the user comparing the reference's bottom bar while viewer-skin-glyphs-from-content was in progress (2026-09-24)
refs: [viewer-skin-widget-state-classes, viewer-skin-image-backed-widgets,
  viewer-vintage-skin, viewer-skin-glyphs-from-content]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

## Observation

The reference's bottom-bar buttons change their look **while the mouse is
held down on them**. In CSS that state is `:active`. Ours do not change at
all in the two flat skins.

## Two defects, one name

**The name.** Our skin vocabulary uses `.sk-active` (`ACTIVE_CLASS` in
`sl-viewer-ui-core/src/skin.rs`) for *toggled on / selected*: a toolbar button
whose floater is open, the active tab of a strip, the selected row of a
table. That is CSS's `:checked`, or ARIA's `aria-pressed` /
`aria-selected`. It is not `:active`, which CSS defines as "being activated
by the user", i.e. held down. A skin author who knows CSS reads `.sk-active`
as the pressed state and writes the wrong rule. This has already cost this
codebase once: the active-group marker in `groups.rs` could not use
`.sk-active` for "this is the active group", because `.sk-active` also
paints a selection background, so it needed a separate `sk-current` class
(`glyph::CURRENT`).

About 103 uses across 28 files (Rust and CSS) as of 2026-09-24.

**The missing state.** `bevy_flair` does support `:active`. It maps to
`Interaction::Pressed` and bevy_ui's `Pressed` marker. The only rule using it
is the `relief` theme's `.sk-button:active` nine-slice. `common.css` has no
pressed look for any widget, so under `graphite` and `azure` a press is
invisible on every button, and the bottom bar's buttons in particular. The
reference draws it as a texture of its own (`image_pressed` /
`image_pressed_selected` in `widgets/button.xml`). The Vintage skin needs
it: its pressed and toggled buttons wear a warm gold frame.

## What to do

- Rename `.sk-active` / `ACTIVE_CLASS` (and its text half,
  `.sk-active-text` / `ACTIVE_TEXT_CLASS`) to a name that cannot be read as
  the pointer state. `.sk-selected` fits rows and tabs; a toggle button is
  "on" rather than "selected", so check whether one name serves both or
  whether a toolbar toggle wants `:checked` (a `Checked` marker) instead.
  Update both shipped skins, the themes, the test stylesheets, the book's
  skin chapter token table and the vocabulary test at the same time.
- Give every button family a `:active` rule and a token pair
  (`--control-bg-pressed` / `--control-border-pressed`, or per family where
  the reference distinguishes them): push buttons, flat action buttons, the
  toolbar and bottom-bar buttons, the floater chrome buttons, tab-scroll and
  scrollbar arrows. Include pressed-and-selected, the reference's
  `image_pressed_selected`.
- Check that every one of those widgets actually gets `Pressed` or
  `Interaction::Pressed` synced. A `ButtonKind::Plain` box (no button
  component) has neither, so `:active` can never match it.

## Done when

No class in the skin vocabulary shares a name with a CSS pseudo-class that
means something else, a press is visible on every button in both flat skins,
and a scratch skin can give the bottom bar's buttons a distinct pressed look
by CSS alone.

## Resolution (2026-09-24)

**The name split in four**, by what each use actually meant:

| Was | Now | For |
| --- | --- | --- |
| `.sk-active` on a row, tile, marker | `.sk-selected` (`SELECTED_CLASS`) | the selected item of a collection — a class, because rows are recycled and selection follows the data index |
| `.sk-active` on a toggle button | `bevy_ui::Checked` → `:checked` (`skin::set_button_on`) | the bottom toolbar's open-floater buttons, the day-cycle track being edited, the Photo Tools time preset — the same engine state a tab or a tick box carries |
| `.sk-active` / `-text` on a floater title | `.sk-frontmost` / `.sk-frontmost-text` | the front-most floater (`:focus` would be the same trap: it need not hold focus) |
| `.sk-active-text` on a label | `.sk-accent-text` (`ACCENT_TEXT_CLASS`) | accent text that was never a selection: the active group, the People sort arrow, a profile's group links |

The toolbar's greyed placeholders moved from `.sk-disabled-surface` to
`InteractionDisabled` → `:disabled`, so its three states are all engine state
and its label follows them as descendant rules.
`no_class_is_named_after_a_pseudo_class` guards the vocabulary.

**The press.** `PRESS_CLASSES` + `stamp_press_state` (on `stamp_hover_state`'s
model) give every button box with neither button component a `PressTracked`
marker, and four global pointer observers keep `bevy_ui::Pressed` on it while
the primary button holds it (not on a refused one; release, drag end and cancel
let it up). `every_press_rule_has_something_to_press` holds the list to exactly
the classes `common.css` writes `:active` for. Every family has a rule and a
token: `.sk-button` (face darkens, bevel turns inside out; `:disabled` restates
the resting surface after it, because `bevy_ui`'s legacy `Interaction` presses a
refused button too), `.sk-action-button`, `.sk-toolbar-button` (plus
`:checked:active` → `--control-bg-pressed-selected`), `.sk-floater-button`,
`.sk-scrollbar-arrow`, `.sk-tab-scroll-button`.

**Found on the way:** `.sk-action-button` had state rules and no resting rule,
so a re-enabled action button stayed grey and a Photo Tools time button stayed
lit (bevy_flair reverts nothing). It now rests on `--action-button-bg` /
`--action-button-border`, which moves the Photo Tools and parcel audio buttons
(which wear that class over a darker hand-painted fill) onto the flat action
buttons' face.

Cascade tests (`skin_palette_resolves`): every family goes down and comes back
up in both flat skins; the toolbar button walks rest → pressed → lit → lit and
pressed → off → greyed on one entity; and a scratch sheet
(`tests/assets/pressed-toolbar.css`) restyles the toolbar press with one rule.

Visually confirmed by the user in the live viewer on the local grid.
