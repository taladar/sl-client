---
id: viewer-skin-widget-state-classes
title: Widget state belongs in the cascade, not in a per-frame Rust paint
topic: viewer
status: done
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-skin-tokens, viewer-skin-image-backed-widgets]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Several of our widgets paint their **state** — hovered, pressed, toggled-on,
disabled — from Rust, every frame, and `common.css` then deliberately declines
to set a background so the skin does not fight the paint. The comments say so
in as many words:

- `.sk-menu-bar-item` / `.sk-menu-item`: "The hover highlight is painted in
  Rust (`menu.rs` `highlight_menu_hover`) … Deliberately no
  `background-color`, so the skin does not fight the per-frame highlight."
- `.sk-toolbar-button`: "enabled / lit (floater open) / disabled background and
  label colours are painted in Rust (`bottom_toolbar.rs`
  `update_toolbar_button_states`) … so this rule carries only the skin's corner
  and border."

That was a reasonable call when the skin was two classes wide. It stops being
one the moment a skin wants to express state *at all*, because the widget's
most visually loaded moments are then the ones the skin cannot reach. In the
reference's Vintage skin state is not a tint — idle, pressed/toggled and
disabled are **three different textures**, and the pressed one changes hue
family completely (a blue-violet face `#6473bd` picks up a warm gold frame
`#ffd794`). A skin that cannot restyle a pressed button cannot be that skin.

## What to do

Move the state onto the entity as something the cascade can select, and let
CSS decide what it looks like:

- Prefer the pseudo-classes `bevy_flair` already resolves (`:hover`,
  `:focus-visible`, and whatever it exposes for pressed/checked) where the
  state genuinely is one of those.
- For the states that are ours and not the engine's — "the floater this button
  toggles is open", "this action does not apply right now" — add explicit
  marker classes on the same model as the existing `.sk-disabled-surface` /
  `.sk-disabled-text` pair, which already proves the shape works (two classes,
  because `bevy_ui` has no style inheritance and the surface and its label are
  separate nodes).
- Keep exactly one legitimate exception, already documented:
  `notification_host.rs` `kind_accent` is *meaning-bearing* (Tip / Notify /
  Alert / Modal), not a fallback.

The per-frame write is worth removing on its own account:
[[viewer-audit-ui-widget-per-frame-writes]] and
[[viewer-perf-ui-layout-gate-open-widget-churn]] are about the same systems
from the performance side, and a class that changes only when the state
changes is strictly less work than a colour written every frame.

## What the blocker landed (2026-09-21)

[[viewer-audit-skin-token-coverage]] is done, and it moved the paint paths
this task rewrites onto **role tokens** — a `SkinPalette` shim on the styled
root, read through a `SkinColors` system param. So the colours a state is
painted in are already the skin's; what is still Rust's is the *choice* of
which role each state takes. That is exactly this task's remaining half.

It also widened the field: the same per-frame state paint now covers the
floater title bar, a tab's active / inactive / disabled trio, table row
selection, the combo's disabled anchor, the radio indicators, the menu's hover
and filter-match, and the pie menu's captions and disc.

**One thing to read past.** `common.css` and `skin_palette.rs` now say in
several places that a class would *beat* the Rust-painted value and flatten
the state distinction — true while Rust keeps painting, and not an argument
against this task, which removes that writer. Once the cascade is the only
writer there is nothing left to fight. The pie disc is the one piece that
cannot become a class at all: it is a shader, not a node, and its three
colours ride in through `drive_pie_material`'s params.

## The unskinned world: bake a fallback sheet (decided 2026-09-21)

Moving state wholly into CSS costs one thing: a world with no stylesheet stops
showing the distinction at all. Today an unskinned world (a widget crate's unit
tests, the gallery before its first dress) still renders active / disabled
correctly, because Rust paints it from `SkinPalette::FALLBACK`.

**The decision is to accept that and remove the case instead**: bake a fallback
stylesheet into the binary (`bevy::asset`'s `embedded_asset!`), so there is
always a sheet and the logic never branches on "is there a skin". Nothing else
about the design changes. The viewer is already badly degraded with no assets —
untranslated strings everywhere — so "no assets" is not a state worth designing
the widget paint around.

Two things to settle while doing it, neither a one-liner:

- **Layering.** The skin CSS lives in `sl-client-bevy-viewer/assets/`, but the
  widgets that stop painting live in `sl-viewer-ui-core` and
  `sl-viewer-ui-widgets`. That split is deliberate — `tests/shipped_skins.rs`
  sits in the viewer crate so `sl-viewer-ui-core` does not reach outside its
  own directory and widen its commit-hook relevance to the whole repository
  (see `book/src/tools/build-performance.md`). A fallback those crates' **own**
  unit tests can see therefore has to live at or below them: either a minimal
  sheet in `sl-viewer-ui-core` that `common.css` supersedes, or the widget
  tests take the binary's assets.
- **`@import` paths are asset-root-relative** (`@import "skins/common.css"`),
  so an embedded source needs the same internal layout or the imports resolve
  to nothing — silently, like every other link in this chain.

`SkinPalette::FALLBACK` then stops being load-bearing: still the backstop for a
third-party skin that omits a role, no longer what anything actually renders
from.

## What landed (2026-09-21)

Three commits. The state vocabulary and an embedded fallback sheet so there is
always a stylesheet to select from, then the widget set family by family.

**Nothing needed a marker class where the engine already knew the state.**
`bevy_flair` syncs `bevy_ui::Checked` to `:checked` and `InteractionDisabled`
to `:disabled`, and both were already being maintained for accessibility — so
tabs, radios, the combo, the menu rows and the friends list's permission
checkboxes select on the state they already had. Markers were needed only for
states that are genuinely ours: the menu highlight (keyboard navigation and an
open sub-menu's ancestor chain, neither of which `:hover` sees), the floater
title (our `ActiveFloater`, not `InputFocus`), the toolbar's "its floater is
open", and a table row's selection in a recycled virtual list.

Net: four systems, two observers, a component, six colour helpers, three
skin-chasing `SkinColors::is_changed` widenings and two hand-written classes
deleted, against ~90 lines of CSS. The widget set is smaller than it was.

**Three exceptions stand, each for a reason.** The pie disc is a shader rather
than a node; `notification_host`'s `kind_accent` is meaning-bearing; and the
icon swaps (the radio's ring, the rights checkbox) stay in Rust because CSS
cannot change which image a node shows — which is load-bearing, since it means
those distinctions survive a skin that recolours badly.

**A capability went with it.** Nothing had ever disabled a whole tab *strip* —
every insert was in the widget's own tests, and the reference has no
container-wide equivalent of `enableTabButton` — so the refusal, the divider's
resize gate and its tests were dropped, with `a_marked_strip_still_switches`
pinning the new rule.

The panels were **not** in scope and are surveyed in
[[viewer-skin-panel-state-classes]]: 26 of their 54 state constants turned out
to be an existing class under a local name, nine of them hand-copying
`selection_bg`'s exact value.

## Done when

No widget writes a `BackgroundColor` or `TextColor` per frame to express a
state (the accent exception aside), each state is selectable from CSS, and a
scratch skin that gives pressed buttons a loud, obviously different look
proves it end to end in the gallery.
