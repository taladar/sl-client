---
id: viewer-ui-focus-scroll-into-view
title: Scroll the keyboard-focused widget into view
topic: viewer
status: done
origin: live review of viewer-ui-focus-ring-visible (2026-07-20)
refs: [viewer-ui-focus-ring-visible, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Now that [[viewer-ui-focus-ring-visible]] draws where keyboard focus is, a
second problem is visible: `Tab` happily moves focus to a widget that is
**scrolled off-screen**. In the gallery — an `Overflow::scroll` page
(`GalleryPage`, moved by `scroll_gallery` via `ScrollPosition`) — tabbing past
the fold rings a widget the user cannot see. The same will bite the inventory
list and any other scroll container with focusable rows.

Wanted: when keyboard focus moves (`InputFocus` changes with
`InputFocusVisible` true), **scroll the focused widget into view** — the
conventional desktop behaviour. Keep it a widget-scaffold-level concern like the
focus ring: one system that, on a focus change, walks up from the focused entity
to the nearest scroll container (a `Node` with `Overflow::scroll` + a
`ScrollPosition`), computes the focused widget's box relative to the container's
visible area (via `ComputedNode` / `GlobalTransform`), and nudges
`ScrollPosition` just enough to bring it fully on screen (no-op when it already
is). So a new focusable widget inside any scroll container gets it for free.

- Cover the gallery page and the inventory list specifically (the two scroll
  surfaces that carry `TabIndex` today).
- Do not fight the wheel: only move when focus lands outside the visible band,
  and clamp to the scrollable range (`bevy_ui` clamps the far end at layout).
- Mouse focus (`InputFocusVisible` false) should not scroll — only keyboard
  navigation, matching the ring's show/hide rule.

## Done

One scaffold system `scroll_focus_into_view` (`ui.rs`), plus a pure, unit-tested
core `reveal_delta` and a small `FocusRevealBounds` component.

- **THE bug that ate the first attempt:** `Node` *requires* `ScrollPosition`
  (Bevy 0.19 required component), so **every** UI node has one. Detecting the
  scroll container by "has `ScrollPosition`" matched the focused widget's own
  parent, which always contains it, so nothing ever scrolled. A container is
  identified by its `Overflow` actually being `OverflowAxis::Scroll` on an axis
  (`scrolls_on_some_axis`) — the roadmap text above already said `Overflow`, but
  the code checked the wrong thing until a debug dump showed `view == item`
  every time.
- `reveal_delta(viewport_min, viewport_max, item_min, item_max) -> Option<f32>`:
  the per-axis scroll delta, near/leading-edge priority (align the top when the
  item is taller than the viewport), `None` when already visible so the wheel is
  left alone. Unit-tested; the system just wires `ComputedNode`/
  `UiGlobalTransform` boxes to it.
- Runs in `PostUpdate` `.after(UiSystems::Layout)`, where those two are fresh
  (both written by `ui_layout_system`). `UiGlobalTransform.translation` is the
  node **centre**; boxes are **physical** pixels, `ScrollPosition` is
  **logical**, so the delta is scaled by `ComputedNode::inverse_scale_factor`;
  near end clamped to 0, `bevy_ui` clamps the far end at layout. Keyboard-only
  (`InputFocusVisible`), matching the ring.
- Registered in **both** `ViewerUiPlugin` and the gallery's own setup — the
  gallery stands up the scaffold systems by hand rather than adding
  `ViewerUiPlugin`, the same split the focus-ring stamp navigates.
- **`FocusRevealBounds(Entity)`** (live-review refinement): a composite widget
  whose focus stop is smaller than its visual whole names a wider bounding
  entity; the system reveals the **union** of the focus stop and that target, so
  the ring on the small stop stays visible while as much of the whole as fits
  comes in, on **both** axes (a vertical tab strip revealed horizontally brings
  its side panel too). The tab widget (`ui_tab::spawn_tab_container`) points its
  strip — its one focus stop — at its container. Standalone strips (no panel)
  carry none and fall back to their own box.

Scope note: only the **nearest** scroll container is moved. A widget inside a
*nested* inner scroll container (e.g. a tab strip's own scrolling viewport, when
that viewport is itself off the outer page) is revealed within its inner
container but the outer page is not chained — an accepted edge case, matching
the "nearest scroll container" wording above.

Live-confirmed in the gallery: tabbing up and down keeps the focused widget on
screen, and reaching a tab widget brings its whole panel in, both directions.
