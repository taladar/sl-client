---
id: viewer-skin-panel-state-classes
title: The panels paint their own states, and nine files hand-copied one role
topic: viewer
status: in-progress
origin: survey during viewer-skin-widget-state-classes (2026-09-21)
points: 5
refs: [viewer-skin-widget-state-classes, viewer-skin-panel-text-roles]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-skin-widget-state-classes]] moved widget state — hovered, selected,
toggled, disabled — out of per-frame Rust paint and into the cascade, for the
shared widget set: floaters, tabs, tables, menus, combos, radios, the pie
captions and the toolbar. It did not touch the ~40 **panels** built out of
those widgets, which paint their own states from their own constants.

Its sibling [[viewer-skin-panel-text-roles]] owns a panel's *text* colours.
This is the other half: the colours a panel writes to express a **state**.

## What the survey found (2026-09-21)

370 `Color` constants in the panel-side crates. By name, **54 express a
state** (active / selected / disabled / hover / online …); the remaining 316
are plain panel colour and belong to [[viewer-skin-panel-text-roles]].

**26 of the 54 are a class that already exists, under a local name:**

| Target | Count |
| --- | --- |
| `.sk-active` — a selected row, tile or marker | 14 |
| `.sk-disabled-text` | 9 |
| `:hover` | 2 |
| `.sk-disabled-surface` | 1 |

And they have **drifted**, the same way the twenty copies of the label colour
had before `viewer-audit-skin-token-coverage` collapsed them. The 14
selected-row backgrounds hold **five distinct values**:

- **nine agree** — and are exactly `SkinPalette::FALLBACK.selection_bg`,
  `srgba(0.24, 0.34, 0.52, 0.55)`. Nine files hand-copied the role's value
  rather than reading it. **Ten**, in fact: the survey anchored on
  `^const NAME: Color` and so missed the ten *indented* constants in the
  workspace, one of which is `sl-viewer-environment`'s
  `style::SELECTED_BACKGROUND` — a crate-local role module, which is the right
  instinct one layer short of the shared roles.
- `group_profile.rs` and `groups.rs` drifted to a different blue
  (`0.30, 0.42, 0.62`);
- `about_floater.rs` drifted in **alpha** as well (`0.35`, not `0.55`);
- `emoji_complete.rs` is **opaque**, so a banded list cannot read through its
  own highlight;
- `day_cycle_editor.rs`'s `MARKER_SELECTED` is **gold**, not blue at all.

The disabled text is tighter: 9 constants, 2 values, 8 of them exactly
`FALLBACK.text_disabled` and one a hair off.

Beyond the constants, **22 `TextColor` assignments across 18 files** actually
recolour panel text per state (44 functions *take* a mutable paint component,
but many only carry it — `about_land.rs`'s `set_value_node` destructures
`_color` and writes only the text). They are spread one or two per file rather
than clustered, so this is a wide, shallow sweep.

Those 22 are also what [[viewer-skin-panel-text-roles]] waits on: a class on
the spawn helpers would beat a Rust-painted `TextColor` and flatten a greyed
action column, a disabled check glyph or a faded toast into one colour —
silently, and only in the live viewer, since the headless harness excludes
styling.

## The work, in three parts

1. **The mechanical sweep.** 26 constants onto classes that already exist, in
   17 files across 9 crates. This deletes the copies *and* the drift, and is
   the bulk of the value. Best done solo: it is wide and shallow, and it
   collides with anything else editing those panels.
2. **Six drift decisions**, which are not swaps. `MARKER_SELECTED` is the
   interesting one — a selected marker on a day-cycle **gradient** is not a
   selected row in a list, and gold against a sky ramp may be right. Decide
   whether it takes `.sk-active`, `--match-highlight`, or a role of its own;
   the same question applies to `emoji_complete.rs`'s opaque highlight, which
   may be deliberate over a dense grid.
3. **~28 genuinely local states.** The one worth naming: the **People panel
   paints its own tab strip** (`TAB_ACTIVE_BACKGROUND` / `TAB_INACTIVE_*` /
   `TAB_ACTIVE_BORDER` in `conversations.rs`) rather than using the shared tab
   widget, which now takes its selected look from `:checked` for free. That is
   a widget-adoption question, not a colour one.

## What to reuse

The vocabulary is in place and needs no extension for parts 1 and 3:
`ACTIVE_CLASS` / `ACTIVE_TEXT_CLASS`, `DISABLED_SURFACE_CLASS` /
`DISABLED_TEXT_CLASS`, `HIGHLIGHTED_CLASS`, and `set_state_class` /
`set_state_class_on` (which guard the `ClassList` write, so an idle panel does
not wake the style engine every frame). Prefer a pseudo-class wherever the
panel already carries the component — `:hover`, `:checked`, `:disabled` — as
the widget conversions did; three of them needed no marker at all.

## Progress

### The per-frame writes (2026-09-21)

