---
id: viewer-ui-focus-scroll-into-view
title: Scroll the keyboard-focused widget into view
topic: viewer
status: ready
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
