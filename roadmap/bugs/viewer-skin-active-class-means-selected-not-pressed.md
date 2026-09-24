---
id: viewer-skin-active-class-means-selected-not-pressed
title: .sk-active means "selected", which is not what CSS's :active means, and nothing but relief draws a press
topic: viewer
status: bugs
origin: the user comparing the reference's bottom bar while viewer-skin-glyphs-from-content was in progress (2026-09-24)
refs: [viewer-skin-widget-state-classes, viewer-skin-image-backed-widgets,
  viewer-vintage-skin, viewer-skin-glyphs-from-content]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

## Observation

The reference's bottom-bar buttons change their look **while the mouse is
held down on them**. In CSS that state is `:active`. Ours do not change at
all in the two flat skins.

## Two defects, one name

**The name.** Our skin vocabulary uses `.sk-active` (`ACTIVE_CLASS` in
`sl-viewer-ui-core/src/skin.rs`) for *toggled on / selected*: a toolbar button
whose floater is open, the active tab of a strip, the selected row of a
table. That is CSS's `:checked`, or ARIA's `aria-pressed` /
`aria-selected`. It is not `:active`, which CSS defines as "being activated
by the user", i.e. held down. A skin author who knows CSS reads `.sk-active`
as the pressed state and writes the wrong rule. This has already cost this
codebase once: the active-group marker in `groups.rs` could not use
`.sk-active` for "this is the active group", because `.sk-active` also
paints a selection background, so it needed a separate `sk-current` class
(`glyph::CURRENT`).

About 103 uses across 28 files (Rust and CSS) as of 2026-09-24.

**The missing state.** `bevy_flair` does support `:active`. It maps to
`Interaction::Pressed` and bevy_ui's `Pressed` marker. The only rule using it
is the `relief` theme's `.sk-button:active` nine-slice. `common.css` has no
pressed look for any widget, so under `graphite` and `azure` a press is
invisible on every button, and the bottom bar's buttons in particular. The
reference draws it as a texture of its own (`image_pressed` /
`image_pressed_selected` in `widgets/button.xml`). The Vintage skin needs
it: its pressed and toggled buttons wear a warm gold frame.

## What to do

- Rename `.sk-active` / `ACTIVE_CLASS` (and its text half,
  `.sk-active-text` / `ACTIVE_TEXT_CLASS`) to a name that cannot be read as
  the pointer state. `.sk-selected` fits rows and tabs; a toggle button is
  "on" rather than "selected", so check whether one name serves both or
  whether a toolbar toggle wants `:checked` (a `Checked` marker) instead.
  Update both shipped skins, the themes, the test stylesheets, the book's
  skin chapter token table and the vocabulary test at the same time.
- Give every button family a `:active` rule and a token pair
  (`--control-bg-pressed` / `--control-border-pressed`, or per family where
  the reference distinguishes them): push buttons, flat action buttons, the
  toolbar and bottom-bar buttons, the floater chrome buttons, tab-scroll and
  scrollbar arrows. Include pressed-and-selected, the reference's
  `image_pressed_selected`.
- Check that every one of those widgets actually gets `Pressed` or
  `Interaction::Pressed` synced. A `ButtonKind::Plain` box (no button
  component) has neither, so `:active` can never match it.

## Done when

No class in the skin vocabulary shares a name with a CSS pseudo-class that
means something else, a press is visible on every button in both flat skins,
and a scratch skin can give the bottom bar's buttons a distinct pressed look
by CSS alone.
