---
id: test-e2e-sweep-environment
title: End-to-end tests for the environment editors and their gates
topic: test
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 8
refs: [test-e2e-live-verify-sweep]
---

Context: [context/automation.md](../context/automation.md).

The environment windows were each unit-tested and never driven against a
grid. The fake grid serves `ExtEnvironment`, `UpdateSettingsAgentInventory`
and the settings assets, so each becomes a fake-grid test: open, edit, save,
and read back what the grid stored (and what the sky readout says).

- [[viewer-environment-fixed-editor]]: open, edit and save a sky / water
  settings item (`UpdateSettingsAgentInventory` plus the flags stamp).
- [[viewer-environment-day-cycle-editor]]: open, scrub, add a keyframe,
  save.
- [[viewer-environment-land-day-cycle-edit]]: Customize Day Cycle, edit a
  keyframe, Save, Apply — the inline cycle OpenSim uses.
- [[viewer-region-environment-panel]]: publish with region scope, day
  length and offset, then reset (aditi's per-track `trackno` path belongs
  to [[test-e2e-sweep-live-grid]]).
- [[viewer-environment-import-legacy-presets]] and
  [[viewer-windlight-bulk-import]]: import a `windlight/skies/*.xml` and a
  folder of them. Both go through the desktop's file chooser, so a test
  needs a way to answer it with a path (the chooser's own seam, not a
  second code path).
- [[viewer-environment-my-environments]]: drive the picker.
- [[viewer-environment-settings-unsupported-gate]]: a fake region that
  serves neither settings capability greys the entries — the one grid that
  can show it, since aditi and OpenSim both serve both.
- [[viewer-environment-personal-lighting]] and
  [[viewer-ui-virtual-trackball]]: the pickers, the fade, the trackball,
  and the parcel layer when walking between two parcels.

## Done

`sl-client-bevy-viewer/tests/e2e_environment.rs`, nine stage tests on the fake
grid, both backends, each reading back what the grid stored (the item, the
settings asset under it decoded, the environment a publish left):

- **Sky and water editors**: New Sky / New Water in My Environments, the
  grid's items stamped with their kind in their flags, Edit, a slider to its
  end, Save over `UpdateSettingsAgentInventory`, the stored asset holding it.
- **Day-cycle editor**: New Day Cycle, a press on the timeline moves the
  scrubber (Add Frame greyed on the midnight keyframe, live off it), Add
  Frame, a knob on the new keyframe, Save — the stored cycle has one more
  ground keyframe, where the timeline was pressed, holding the edit.
- **Region environment**: as estate manager, Customize Day Cycle opens the
  region's inline cycle, its Save hands the edit back, Apply publishes it with
  a week-long day and a −11.5 h offset (sent wrapped, 45 000 s), Use Default
  Settings, confirmed, is the `ExtEnvironment` DELETE.
- **Legacy import**: Import in the sky editor, the chooser answered with a
  WindLight preset, the name from the file (`Amber%20Dawn.xml` → "Amber
  Dawn"), Save As filing it holding the preset's haze; **bulk import** of a
  folder of two, each filed holding its own.
- **My Environments**: the name filter and the sky kind filter, Rename (the
  grid's item renamed), Apply Only To Myself (the local sky), Delete confirmed
  (the grid's item in the Trash).
- **The unsupported gate**: a region withholding both settings capabilities
  greys New Sky / Water / Day Cycle, Rename and Delete, the sky editor's Save
  and Save As (Import stays), and refuses a bulk import with
  `SettingsUnsuported`.
- **Personal Lighting**: a slider, the ambient swatch's colour picker, the sun
  trackball (a click aims it overhead, an arrow nudges it down), Reset
  confirmed; and **the cross-fade**: a transition time set in the debug-settings
  editor, a Legacy preset fades (the readout's `transition` present, then
  gone).

What it took:

- **A file chooser a test can answer**: a viewer with no window of its own
  holds an `OpenFileDialog` (`FileDialogBackend::Answered`) until the
  `AnswerFileDialog` request answers it — driver `answer_file_dialog`,
  `sl-viewer-ctl file-dialog PATH|--cancel`. The chooser's own seam: the same
  reply and the same remembered directory as the desktop's.
- **The fake grid answers `CreateInventoryItem`** (it ignored it, so New Sky,
  Save As and bulk import hung there) the way OpenSim does — a settings item
  starts as its kind's default with the subtype in its flags — and applies a
  **UDP `MoveInventoryItem`** (how a rename travels) to its tree.
- `RegionConfig::withheld_caps`; `FakeAgent::stored_asset`; the environment
  readout's haze, sun and `transition`.

The drive found two bugs, each fixed and held by the test that found it:

- **Two uploads through one capability overwrote each other on the fake
  grid.** `SimCaps` parked one upload per capability, so a bulk import's two
  saves left both items on the default sky. Each step 1 now gets an uploader
  URL of its own (`upload/<ticket>`), as OpenSim mints one per request
  (`concurrent_uploads_through_one_cap_stay_apart`).
- **My Environments' Delete and Rename acted on the row a closed context menu
  had been opened over**, not the selected one: the menu target outlived the
  menu. A press on a row now resets it.

**The previews** (asked for on review, 2026-10-01): every control of every
environment window has to move what is drawn, and Revert, Reset and closing
have to undo it.

- `sl-viewer-environment/src/preview_harness.rs` finds every slider, colour
  swatch, texture swatch and trackball a window draws by the names the rows
  give them, drives each — sliders and trackballs by the keyboard through the
  real focus and input stack, swatches by their picker's reply — and checks
  the sky or water `EnvironmentState` draws then holds the control's value. A
  control of the window it does not drive is a failure of its own, so a new
  knob cannot slip past it (the trackballs did, until that check). Tests: the
  sky and water editors and the day-cycle editor (each control, Revert
  previews the frame as opened and re-seeds every widget, closing drops the
  preview) and Personal Lighting (each control, Reset confirmed hands the sky
  back). Breaking one `dirty` flag in the water editor fails all eleven water
  knobs by name.
- The e2e tests check the same live: an editor previews its frame the moment
  it opens (`previewing`), a knob moves the drawn sky or water before anything
  is saved, the sky editor's Revert moves it back, the day-cycle editor's
  preview follows the scrubber between keyframes, and closing ends it — the
  environment readout gained the drawn water and the windows previewing.

The sweep found a third bug: **the edit layer had one slot per track for every
editor**, so with the day-cycle editor open, closing the sky editor wiped the
day editor's sky preview and drew the region's sky while the day editor still
showed its frame. `EnvironmentState`'s edit layer is now per previewing window
(`EditPreviewer`, the floater id), the one written last on top; closing a
window takes only its own tracks out
(`closing_the_top_preview_shows_the_one_beneath`,
`closing_the_sky_editor_leaves_the_day_editor_previewing`).

Left over, filed: the parcel layer of
[[viewer-environment-personal-lighting]] — walking between two parcels — needs
the fake grid to push the parcel an agent walks onto and a way for the driver
to hold a key ([[test-e2e-environment-parcel-layer]], blocked on
[[server-fake-grid-parcel-on-movement]]).
