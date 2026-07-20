---
id: viewer-ui-focus-ring-visible
title: Draw a visible focus ring on the keyboard-focused widget
topic: viewer
status: ready
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
