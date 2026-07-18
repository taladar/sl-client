---
id: viewer-ui-floater-basic
title: Floater window manager (basic)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The floater window manager, basic tier: a draggable title-bar window with drag,
z-order, focus, and close. Nothing upstream has a floater manager — every SL
viewer hand-writes one — so this is ours to build on top of `bevy_ui`.

Per the cross-cutting layout convention, a floater **sizes to its content**
(clamped to min-content) and **reflows on a font-size or locale change** — no
fixed pixel rects — so a longer translated label or a larger UI font never
overflows or clips. Resize / minimize / dock / tear-off are the follow-on
[[viewer-ui-floater-resize-dock]].

Reference (Firestorm, read-only): `indra/llui/llfloater`, `llfloaterreg`.
Every panel/floater task ([[viewer-chat-history-panel]],
[[viewer-emoji-picker-floater]], the repointed panel ideas) builds on this.

## Done

`src/floater.rs` — `FloaterPlugin`. A `Floater` root under the scaffold `UiRoot`
with a **title bar** (drag to move, direction-aware), **z-order** (any press
raises via a monotonic `GlobalZIndex`; `ActiveFloater` tracks the front-most one
and highlights its title band with the reference's `White_10` focus wash + a
brightened title), and **close** (a title-bar `✕`, plus `Ctrl+W` on the active
floater). Kept on screen by an inline-space clamp that keeps ≥ 16 px reachable
(`FLOATER_MIN_VISIBLE_PIXELS`), corrected the same frame so there is no
snap-back bounce.

The scaffold's reserved **`LogicalInset`** (`src/ui.rs`) landed here — the
logical `left`/`right`/`top`/`bottom` resting at `Val::Auto`, so a floater's
remembered leading/top offset mirrors under RTL and its unset edges follow flow.

Content-driven, not a pixel rect: a floater sizes to its content (the title bar
sizes to the title + chrome rather than a fixed `header_height`). Chrome actions
route through one `FloaterCommand` message so the observers stay one-liners and
the reparent/restack logic is one testable system; each observer captures its
floater root by `move` (a bubbled `Pointer` keeps the *original* hit entity).

**First live consumer: the inventory window** (`src/inventory.rs`), re-hosted in
a floater — so it drags, raises and closes for real. A static chrome
**specimen** is registered in `crate::ui_element::ELEMENTS`, so the headless
matrix sweeps the title bar / buttons / content across every script, size and
direction.

Deliberate deviations from the reference, documented in the module:
bring-to-front does **not** steal keyboard focus (that stays with the clicked
child), and sibling-snap is a follow-on. Follow-up:
[[viewer-ui-floater-persist-geometry]] (remember position + size per user).
