---
id: viewer-audit-binary-module-extraction
title: About 15k lines still in the viewer binary map onto existing feature crates
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [build-split-viewer-crate]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy-viewer/src/lib.rs` is now ~300 lines of re-export shim (130
`pub(crate) use sl_viewer_*` aliases keeping old `crate::foo::` paths resolving
— that part is healthy) plus a ~2400-line composition root. What is left in
`src/` is 27 real modules and ~19.4k lines, of which only ~1.2k (`REGISTRARS`,
`ui_elements::ELEMENTS`, `build_info`, `run_session`) genuinely needs to be the
composition root.

**Zero-blocker moves** (no new dependency edges):

- `teleport_progress.rs` (820) — imports only `ui`, `ui_font`, `world_api`; goes
  to `sl-viewer-places` or `sl-viewer-world-view`;
- `media_controls.rs` (980) + `web_floater.rs` (472) — every import already
  lives in `sl-viewer-media` / `-platform` / `-ui-*`; goes to `sl-viewer-media`;
- `hover_tooltip.rs` (738) — `gpu_pick`, `hud_pick`, `objects`,
  `name_tag_billboard`; goes to `sl-viewer-world-objects`;
- `gallery.rs` (1005) + `render_gallery.rs` (801) + `ui_elements.rs` (447) —
  declared `pub mod` with **no `#[cfg(test)]`**, unlike the sibling
  `render_test` / `ui_test` / `render_readback` / `settings_golden`. That is
  2253 lines of test-harness app compiled into the production library, beside a
  `sl-viewer-testkit` crate that already exists.

**One-type blockers**: `asset_blacklist.rs` (815) and `avatar_render_floater.rs`
(914) belong in `sl-viewer-world-avatar`; the sole obstacle is
`use crate::snapshot_floater::LocalTimeZone`, which should move to
`sl-viewer-platform`. `load_url.rs` (748) and `inspector_popup.rs` (1006) belong
in `sl-viewer-notices` next to `script_dialog` / `script_permission`.
`slurl_dispatch.rs` (851) belongs in `sl-viewer-places`.

**A new crate**: `avatar_menu.rs` (1746) + `object_menu.rs` (1344) +
`attachment_menu.rs` (1150) + `land_menu.rs` (369) are 4609 lines of pie-menu
entry trees that cross-reference each other — one cohesive unit, not
composition. They belong in a `sl-viewer-ui-context-menus` crate beside
`sl-viewer-ui-pie-menu` (which holds only the widget).

**Correctly here, for the record**: `bottom_toolbar.rs` fans into 13 feature
modules; that is composition. `menu_bar.rs` (1020, imports only
`floater`/`menu`/`ui`/`ui_element`) is not, and could move with the widget.

Also of note in the same crate: 22 CLI options, **10 of which are debug
affordances** whose own doc comments say so (`--camera-position`,
`--camera-look-at`, `--camera-spin`, `--camera-spin-axis`, `--screenshot-dir`,
`--play-animation`, `--repeat-animation`, `--replay`, `--replay-orbit-light`,
`--replay-reflection-probe`). Under the project's CLI rule those belong behind
one `--debug-*` subcommand or a `#[cfg(feature = "harness")]` gate, not in the
shipping `--help`.
