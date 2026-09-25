---
id: viewer-skin-floater-controls-outside-the-skin
title: A dozen floaters build their buttons, labels and marks with inline colours no skin reaches
topic: viewer
status: ready
origin: viewer-gallery-floaters-are-mostly-stubs — the specimens made these windows visible in the gallery (2026-09-25)
points: 5
refs: [viewer-gallery-floaters-are-mostly-stubs, viewer-ui-button-widget,
  viewer-skin-panel-text-roles, viewer-skin-glyphs-from-content]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Writing a real content specimen for every gallery floater meant reading each
window's spawn code, and a good part of it paints itself rather than asking
the skin. The gallery now shows these windows as they ship, so a skin author
sees them ignore the skin.

**Hand-rolled buttons** — a plain `Node` + `BackgroundColor(ACTION_BACKGROUND)`
(or a `Button` with hard-coded background/border colours) instead of the
button widget, so no `.sk-button` class, often no `Button`, no tab stop and
no greying:

- Block Object by Name: OK / Cancel (`sl-viewer-people/src/blocked.rs`);
- Add to Contact Set and Contact Set config rows (`contact_sets_panel.rs`);
- Conversations: Accept / Decline invite, the pane close and
  add-participants controls (`conversations.rs`);
- RLV Behaviours / Locks / Strings / Console: Copy, Clear, Restore default
  (`sl-viewer-rlv`);
- Asset Blacklist and Avatar Render Settings action columns
  (`sl-viewer-world-avatar`);
- Experience Profile and Experience Picker actions (`sl-viewer-notices`);
- the settings picker's buttons (`settings_picker::spawn_picker_button`), and
  My Environments using the plain button class where every other environment
  window uses the action-button one (`sl-viewer-environment`);
- `personal_lighting::spawn_reset_button` replaces the whole `Node` the button
  helper gave it, discarding the helper's layout;
- Material editor, wearable editor, object contents: buttons and sliders in
  hand-picked colours (`sl-viewer-edit`, `sl-viewer-asset-editors`);
- the web browser's toolbar colours (`sl-viewer-media/src/web_floater.rs`);
- the world map's layer filters (hand-built checkbox rows: no focus, no
  keyboard, no checkbox classes) and its side-panel buttons' colours
  (`sl-viewer-map/src/world_map.rs`).

**Text without a role class** — a bare `TextColor(…)` rather than
`text_role`, so a light skin cannot recolour it: About Land and Region /
Estate labels and values, the RLV console's Clear label, object contents'
row / selection / pending colours; About Landmark's snapshot box has a
hard-coded `BackgroundColor`.

**Marks in content, not glyph slots** — the texture picker's folder arrows
and `📁` are literal characters rather than skin glyph slots.

## What to do

Move each onto the widget or role that already exists for it (the button
widget with its modifiers, `text_role`, `glyph_host`), window by window. The
gallery now shows every one of these windows with its real content, so each
change can be checked there under two skins.

## Done when

None of the windows above paints a control, a label or a mark in a colour
of its own choosing.
