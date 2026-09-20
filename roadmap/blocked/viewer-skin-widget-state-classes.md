---
id: viewer-skin-widget-state-classes
title: Widget state belongs in the cascade, not in a per-frame Rust paint
topic: viewer
status: blocked
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-skin-tokens, viewer-skin-image-backed-widgets]
blocked_by: [viewer-audit-skin-token-coverage]
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

## Why it is blocked

It rewrites the same widget paint paths that
[[viewer-audit-skin-token-coverage]] is moving to tokens. One pass, not two.

## Done when

No widget writes a `BackgroundColor` or `TextColor` per frame to express a
state (the accent exception aside), each state is selectable from CSS, and a
scratch skin that gives pressed buttons a loud, obviously different look
proves it end to end in the gallery.
