---
id: viewer-gallery-floaters-are-mostly-stubs
title: 43 of the gallery's 56 floaters show a line of prose instead of their content
topic: viewer
status: done
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

## Done (2026-09-25)

Every floater shows its real content, and the stub is gone from the type.

- **One builder, two callers.** Each of the 43 windows' content building moved
  out of its live spawn system into a `spawn_…_content(commands, slot,
  font_size, …)` that the live window and a new `spawn_…_specimen` both call.
  The specimen fills it with fixed sample data through the helpers the live
  window shows data with (row binds, populate / paint systems run once,
  `project` / `row_cells`-style pure functions split out of the bind systems)
  — never a hand-rolled look-alike. Shared helpers for this:
  `virtual_list::spawn_specimen_row` and `ui_table::spawn_specimen_table_rows`
  (a table's rows as its live bind would place them).
- **The stub cannot come back.** `FloaterContent` is gone;
  `FloaterElement::content` is a plain builder fn (`FloaterContentFn`). The
  registry's docs say why.
- **The sweep measures the window, with text in it.** The floater sweep
  resolves every `Translated` label to the real English and then applies the
  cell's script / pseudolocale transform (`install_cell_strings`), and both
  floater hosts run the virtual-list and table widget systems
  (`ui_contract::install_list_widgets`), so a table lays out as it does live.
- **Harness gaps it exposed, closed:** a node's on-screen check now uses its
  clipped box (the fourteenth page of a license text inside a scroll area is
  not "off screen"); a violation on an anonymous node names its nearest named
  ancestor; a parent declaring `ContentMayOverflow` exempts its children from
  containment. Declarations, each with a reason, on what clips by design: the
  table's cell and header clips and the column-resize grip that straddles the
  border, the search slot, a clipped tab label, and `VirtualViewport` (which
  now requires both `ContentMayOverflow` and `TextMayClip`).
- **The real layout bugs it then found, fixed** in the live windows — tables
  squeezed to 0 px at 22 px (About Land / Region, telehub, top objects),
  fixed-width 140 px slider columns and 12 px trackball compass boxes (every
  environment window), a 430 px fixed day-cycle timeline, non-wrapping button
  rows (inventory, my environments, experience profile), the snapshot floater
  50 px taller than a 1280×800 screen (its preview is now 512×320), fixed row
  heights at large fonts (inventory, RLV console, texture picker, object
  contents), the Conversations transcript scroll box laid out as a row, the
  search details pane running under its scrollbar, and more.
- **Found and fixed on the way:** About Landmark never showed the parcel
  description (it stored the column, not the text node); the RLV Behaviours
  and Locks lists named their viewport as their table, so their cells never
  reached the `TableState` the width sync reads.
- **Toast overflow:** a `notification-overflow` element shows a full channel —
  the visible toast and the live "N more ▸" control with its `::after` glyph.
- **The gallery survives a click.** A specimen carries the live window's
  observers, whose parameters name session resources the gallery does not
  have; the gallery's fallback error handler now logs such a failure at
  `error!` instead of panicking.

### Second round: the whole window, and no needless scrollbar

The user's look at the gallery found windows that still did not match the
viewer, for two reasons, both now fixed:

- **Ten specimens predating the task were hand-drawn sketches** — Build Tools,
  Preferences, Quick Preferences, Phototools, Debug Settings, the emoji picker,
  Experiences, the radar, the minimap and the world map. Each is now the live
  window, built and filled through its own code (Build Tools with a sample
  prim selected, every tab filled by the module that owns it; Preferences with
  all nine tabs; the maps composited by their live renderers).
- **Some windows are assembled by more than one module.** Conversations gets
  the People tab (and through it the Friends / Groups / Blocked / Contact Sets
  panes) from `people.rs`, and a class on its root; the group profile's
  details and compose areas, the land Environment tab, the environment
  editors' slider readouts, Preferences' device / theme / alert lists and more
  are drawn by other systems. Each composing step is now a function the live
  system and the specimen both call.
- **`every_floater_default_size_shows_its_content`**: a floater whose scroll
  area hides some but at most half of its content, in a window under 80% of a
  1280×800 (logical) laptop screen on that axis, must grow its default size.
  Windows grown by it or by the user's review: About Land 890×630, Region /
  Estate 800×600 (its Access and Experiences lists now side by side, as the
  reference's), Search 930×460, Telehub 360×420, Build Tools 510×640, Avatar
  Profile 550×600, World Map 620×600, Preferences 760×620, Day Cycle 705×715
  with four knob columns, Phototools 400×680, and others. The persisted rects
  of the changed windows were purged from the local test accounts.
- **Live bugs found on the way:** the Conversations window *replaced* its
  `.sk-floater` class (losing the skinned frame) instead of adding its own; a
  combo near a window's right edge opened its list off screen; several
  observers panicked when a session resource was absent (radar, phototools,
  experiences); the group picker's "your groups only" mode was not what its
  specimen showed.
- **The gallery renders material spheres** (the Build window's PBR swatch, the
  material editor, the texture picker's material pane): it installs the
  material-preview studio.
- **Why the gallery got slow, and the viewer with it:** building every
  window's real content up front multiplied the text fields, and Bevy 0.19
  flagged every `EditableText` changed every frame, so the whole UI was
  re-laid-out every frame (~380 ms a frame in the gallery). Fixed in the Bevy
  fork (`de3251e1`): ~25 ms. The rich-text widget, which handed its editor
  inline boxes and styles behind `bypass_change_detection`, now flags the
  field itself when that model changes; a testkit test holds an idle field to
  "never flagged" so a Bevy bump cannot bring it back unseen.

Follow-ups filed: [[viewer-skin-floater-controls-outside-the-skin]],
[[viewer-floater-buttons-ignore-keyboard-activate]],
[[viewer-i18n-floater-literal-english]],
[[viewer-floater-font-size-threading]],
[[viewer-day-cycle-strip-click-mirrored-rtl]],
[[viewer-rlv-console-lines-wrap]],
[[viewer-floaters-decoupled-from-the-session]] (a specimen is built once;
the live window's reactions are its plugin's systems, which read the session).
