---
id: viewer-automation-accesskit-bridge
title: Feed AccessKit from the semantic UI model
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-semantic-custom-widgets]
refs: [viewer-a11y-screen-reader]
---

Context: [context/automation.md](../context/automation.md).

The semantic model and a screen-reader tree are the same information: role,
name, state, value, bounds. [[viewer-a11y-screen-reader]] plans to build the
latter; building it *from* the former means one audit of names and roles
serves both, and the automation sweep keeps the accessibility tree
complete for free.

## Wanted

- Emit `AccessibilityNode`s (role, label, disabled, toggled, expanded,
  selected, value, numeric range) from the model for every UI node; keep
  them updated incrementally rather than re-snapshotting every frame.
- Enable the Linux AT-SPI adapter (the cfg-guarded `accesskit_unix` stanza
  described in [[viewer-a11y-screen-reader]]).
- AccessKit exists only under a winit window: headless runs have none, and
  nothing in the automation path may read from it.
- This covers scope items 1–3 of [[viewer-a11y-screen-reader]] (adapter,
  label convention, custom-widget nodes); live regions, action handling and
  accessible text input stay there.

Acceptance: Orca under niri reads the menu bar, a floater's title and a
disabled button as disabled; a unit test checks the emitted nodes against
the model for a registry floater.

## Done (2026-09-30)

- **The bridge** (`sl-viewer-automation/src/screen_reader.rs`,
  `AccessKitBridgePlugin`, installed for `WindowMode::Windowed` only): the
  AccessKit tree is built from the semantic snapshot and pushed to the winit
  adapter directly, with Bevy's own per-frame push switched off
  (`ManageAccessibilityUpdates`). The model's children are the tree's (Bevy's
  push would hang a button off the window whenever a plain container sits
  between it and its floater); hidden nodes are left out; a floater is a
  `Dialog`, static text a `Label` read as its value, an unnamed group a
  `GenericContainer`. States: disabled, read-only, toggled (checkbox, radio),
  selected (tab, list and tree rows), expanded, level, keyboard shortcut,
  value / numeric value, bounds in physical pixels; a slider's range and step
  from the `AccessibilityNode` `bevy_ui_widgets` keeps on it.
- **Incremental**: only nodes that differ from the last update are sent; the
  model is read at most every 200 ms and at once on a focus change, and only
  while `AccessibilityRequested` holds. An inactive adapter forgets what was
  sent, so a new listener gets the whole tree. Headless: no adapter, nothing
  read; the automation path never reads AccessKit.
- **Linux adapter**: `bevy/accesskit_unix` enabled for `target_os = "linux"`
  in the viewer's manifest. It keys on `ScreenReaderEnabled` of
  `org.a11y.Status`, not `IsEnabled`.
- **Upstream bug, forked**: `accesskit_atspi_common` 0.18.1 (Bevy 0.19's)
  reported a disabled button `Enabled | Sensitive`, so it read as available.
  Upstream fixed it in #788 (0.21.0); backported to 0.18.1 in
  `github.com/taladar/accesskit` (branch `sl-client-atspi-disabled`) and
  `[patch]`ed in alone.
- **Names a screen reader can say** (a11y items 2–3):
  - what a stylesheet writes into `::before` / `::after` (every skin glyph)
    is no longer text in the model, so no control is named "✕" or "▾ 📂";
  - icon-only controls got names: floater Dock / Minimize / Close, tab-strip
    scroll arrows, the inventory's options and create menus, the emoji
    picker's category tabs (`TabSpec::names`), the colour picker's palette
    cells, the chat bar's emoji button and volume drop-down (now a combo box
    whose value is the volume), the volume bar's mute buttons, sliders and
    pulldown toggle;
  - slider rows in Preferences and Quick Preferences are `LabelledBy` their
    caption; an experience's owner link by its caption row;
  - unlanded bottom-toolbar entries are greyed buttons, not labels;
  - inventory rows are named by the item's name, not its icon and `…`.
- **Tests**: mapping, range and incremental-update teeth in
  `screen_reader/tests.rs`; the pseudo-element rule in `ui_model/tests.rs`;
  `automation_screen_reader.rs` checks every registered floater's emitted
  nodes against its model (roles, names, disabled, toggled, children, nothing
  hidden) and fails any control of any registered floater or element with no
  letter or digit in its name.
  Also named after the live check: the quick-preferences button, a
  conversation pane's close / add-participants, the media control bar, and
  the Events search's day steppers — glyph buttons on panels no fixture
  shows, so the sweep cannot reach them.
- **Live check** (local OpenSim; Orca is not installed on this machine, so the
  AT-SPI tree Orca would read was dumped through the `Atspi` GI bindings with
  `ScreenReaderEnabled` set): the menu bar and its eight menus; the
  Inventory floater as the dialog "Inventory" with Dock / Minimize / Close,
  its tabs (the active one selected), named menus, search field and a tree of
  rows named by folder; the bottom toolbar as buttons, the unlanded
  Appearance / People / Camera and the stream-less parcel-audio buttons
  **without** `enabled` / `sensitive` (disabled); the chat bar's field,
  "Insert emoji" button and "Chat volume" combo; the volume bar's named
  mute, slider and toggle. The Orca pass itself (speech) is still owed.
- **Coverage for what no fixture shows** (after review): the name sweep
  counts hidden controls too, which found 164 more unnamed ones (Build tab
  fields and swatches, Preferences and Phototools rows, scrollbar arrows,
  About Land / Region, Search, profile notes, group notice, alerts table) —
  all named now, mostly by a `LabelledBy` on the shared row spawner, the
  transform fields per axis ("Position X"). `UiLabel::Glyph` now carries its
  name key (`UiLabel::glyph`), so a glyph button cannot be written unnamed;
  and a `display: none` caption names nothing in the model.
- **Not done**: AccessKit action requests (a screen reader pressing a button)
  and live regions stay with [[viewer-a11y-screen-reader]].
- **Paired fields** (after review): fields under one caption are named by the
  caption and their part — "Offset U", "Path Cut Begin", "Force Z", "Spot
  Field of view", "Color Red" — through `NamePart` / `SpokenLabel` and the
  `labelled-part` Fluent pattern, which a translation may reorder. The About
  Region corner fields read "SW high", not "high" four times. Two guards: no
  two shown fields of any floater share a name, and no two fields of any
  Build tab (each opened in turn, a prim selected) do — the latter fails on
  all ten paired rows with the part naming switched off.
