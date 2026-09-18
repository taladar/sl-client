---
id: viewer-audit-binary-module-extraction
title: About 15k lines still in the viewer binary map onto existing feature crates
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 8
refs: [build-split-viewer-crate, viewer-audit-debug-cli-affordances]
---

Context: [context/viewer.md](../context/viewer.md).

**Done 2026-09-18.** `sl-client-bevy-viewer/src/` went from 47,760 lines in 45
files to 33,589 in 30 — 14,171 lines into eight other crates, two of them new.
No behaviour change: verified by a normalised token diff of every moved file
against `git show HEAD:<file>` (see the `sl-client-verify-mechanical-refactor`
memory), which came back empty but for the `#[derive(Debug)]`s the
`missing_debug_implementations` lint requires on a newly-`pub` plugin type, the
visibility changes below, and the three deliberate interface changes.

## Where each module went

| Module | Lines | Now in |
| --- | --- | --- |
| `teleport_progress` | 820 | `sl-viewer-places` |
| `web_floater` | 504 | `sl-viewer-media` |
| `hover_tooltip` | 766 | `sl-viewer-world-view` |
| `media_controls` | 965 | `sl-viewer-world-view` |
| `asset_blacklist` | 816 | `sl-viewer-world-avatar` |
| `avatar_render_floater` | 928 | `sl-viewer-world-avatar` |
| `load_url` | 748 | `sl-viewer-notices` |
| `inspector_popup` | 1006 | `sl-viewer-notices` |
| `slurl_dispatch` | 859 | `sl-viewer-places` |
| `avatar_menu` / `object_menu` / `attachment_menu` / `land_menu` | 4751 | **new** `sl-viewer-ui-context-menus` |
| `gallery` / `render_gallery` | 1984 | **new** `sl-viewer-gallery` |

Three small types moved to unblock the rest: `LocalTimeZone` out of
`snapshot_floater` into a new `sl-viewer-platform::local_time` (the audit called
this one), and `DispatchSlurl` into `sl-viewer-intents`, which is what breaks
the apparent cycle between the inspector popup (writes it) and the SLURL
dispatcher (reads it) — they could then go to different crates.

## Where the audit's reading had gone stale

Written 2026-08-26, before the world-API split moved several modules; three of
its placements no longer held.

- **`hover_tooltip` to `sl-viewer-world-objects`, `media_controls` to
  `sl-viewer-media`.** Both now went to `sl-viewer-world-view` instead. The
  tooltip's pick arbitration is `gpu_pick` + `hud_pick` and the control bar
  drives `media_prim` and the camera focus that frames it — all four live in the
  view crate now, and `sl-viewer-media` is deliberately closed machinery with
  nothing pointing upward. The two surfaces follow what they read rather than
  what they draw; `sl-viewer-world-view` picked up `sl-viewer-ui-core` /
  `-ui-widgets` for the first time, and its module doc says why.
- **`ui_elements` as harness code.** It is not: `ELEMENTS` names three dozen
  feature modules plus surfaces that are still in the binary, exactly like
  `REGISTRARS` and `floaters::FLOATERS`. All three stayed, and
  `sl-viewer-gallery` takes the two registries — and the resolved `AssetPlugin`,
  whose development fallback is a compile-time `CARGO_MANIFEST_DIR` — as
  arguments instead. That is the one real interface change, and it is the point:
  a gallery can no longer reach into the binary's private module tree.
- **`menu_bar` "could move with the widget".** It could when it was 1,020 lines
  importing four modules. It is now 1,853 lines naming 45, which makes it the
  same kind of thing as the registries. It stays, correctly.

## What else changed

- The two gallery **binaries keep their names and their crate**
  (`cargo run --release --bin sl-client-bevy-viewer-gallery` is unaffected):
  `src/bin/` now holds two ~20-line shells that gather the four composition
  pieces — registries, asset root, tracing — and hand them over.
- `asset_root`, `floaters`, `ui_elements` and `init_tracing` turned `pub` for
  those shells; the rest of the binary's module tree is still `pub(crate)`.
- `sl-cef` left the viewer's direct dependencies (it was `web_floater`'s); the
  CEF runtime files are still copied, via `sl-viewer-media`.
- Three `SystemParam` structs the tooltip keeps to itself went the other way,
  from `pub(crate)` to private.
- Eight `ready/` task files had their `sl-client-bevy-viewer/src/*.rs` paths
  repointed.

## Deliberately not done

The 22 CLI options — 10 of them debug affordances in the shipping `--help` —
are now [[viewer-audit-debug-cli-affordances]] in `deferred/`. Both shapes for
it cost more than they look like they cost, and neither belongs in a commit
series about the module tree; the `harness` feature gate in particular would
add a feature to the heaviest crate in the workspace, and the ggh pre-commit
hook builds a **feature powerset**.
