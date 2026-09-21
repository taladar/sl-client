---
id: viewer-skin-panel-state-classes
title: The panels paint their own states, and nine files hand-copied one role
topic: viewer
status: ready
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
  rather than reading it.
- `group_profile.rs` and `groups.rs` drifted to a different blue
  (`0.30, 0.42, 0.62`);
- `about_floater.rs` drifted in **alpha** as well (`0.35`, not `0.55`);
- `emoji_complete.rs` is **opaque**, so a banded list cannot read through its
  own highlight;
- `day_cycle_editor.rs`'s `MARKER_SELECTED` is **gold**, not blue at all.

The disabled text is tighter: 9 constants, 2 values, 8 of them exactly
`FALLBACK.text_disabled` and one a hair off.

Beyond the constants, **44 distinct panel functions** take a `&mut
BackgroundColor` / `TextColor` / `BorderColor` — `sl-viewer-people` (9),
`-places` (7), `-chat` / `-notices` / `-preferences` (4 each), across 12
crates.

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

## Done when

No panel writes a `BackgroundColor` / `TextColor` / `BorderColor` to express a
state, the 26 local aliases are gone, each drift has been decided rather than
inherited, and a skin switch recolours a selected inventory row, a greyed
About Land field and a hovered emoji cell the way it already recolours a
selected table row.
