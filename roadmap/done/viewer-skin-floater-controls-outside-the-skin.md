---
id: viewer-skin-floater-controls-outside-the-skin
title: A dozen floaters build their buttons, labels and marks with inline colours no skin reaches
topic: viewer
status: done
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

## Implementation (2026-09-25)

Every window above now builds its controls from the widget or role that
exists for them:

- **Buttons** go through `ui_spawn::spawn_button` as headless buttons observing
  `Activate`, so each gained a skin class, a tab stop and `Enter` / `Space`:
  Block Object by Name's OK / Cancel and the Blocked tab's action column;
  Contact Sets' chooser, action column, Add to Contact Set and Contact Set
  Settings buttons (their greying is now `InteractionDisabled` +
  `.sk-button:disabled`); Conversations' Accept (`.primary()`) / Decline and the
  pane close / add-participants glyph buttons; RLV Copy / Clear / Restore
  default (one `style::spawn_action_button`); the Asset Blacklist and Avatar
  Render Settings action columns; the Experience Profile / Picker /
  Experiences action buttons. The settings picker's buttons use the widget
  too, and My Environments wears the action-button class its siblings do.
  `personal_lighting::spawn_reset_button` amends the helper's `Node` (the top
  margin) instead of replacing it.
- **Sliders**: the slider widget itself carried no class, so no slider in the
  viewer was skinnable. The track now wears `.sk-slider` and the thumb
  `.sk-slider-thumb` (`--track-bg`, `--control-border`, `--slider-thumb`,
  greyed from `:disabled`); `SliderStyle`'s colours are the pre-load fallback.
- **Text**: About Land / About Region labels and values, the About Land
  environment tab, the RLV console prompt and the world map's side-panel
  labels take `text_role`; object contents' rows are `.sk-list-row` with
  `.sk-selected` and a role class that moves to the disabled role while an
  item is pending; the world map's search results are list rows the same way.
- **Surfaces / marks**: About Landmark's snapshot box wears the new
  `.sk-image-well` (`--tile-bg`); the texture picker's folder arrow is a
  `glyph::DISCLOSURE` slot (with `EXPANDED`), and its 📁 is the inventory's own
  `item_icon(Category)`, which [[viewer-skin-icon-set]] owns.
- The world map's layer filters are the shared checkbox widget, still raising
  the `toggle-*` actions the menu raises; the UI contract gains `Enter` and
  `Space` rows per filter. The web browser's and world map's buttons drop their
  hand-picked fallback colours.
- `.sk-disabled-surface` / `DISABLED_SURFACE_CLASS` had Contact Sets as its
  only user and is retired; `.sk-disabled-text` stays.

The gallery's sliders did not move (found on the live look): bevy's headless
slider only announces a drag, and the gallery never added
`SettingsBindingPlugin`, whose observer writes a bound slider's value back. It
does now; the parcel audio bar's specimen, whose slider was deliberately static
and whose buttons were hand-rolled copies, builds with the live bar's
`spawn_glyph_button` and bound slider.

Tests: `a_slider_carries_the_classes_the_skin_paints`,
`a_drag_along_the_track_moves_the_value`,
`a_skinned_slider_thumb_follows_its_value` (real stylesheet),
`block_by_name_ok_acts_on_enter_space_and_a_click`,
`a_blacklist_action_acts_on_enter_space_and_a_click`.

Not touched, as not listed: the Conversations dock host / transcript / invite
bar backgrounds (surfaces), the day-cycle strip, and the world map's region
name labels drawn over the map imagery.

## Closed (2026-09-25)

The user checked the gallery: the sliders move. Quick preferences' value
readout stays frozen in the gallery, because it follows the settings store the
gallery lacks; a readout owned by the widget is
[[viewer-sliders-show-no-value]].
