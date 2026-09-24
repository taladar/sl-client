---
id: viewer-gallery-floaters-are-mostly-stubs
title: 43 of the gallery's 56 floaters show a line of prose instead of their content
topic: viewer
status: bugs
origin: the user opening the gallery's floaters while checking viewer-skin-glyphs-from-content (2026-09-24)
refs: [viewer-skin-glyphs-from-content, viewer-floater-minimize-caps-follow-no-pattern,
  viewer-audit-specimens-carry-widget-classes]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Most floaters the gallery opens contain only a sentence describing what the
live window would show. A skin author checking a skin, or anyone checking a
change to a widget those windows are built from, sees nothing of what the
window really looks like.

The mechanism is `FloaterContent` in `sl-viewer-ui-widgets/src/floater.rs`:
a registered floater's content is either `Specimen(fn)` (the module's static
content specimen, the real layout) or `Stub(&str)` (one line of prose). The
stub was meant as "an obvious, greppable place for the specimen when someone
writes one", so that the chrome could be swept before every window had content.
Nobody came back to fill them in: 43 of the 56 entries in
`sl-client-bevy-viewer/src/floaters.rs` are still stubs:

about, about-land, about-landmark, about-region, add-to-contact-set,
asset-blacklist, avatar-picker, avatar-profile, avatar-render-settings,
block-by-name, contact-set-config, conversations, day-cycle-editor,
experience-profile, experience-picker, group-picker, group-profile, inventory,
inventory-filters, inventory-gallery, item-properties, material-editor,
my-environments, object-contents, panorama, personal-lighting,
preview-animation, preview-texture, rlv-behaviours, rlv-console, rlv-locks,
rlv-strings, search, settings-editor-sky, settings-editor-water,
settings-picker, snapshot, telehub, texture-picker, top-colliders, top-scripts,
wearable-editor, web-browser.

The UI layout sweep has the same gap: its floater pass measures a stubbed
window's chrome around one line of text. A layout bug inside any of these 43
windows goes unseen.

## What to do

- For each stub, first check whether the module already has a static
  specimen registered as a gallery **element** (`ui_elements.rs`); where one
  exists, switching the floater to `FloaterContent::Specimen` costs almost
  nothing.
- Write the missing specimens, driven by the same spawn code as the live
  window, the way the existing specimens are, with fixed sample data in place
  of a session. A specimen that hand-rolls its own nodes shows none of the
  skin ([[viewer-audit-specimens-carry-widget-classes]]).
- Make the stub harder to leave behind: a test listing the floaters still
  stubbed, which a new stub must be added to and a converted one removed
  from. The list can then only shrink.
- Related gap found the same day: no notification specimen shows the toast
  queue's "N more ▸" overflow control, so its glyph (a `::after` slot, the
  only one of its kind) cannot be seen without a live grid. A specimen with
  more toasts than `MAX_VISIBLE_TOASTS` would show it.

## Done when

Every gallery floater shows its real layout with sample data, and the layout
sweep measures that rather than a sentence.
