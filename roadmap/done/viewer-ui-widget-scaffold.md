---
id: viewer-ui-widget-scaffold
title: UI widget scaffold (bevy_ui plugin + conventions anchor)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-text-foundation, viewer-ui-text-grapheme-backdelete]
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

## Done

`src/ui.rs` — `ViewerUiPlugin`. `DefaultPlugins` already brings `UiPlugin`,
`UiWidgetsPlugins` and `InputFocusPlugin`/`InputDispatchPlugin` (the `ui`
feature is on by default), so the plugin adds what it does not:
`TabNavigationPlugin` (without which `Tab` is inert — focus is *dispatched* but
never *navigated*), the bundled font stack, and the `UiRoot`.

Convention 1 is `UiDirection` + `LogicalRect` (`inline_start`/`inline_end`/
`block_start`/`block_end`) + `LogicalMargin`/`LogicalPadding`/`LogicalBorder`,
resolved into the physical `Node` each frame. Most of `bevy_ui` turned out to be
logical already (`FlexDirection::Row` means "along the text direction"; the
alignment enums only offer `Start`/`End`), so the box model was the only leak.
The load-bearing find: **taffy has no style inheritance** — it reads `direction`
off each node's own style and defaults to `Ltr` — so the direction is written to
every node, not just the root.

Convention 2 is `column()`/`row()` plus the rule; `LogicalInset` was
deliberately **not** added (no consumer until a floater remembers a position —
see [[viewer-ui-floater-basic]]).

Beyond the brief, and why:

- **`UiPanelShown`** — hiding a panel is three things, not one: `Display::None`
  (not `Visibility::Hidden`, which leaves a panel-shaped hole in the root's
  flow), **parking its `TabIndex`** (`bevy_input_focus` walks the hierarchy
  without consulting visibility or display, so a closed panel's buttons stay
  reachable by `Tab` — nothing upstream handles this), and dropping focus that
  is inside it.
- **An `F5` demo panel**, in the pattern of the `F3`/`F4` overlays. Without an
  in-app consumer the whole logical vocabulary was dead code, and tab navigation
  was unverifiable. Three buttons, not two: with two, `Tab` and `Shift+Tab` are
  indistinguishable and neither order nor direction is observable. The third
  cycles the text size, which also covers the "reflows on a font-size change"
  claim that nothing else tested.
- **[[viewer-input-focus-contexts]] was folded in** rather than left to follow —
  see that task. The scaffold is what makes focus reachable, and a focused text
  field that also walks the avatar is not a finished foundation.

[[viewer-ui-text-foundation]]'s `F4` panel was rebuilt on all of this, which
also fixed a latent bug: its editor's fixed `width` overflowed the panel's
`max_width` by exactly its padding — the fixed-rect failure mode in miniature.
