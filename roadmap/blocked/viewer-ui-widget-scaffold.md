---
id: viewer-ui-widget-scaffold
title: UI widget scaffold (bevy_ui plugin + conventions anchor)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-text-foundation]
---

Context: [context/viewer.md](../context/viewer.md).

The "the framework is stood up" anchor: a viewer UI plugin that wires
`bevy_ui` + `bevy_ui_widgets` + `bevy_input_focus` into the app, establishes a
`UiRoot`, a focus resource and tab navigation, and turns on
`system_font_discovery` + the bundled colour-emoji font from
[[viewer-ui-text-foundation]]. Most other UI tasks `blocked_by` this one.

Crucially, this task **establishes the two cross-cutting conventions** every
downstream widget/panel inherits (the reference viewer does neither, so there is
no prior art to copy and a late retrofit is expensive):

- **Direction-neutral, bidi-first APIs.** Name every directional API and style
  hook **logically** — `forward`/`backward`, `start`/`end`, `leading`/`trailing`
  — never `left`/`right`. We implement the Unicode Bidirectional Algorithm; an
  RTL locale must mirror layout with no per-side special-casing.
- **Content-driven auto-layout.** Build on Bevy's taffy/flexbox with min-/
  max-content sizing; **no absolute pixel rects**. This is a strict superset of
  the reference's absolute-`topleft` model and dissolves the whole class of
  breakage where a longer translated label overflows a fixed-width panel.

The ~40 generic widget primitives come from `bevy_ui_widgets`; the ~100
viewer-domain composites (trees, chiclets, texture pickers, the net map) are
written per feature in their own tasks.

Reference (Firestorm, read-only): `indra/llui/` (`llpanel`, `lluictrlfactory`),
and the XUI layouts under `newview/skins/` (as a feature checklist, **not** to
import — the pixel coordinates *are* the design and cannot carry over).
