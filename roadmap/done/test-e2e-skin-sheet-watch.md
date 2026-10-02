---
id: test-e2e-skin-sheet-watch
title: End-to-end check that --watch-skins picks up an edited sheet
topic: test
status: done
origin: test-e2e-sweep-single-viewer-ui (2026-10-01)
points: 2
refs: [viewer-preferences-colors-skins-tab, test-e2e-sweep-single-viewer-ui]
---

Context: [context/automation.md](../context/automation.md).

[[viewer-preferences-colors-skins-tab]]'s last live check: a viewer run with
`--watch-skins` re-dresses itself when a skin's `.css` changes on disk. The
stage starts every viewer on the workspace's own asset tree, which a test
must not edit, so it needs a way to run a viewer on a copy:

- a stage option giving a viewer its own asset root — `BEVY_ASSET_ROOT` for
  a process, the asset plugin's path in process — and `--watch-skins`
  (`SkinRuntime::watch`);
- the check: copy the tree, start the viewer on it, change a token the model
  can read (a `--chat-self` the colour swatch shows, a text colour), and the
  swatch or the ink follows without a restart.

## Outcome (2026-10-02)

- `StageBuilder::watch_skins()` gives every viewer a copy of the asset tree
  and `--watch-skins` on both backends; `Stage::assets(label)` names the
  copy. In process the base is `ViewerAppOptions::assets` (new; the asset
  plugin's stated root), for a process `BEVY_ASSET_ROOT`.
- `e2e_preferences::an_edited_skin_sheet_redresses_a_watching_viewer`:
  graphite's `--chat-self` edited in the copy, the Preferences swatch
  follows with no restart, on both backends.
- The check found `--watch-skins` (and the gallery's always-on watch) had
  never worked: Bevy's `file_watcher` feature was not enabled, so the
  override was accepted and nothing watched. The viewer crate now enables
  it.