Gone: the filter highlight and the "this tab has no hit" dim in
`preferences.rs`, the mute glyph in `volume_panel.rs`, the enable-gated bar
glyphs in `media_controls.rs`, the teleport outcome title, and the four
notification kinds. Two classes were added for states that had no role —
`MATCH_CLASS` and its counterpart `NO_MATCH_CLASS` — and `ui_tab`'s
`TAB_LABEL_CLASS` turned `pub` so a panel that dims a caption spawns the same
pair the widget does.

One case does **not** convert and is not meant to: `chat.rs` fades a line by
its *age*, and there is no selector for how old a node is. It stays Rust.

### A regression this found (2026-09-22)

`set_action_button_enabled` stopped painting in the greying commit, but
`rows::spawn_action_button` was never given the `ButtonSpec::class` /
`::label_class` the commit message said these buttons pass — so the
environment windows' action buttons had **neither** end of the selector and a
refused one stopped greying at all. The day-cycle track buttons were worse:
with no `ClassList` on them, `.sk-active` could not be added either, so the
selected track stopped reading as lit.

Both are fixed, and `rows.rs` now has a unit test asserting the button
carries the class *and* its caption carries `.sk-text` — the assertion that
would have caught it, and the shape to copy wherever a paint is replaced by a
selector.

The helper moved with the fix: `ButtonPaint` / `set_action_button_enabled`
are now `sl_viewer_ui_core::skin`'s `DisabledButtons` /
`set_action_button_enabled`, because a disabled action button stopped being
one crate's idea the moment the Friends and Groups panes wanted it too.

### The constants (2026-09-22)

Converted: `.sk-action-button:disabled` for the Friends and Groups action
columns (and with it `FriendActionButton::label` / `ActionButton::label`,
which existed only to be repainted); `.sk-presence-online` /
`-offline` for the Friends presence dot, meaning-bearing like `--gain` /
`--loss`; `.sk-active-text` for the worn group's row; `.sk-tile:hover` for
the emoji grid, which retired a **fourth** hand-written hover-observer pair;
`:checked` for the tone swatches, which are a radio group; `.sk-list-row` +
`.sk-active` for the chat volume dropdown and the radar specimen;
`.sk-action-button` + `.sk-active` for phototools' time buttons;
`.sk-swatch:disabled` for the colour swatch, deleting its whole reflect
system; `.sk-trackball:disabled .sk-trackball-disc` for the trackball's rim;
`.sk-disabled-text` for the web floater's Back / Forward; and `.sk-active`
for the two environment windows that painted a table row's selection
themselves, which is what `style::SELECTED_BACKGROUND` — the tenth copy of
`selection_bg`, and the one the first survey named — existed for.

Still to do, from a survey that (unlike the first) also catches indented and
`pub(crate)` constants:

- `sl-viewer-pickers` — `ui_texture_picker.rs` is the densest one left:
  `SELECTED_FILL`, `ROW_HOVER` (another hover-observer pair) and
  `DISABLED_BORDER`.
- `sl-viewer-inventory` — `inventory.rs`'s `SELECTED_ROW_BACKGROUND` (a
  tenth copy of `selection_bg`) and `inventory_drag.rs`'s `DROP_HIGHLIGHT`,
  which is a state no pseudo-class reaches and wants a class of its own.
- `sl-client-bevy-viewer` — `about_floater.rs`'s `LICENSE_ROW_SELECTED`.
- `sl-viewer-audio` — `parcel_audio.rs`'s `BUTTON_FILL_DISABLED` /
  `BUTTON_BORDER_DISABLED`.
- `sl-viewer-people` — the hand-rolled tab strips in `people.rs` and
  `conversations.rs` (`TAB_ACTIVE_*` / `TAB_INACTIVE_*`). Part 3's
  widget-adoption question, deliberately not converted in place.
- `sl-viewer-ui-widgets` — `ui_trackball.rs`'s `DISABLED_MARKER` is the one
  case that resists a split: the marker's enabled colour is the *body's*
  (sun / moon) and the horizon swaps which of fill and rim carries it, so
  Rust and the cascade would be writing the same two properties. Converting
  it means tokenising the sun and moon colours, which is a design decision of
  its own.

Not this task, though a grep for state names turns them up: the three copies
of `DISABLED_COLOR` (`about_region.rs`, `about_land.rs`,
`land_environment.rs`) and `preferences.rs`'s `CHECK_DISABLED` are all
hand-rolled three-state check painters, which wait on
[[viewer-skin-checkbox-radio-shape]]; `about_land.rs` also has one value node
born grey, and the `DIM_LABEL_COLOR` copies are resting colours — both
[[viewer-skin-panel-text-roles]]'s.

## Done when

No panel writes a `BackgroundColor` / `TextColor` / `BorderColor` to express a
state, the 26 local aliases are gone, each drift has been decided rather than
inherited, and a skin switch recolours a selected inventory row, a greyed
About Land field and a hovered emoji cell the way it already recolours a
selected table row.
