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

## Progress (2026-09-21)

The per-frame `TextColor` writes are gone: the filter highlight and the
"this tab has no hit" dim in `preferences.rs`, the mute glyph in
`volume_panel.rs`, the enable-gated bar glyphs in `media_controls.rs`, the
teleport outcome title, and the four notification kinds. Two classes were
added for states that had no role — `MATCH_CLASS` and its counterpart
`NO_MATCH_CLASS` — and `ui_tab`'s `TAB_LABEL_CLASS` turned `pub` so a panel
that dims a caption spawns the same pair the widget does.

One case does **not** convert and is not meant to: `chat.rs` fades a line by
its *age*, and there is no selector for how old a node is. It stays Rust.

What is left is the constants, not the writes — the state-named `Color`
constants still read at spawn time. By crate:

- `sl-viewer-people` — the largest share: `radar.rs`'s
  `SELECTED_BACKGROUND`, the hand-rolled tab strips in `people.rs` and
  `conversations.rs` (`TAB_ACTIVE_*` / `TAB_INACTIVE_*`, part 3's
  widget-adoption question), `ONLINE_COLOR` / `OFFLINE_COLOR`,
  `ACTION_DISABLED_BACKGROUND` and `DISABLED_LABEL_COLOR` in both
  `people.rs` and `groups.rs`, `groups.rs`'s `ACTIVE_COLOR`.
- `sl-viewer-chat` — `emoji_picker.rs`'s `CELL_HOVER_BACKGROUND` (a
  `:hover`) and `SWATCH_BORDER_ACTIVE`, `local_chat_input.rs`'s
  `OPTION_ACTIVE_BACKGROUND`.
- `sl-viewer-preferences` — `phototools.rs`'s `BUTTON_ACTIVE_FILL` /
  `BUTTON_DISABLED_FILL`, `preferences.rs`'s `CHECK_DISABLED` (which waits
  on [[viewer-skin-checkbox-radio-shape]] — it is a hand-rolled checkbox).
- `sl-viewer-places` / `sl-viewer-environment` — the three copies of
  `DISABLED_COLOR` (`about_region.rs`, `about_land.rs`,
  `land_environment.rs`), all the same drifted grey.
- `sl-viewer-ui-widgets` — `ui_trackball.rs`'s `DISABLED_BORDER` /
  `DISABLED_MARKER` and `ui_color_picker.rs`'s `DISABLED_BORDER`, which are
  widgets the earlier pass did not reach.
- `sl-viewer-media` — `web_floater.rs`'s `BUTTON_LABEL_DIM`.

The `DIM_LABEL_COLOR` copies that a grep for state names also turns up are
*not* this task: a muted caption is a resting colour and belongs to
[[viewer-skin-panel-text-roles]].

## Done when

No panel writes a `BackgroundColor` / `TextColor` / `BorderColor` to express a
state, the 26 local aliases are gone, each drift has been decided rather than
inherited, and a skin switch recolours a selected inventory row, a greyed
About Land field and a hovered emoji cell the way it already recolours a
selected table row.
