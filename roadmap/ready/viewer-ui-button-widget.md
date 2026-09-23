---
id: viewer-ui-button-widget
title: The button is the one control that never became a widget
topic: viewer
status: ready
origin: relief-theme live look (2026-09-23)
points: 8
refs: [viewer-skin-image-backed-widgets, viewer-skin-widget-state-classes,
  viewer-audit-checkbox-box-widget]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets/` has sixteen widget modules — checkbox, radio, combo,
slider, tab, table, text input, search, colour picker, trackball, menu,
floater, rich text. There is no `ui_button.rs`. What stands in for one is
`sl-viewer-ui-core/src/ui_spawn.rs::spawn_button`, a helper that builds a node
from a `ButtonSpec` — and the skin class is an **`Option` that defaults to
`None`**:

```text
if let Some(class) = spec.class {
    button.insert(ClassList::new_with_classes([class]));
}
```

So a button is skinnable only when its caller remembers to ask, and almost
none do.

## What that costs, measured

- **35 of 37** `ButtonSpec::bordered` / `ButtonSpec::flat` call sites never
  chain `.class(...)`. Those buttons carry no `.sk-*` class at all: they are
  painted by the inline `BackgroundColor` / `BorderColor` the spec sets, and
  **no stylesheet can reach them**.
- **~72** further spawn tuples hand-roll a bordered, filled box with no class
  and no `Button` component — a button in everything but name. The
  debug-settings specimen is one:

  ```text
  commands.spawn((
      Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
             border: UiRect::all(Val::Px(2.0)), ..default() },
      BorderColor::all(CONTROL_BORDER),
      BackgroundColor(Color::srgb(0.16, 0.19, 0.25)),
      ChildOf(buttons),
  ))
  ```

  (An upper bound — the scan also catches panels, swatches and field boxes,
  which are not buttons. The scan is `BorderColor` + `BackgroundColor` in one
  spawn tuple with no `ClassList`.)
- The few that *are* skinned each re-declare `const BUTTON_CLASS: &str =
  "sk-button"` locally. That string is duplicated in at least eight files
  (`group_notice`, `contact_sets_panel`, `offers_invites`, `telehub`,
  `top_objects`, `about_region`, `about_land`, `snapshot_floater`), with no
  shared constant in `skin.rs` — which has `ACTION_BUTTON_CLASS` but no
  `BUTTON_CLASS`.

## How it was found

The `relief` theme dresses `.sk-button` in nine-sliced art. On the live look
the floater chips and dialog buttons changed and **panel, preferences and
debug-settings buttons did not** — they have no class for the theme to match.
A skin that cannot reach most of the viewer's buttons is the real reason
[[viewer-skin-image-backed-widgets]] looks half-applied, and no test caught it
because every skin test spawns its own `.sk-button` node rather than asking
what the viewer actually spawns.

## The work

1. A real `ui_button` widget in `sl-viewer-ui-widgets`, on the
   `ui_checkbox` / `ui_radio` model: it owns the class, the `Button`
   component, the tab index, the disabled state (`InteractionDisabled`, and the
   greyed *caption* rather than a greyed box — only `label_class` gives the
   label a `ClassList`) and the
   label class. `.class()` becomes an **override**, not the only route to
   being skinned.
2. `BUTTON_CLASS` as one `pub const` in `skin.rs` beside `ACTION_BUTTON_CLASS`,
   and the eight local copies deleted.
3. Migrate the `ButtonSpec` call sites, then the hand-rolled boxes that are
   really buttons. `ButtonSpec::bordered` maps to `.sk-button`;
   `ButtonSpec::flat` to `.sk-action-button` (which deliberately carries only
   the refused state, so those keep their resting look — see the comment on
   `ACTION_BUTTON_CLASS`).
4. A contract/gallery test that asserts **the viewer's own buttons** are
   skinned, not a synthetic node: spawn a registry element and assert every
   `Button` under it carries a `.sk-*` class. That is the test whose absence
   let this sit.

## Watch out

- Adopting `.sk-button` changes the box: the inline bordered spec is
  `padding: 8/3` with a 1 px border (9 x 4 of inset), `common.css` is
  `padding: 5px 10px` with a 2 px border (12 x 7). Every migrated button grows
  by 6 px in each direction, so the `ui_test` overflow sweeps are the gate.
- 67 call sites chain `.colors(...)` / `.label_color(...)`. Once the class is
  on, CSS wins and those inline colours become only the pre-load fallback.
  Check each for a *semantic* colour (a destructive action, say) before
  flattening it to the token palette.
