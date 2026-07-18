---
id: viewer-ui-floater-resize-dock
title: Floater window manager (resize / minimize / dock / tear-off)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-floater-basic]
---

Context: [context/viewer.md](../context/viewer.md).

The second tier of the floater manager on top of [[viewer-ui-floater-basic]]:
manual **resize**, **minimize**, **dock** (snap/host a floater into a
container), and **tear-off** (detach a docked panel back into a free floater).
Manual resize composes with the content-driven auto-sizing from the basic tier —
the user's size is a constraint layered over the min-content floor, not a
replacement for it.

Reference (Firestorm, read-only): `indra/llui/llfloater` (resize handles,
`llmultifloater` docking), `lllayoutstack`.

## Done

Built into `src/floater.rs` on the basic tier:

- **Resize** — a visible `◢` grip at the trailing-bottom corner (mirrored under
  RTL) drags the floater's **content-area size** (`Floater::content_size`),
  which the consumer's own content fills. It grows *and* shrinks, floored at a
  per-floater `min_size` (`FloaterSpec::min_size`) below which the chrome would
  not fit; the content slot **clips**, so nothing ever renders past the window
  edge. This is the "definite size for a scroll list" case the content-sizing
  convention carves out — the inventory list has no natural width.
- **Minimize** — collapses to a title-only strip (content out of the layout),
  the `—` glyph swapping to `▭` to restore; the strip
  **holds the window's width** so the restore/close buttons stay put between
  states.
- **Dock / tear-off** — the `▤` button reparents the floater into a host
  container (a trailing-edge dock host the plugin spawns), disabling its drag /
  resize / minimize while docked and keeping it bounded; the `▥` glyph (or a
  title-bar drag past the reference's 12 px slop) tears it back off, restoring
  the free window and re-docking into the *last* host on the next dock.

Every operation is unit-tested through the command system (close/minimize/dock/
tear-off/raise) plus pure helpers (drag→inset, resize→floor, clamp, z-order).

**Deviation from the reference, flagged and accepted:** the dock host is a
vertical **stack**, not a tabbed `LLMultiFloater` — the reparent mechanism is
faithful, but the tabbed presentation is deferred to [[viewer-ui-tab-widget]],
which the host will adopt. Minimized-corner tiling is likewise a follow-on (it
minimizes in place). Follow-up: [[viewer-ui-floater-persist-geometry]].
