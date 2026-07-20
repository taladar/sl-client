---
id: viewer-ui-focus-ring-visible
title: Draw a visible focus ring on the keyboard-focused widget
topic: viewer
status: done
origin: live review of viewer-ui-menu-keyboard-nav (2026-07-20)
refs: [viewer-ui-menu-keyboard-nav, viewer-ui-menu-bar, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

`Tab` moves keyboard focus between widgets (`bevy_input_focus`'s
`TabNavigation`, wired in [[viewer-ui-widget-scaffold]]), and
[[viewer-ui-menu-keyboard-nav]] added `Tab`-then-`Enter` to open a menu-bar
menu — but **nothing draws where focus currently is**. Tabbing looks inert: the
focused bar button (or any focusable widget) gets no outline, so the user cannot
tell which element will act on the next key. Noticed while live-reviewing the
menu keyboard navigation: "Tab never reaches the top menu bar" was in part
because there was no visible indication that it had.

Wanted: a **focus ring** — an outline / halo drawn on the focused widget
whenever focus should be *shown*. `bevy_input_focus` already models this split:
`InputFocus` is *what* is focused, `InputFocusVisible` is *whether* to draw it
(a desktop UI sets it true on `Tab`, false on a mouse click). The viewer today
paints a ring only for the demo panel (`drive_ui_demo_focus_ring` in
`src/ui.rs`); this task generalises it to every focusable widget:

- Set `InputFocusVisible` true on keyboard focus moves (`Tab` / `Shift+Tab`) and
  false on a pointer click, so the ring shows for keyboard use and hides for the
  mouse — the conventional behaviour.
- Draw the ring on the focused entity when `InputFocusVisible` is true: an
  `Outline` (or a bevy_flair `:focus-visible` token, if the skin system can
  express it — see [[viewer-ui-skin-tokens]]) around the focused node, logical
  and theme-aware like the rest of the UI.
- Cover the menu-bar buttons specifically (the case that surfaced this), plus
  the inventory list / search / tab widgets that already carry `TabIndex`.

Keep it a widget-scaffold-level concern, not per-widget wiring: one system that
reads focus + visibility and paints the ring, so a new focusable widget gets it
for free.

## Done

Taken the **bevy_flair `:focus-visible` route**, not a hand-rolled Rust
`Outline` painter — the direction `common.css` had already started
(`.sk-button:focus- visible`, with a comment that the CSS ring "replaces the
hand-rolled ring"). It is auto-theme-aware (the `--focus-ring` token) and
bevy_flair 0.8 already has exactly the pieces: it maps
`outline-color`/`-width`/`-offset` onto Bevy's `Outline` component natively, and
drives the `:focus-visible` pseudo-state from `InputFocusVisible`.

- **One scaffold system, `stamp_focus_ring_class` (in `skin.rs`).** It tags
  every `Added<TabIndex>` widget with a `sk-focusable` class (merged into
  whatever `ClassList` it already carries), so one CSS rule pair rings all of
  them with no per-widget wiring — a new focusable widget gets it for free the
  frame after it spawns. It lives in `ViewerSkinPlugin`, which **both** the main
  viewer *and* the gallery add (the gallery calls `spawn_ui_root` +
  `ViewerSkinPlugin` but not `ViewerUiPlugin`), so both surfaces get the ring
  from one place.
- **One CSS rule pair (`common.css`).** Drawn as an `Outline` (outside the
  border box, so it never disturbs a widget's own frame or reflows), themed by
  `var(--focus-ring)`. A base `.sk-focusable { outline-width: 0px }` neutralises
  bevy_flair's on-exit revert — Bevy's `Outline::default()` is white/1px, which
  would otherwise flash a stray ring when `:focus-visible` stops matching.
- **`InputFocusVisible` needed no code.** Bevy's `TabNavigationPlugin` sets it
  true on `Tab`/`Shift+Tab` (`handle_tab_navigation`) and the `click_to_focus`
  observer sets it false on a pointer press, so the ring shows for the keyboard
  and hides for the mouse out of the box.
- **Removed the three hand-rolled border-repaint rings** (`drive_ui_demo_focus_
  ring`, gallery's `drive_focus_ring`, the focus half of settings-binding's
  `drive_demo_checkbox_visual`) plus their now-dead consts and the `DemoControl`
  marker, so nothing double-rings. Unit test `stamp_tags_every_focusable_widget`
  pins the merge / fresh-list / non-focusable behaviour.

Live-confirmed in the gallery (graphite ring on `Tab`, recolours on skin cycle).

**Follow-ups filed** — both surfaced *because* the ring made keyboard focus
visible, which is the gallery's discovery loop working as intended:

- [[viewer-ui-focus-scroll-into-view]] — `Tab` can move focus to a widget
  scrolled off-screen; the focused element should scroll into view.
- [[viewer-ui-gallery-tab-order]] — the gallery's `Tab` order jumps around
  rather than following the visual reading order.
