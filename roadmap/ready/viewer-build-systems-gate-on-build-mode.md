---
id: viewer-build-systems-gate-on-build-mode
title: Performance — gate all build-tool systems on build mode being active
topic: viewer
status: ready
origin: user request (2026-07-24) — performance follow-up to the build-tool work
refs: [viewer-object-edit-floater-shell, viewer-prim-parameter-editing,
  viewer-prim-texture-editing, viewer-edit-face-selection]
---

Context: [context/viewer.md](../context/viewer.md).

The build tool's `Update` systems run **every frame regardless of whether build
mode is active**. Most bail early on `!tool.active` (or `ui` being absent), but
they still get scheduled, fetch their resources / queries, and iterate — dead
cost on the overwhelming majority of frames a user is *not* editing.

Gate the whole build-system set so it only runs while build mode is active
(and, where relevant, only while the Build floater is shown):

- Add a **run condition** (`run_if`) keyed on `EditToolState::active` (and/or
  the floater's shown state) to the build systems in `edit_tool`, `edit_params`,
  `edit_texture`, `edit_selection`, `edit_link`, `gizmos`, and the face-cursor /
  align / preview systems — rather than each system re-checking `tool.active`
  in its body.
- Audit which systems must still run when inactive (e.g. the `Ctrl+B` toggle
  itself, and any that must *clean up* on the active→inactive edge — the
  selection clear, the live-preview revert, the widget reset). Those either stay
  ungated or move behind an `on-exit` edge trigger.
- Confirm the gizmo picking / draw, the numeric-field sync, the texture-preview
  driver, and the face-cursor highlight all stop doing work when not building.

Measure before / after (system count and frame time via the F-key overlays /
the headless harness) to confirm the win and catch anything that silently
depended on running while inactive.

The per-frame early-returns already in place make this low-risk: the run
condition just hoists the `!tool.active` check out of every system body into the
scheduler.
