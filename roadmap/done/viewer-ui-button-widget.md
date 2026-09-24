---
id: viewer-ui-button-widget
title: The button is the one control that never became a widget
topic: viewer
status: done
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

The first two numbers below were **re-measured on starting the work and are
lower than this file first claimed** ("35 of 37", "~72"). Both original counts
came from a `rg -A` window too short to reach the `.class(…)` at the end of a
long builder chain, and from a spawn-tuple scan that could not tell a button
from a panel. The gap is real either way; it is half the size.

- **14 of the 30** `ButtonSpec::bordered` / `ButtonSpec::flat` call sites never
  chain `.class(...)`, so they carry no `.sk-*` class at all: they are painted
  by the inline `BackgroundColor` / `BorderColor` the spec sets, and **no
  stylesheet can reach them**. (`rg -o 'ButtonSpec::(bordered|flat)\('` counts
  35, of which five are `ui_spawn`'s own tests and one a doc link in `skin.rs`.)
- **40 further spawn tuples carry a button component and no `ClassList`** — a
  hand-rolled bordered, filled box that is a button in everything but the
  helper it went through. The About floater's action button is one:

  ```text
  commands.spawn((
      Button,
      TabIndex(tab_index),
      Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
             border: UiRect::all(Val::Px(2.0)), ..default() },
      BorderColor::all(CONTROL_BORDER),
      BackgroundColor(BUTTON_BACKGROUND),
      ChildOf(parent),
  ))
  ```

  Seven of the 40 are not push buttons and want no button class: the two demo
  panels' buttons, a picker test fixture, the tab strip's scroll arrow and the
  two link runs (`linkified_text`, `ui_name_link`), which paint no box of their
  own.
- The few that *are* skinned each re-declare `const BUTTON_CLASS: &str =
  "sk-button"` locally. **Seventeen** files did, not eight, with no shared
  constant in `skin.rs` — which had `ACTION_BUTTON_CLASS` but no
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

## Done (2026-09-23)

**The class is a default, not a request.** `ButtonSpec::bordered` starts at
`BUTTON_CLASS` and `ButtonSpec::flat` at `ACTION_BUTTON_CLASS`; `.class(…)` is
an override for a button whose family is its own (the toolbar, the floater
chrome). That one change skinned the 14 call sites that had never asked, with
no edit at any of them — and it is the shape the rest of the widget set already
has, where a checkbox does not get to be spawned unskinned either.

`BUTTON_CLASS` and `COMPACT_BUTTON_CLASS` are `pub const`s in `skin.rs`, and
the **seventeen** local `const BUTTON_CLASS = "sk-button"` are gone. The twelve
now-redundant `.class(BUTTON_CLASS)` chains went with them; the one that stayed
is `my_environments`, where it deliberately puts the push-button look on a
*flat* spec.

**Two sizes, because the boxes it absorbed were not one size.** A panel's
action row was padded 10x5 and a table cell's Remove button 6x1, and giving the
second the first's box would push every row of an access list eight pixels
apart. `.sk-button-compact` is a modifier worn with `.sk-button` that restates
only the geometry, set by `ButtonSpec::compact()`. Which of the two a button is
stays a decision at the call site — it is the only thing that knows what the
button sits in; what each *looks* like is the skin's.

**The caption is covered too.** `spawn_button` gives the label its own class if
the panel named one, else the role its colour names, else `TEXT_CLASS`. The
last fallback is what `.sk-button:disabled .sk-text` needs to reach: a caption
with no class is one the refused state cannot grey, which is the state every
hand-rolled button was in.

**`ButtonSpec::disabled(bool)`** puts `InteractionDisabled` on the box, so a
refusal is a *state* rather than a colour the panel picks. Item properties'
permission-gated buttons were the first call site and the reason it exists:
they were naming the **muted** text colour for a refused button, which is a
different idea (a secondary line) that a skin retunes for a different reason.

**33 hand-rolled buttons became `spawn_button` calls** — About, the crowd debug
button, the asset-editor Save, the volume mute and panel toggle, Build Tools'
contents / material / params / texture / link-part buttons, the inventory
toolbar and gallery nav, the world map's side panel, the web browser's toolbar,
the object inspector's actions, the avatar and group profile cycle buttons, the
avatar / group / texture pickers, About Land's and About Region's in-row
buttons, quick preferences and its preset steppers, Search's day and paging
steppers, the colour picker's pipette and OK / Cancel, and the media bar. Each
kept its own inline colours as the pre-load fallback and its marker component
and observer, which now go on the returned entity.

Three of the 40 the scan found are **not** push buttons and took a different
answer: the colour picker's palette cell is a swatch (its fill is the colour it
holds, so `.sk-swatch`, which paints only the rim), and the two link runs paint
no box at all. Four more are demo-panel and test fixtures.

**The specimen was hand-rolled too**, which is the whole reason this sat: the
registry's own `button` / `button-row` elements spawned their own box, so every
sweep measured a node `ui_element.rs` invented rather than the one the viewer's
panels put on screen. They go through the shared helper now.

**The check.** `unskinned_button_violations` in the testkit fails any node that
carries a button component *and* paints a `BackgroundColor` / `BorderColor` but
no `sk-` class. It is in `layout_violations`, so it runs over every element and
every floater in the matrix rather than being a check somebody has to remember
to point at the panel that broke — and both halves of its condition are
load-bearing: a button that paints nothing (a link run, a tab scroll arrow) has
no surface for a skin to restate, and demanding a class of it would be
demanding a look it deliberately does not have.

**The semantic colours (2026-09-24).** The "Watch out" sweep of the `.colors`
/ `.label_color` call sites found two that said something a skin could not see:
a failed teleport's **Retry** and the **Stand Up / Stop Flying** button were
painted an inline blue to read as the call to act, and `.sk-button` — now a
default — painted over it. `.sk-button-primary` is a second modifier beside
`.sk-button-compact`, restating the *fill* (`--button-primary-bg` and its
hover) where the other restates the geometry, so the two combine;
`ButtonSpec::primary()` sets it. Relief's art is opaque, so that theme tints
the image instead. The rest are the neutral panel shades or a refused state
already on `:disabled`; the `DIM_LABEL_COLOR` hits are row labels, not buttons.

### Not done, and why

A `ui_button` module in `sl-viewer-ui-widgets` beside `ui_checkbox` — item 1 of
the plan — **cannot exist**: `sl-viewer-ui-core`'s own `ui_element.rs` spawns
buttons, and ui-core sits *below* ui-widgets. Moving the helper up would leave
the registry unable to spawn the thing the registry exists to measure. So the
button stays `ui_spawn::spawn_button` in ui-core and gains the widget
*behaviours* the item asked for: it owns the class, the button component, the
tab stop, the caption's class and the refused state.
