---
id: viewer-skin-checkbox-radio-shape
title: Checkboxes are hand-rolled per panel, so no skin can shape one
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 5
refs: [viewer-ui-radio-widget, viewer-ui-settings-binding, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

There is no checkbox widget. `sl-viewer-ui-widgets` has `ui_radio`, `ui_combo`,
`ui_slider`, `ui_table`, `ui_tab`, `ui_text_input` — and every checkbox in the
viewer is a square `Node` spawned by whichever panel needed one, with that
panel's own constants. `settings_binding.rs` has `bound_checkbox`;
`preferences/phototools.rs` declares its own `PhotoCheckboxBox` and a fill
colour pair; `environment/my_environments.rs` declares `CHECK_SIZE`,
`CHECK_ON`, `CHECK_OFF`. They do not agree with each other, and none of them
can be reached by a skin.

The reference draws a checkbox as a **light box with a dark frame and a tick**
— Vintage's is a `#d8d8d8` face inside a 1 px `#0a0a0a` frame with a soft drop
shadow, and a radio button is a white disc in a dark ring, both 15×15. Ours
are filled squares that go blue when checked. The shapes are different in
kind, not in tint: one is a container you look into, the other is a swatch.

## What to do

- A shared `ui_checkbox` in `sl-viewer-ui-widgets`, on the same model as
  `ui_radio`: box, tick, label, the interaction contract the rest of the set
  follows, and a class the cascade can select. `ui_radio` is the template —
  including its two-node problem, since `bevy_ui` has no style inheritance and
  the box and its label must each carry their own class.
- Role tokens for both: box surface, frame, tick, and the disabled variants;
  the tick's colour is not the label's.
- Migrate the hand-rolled call sites — `bound_checkbox` first, since the
  settings panels are most of them — and delete their local constants.
- Leave room for the image-backed form ([[viewer-skin-image-backed-widgets]]):
  the reference expresses checked / unchecked / pressed / disabled as six
  separate textures, so the state model must be the selector one from
  [[viewer-skin-widget-state-classes]], not a per-frame fill.

This is as much a consistency fix as a skinning one — three spellings of a
checkbox is three chances for one of them to drift, and two of them already
have.

## Done when

One checkbox widget exists, every panel uses it, its shape and colours come
from the skin, and a scratch skin can make it a light box with a dark frame
and a tick without touching Rust.
