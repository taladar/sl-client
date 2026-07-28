---
id: viewer-build-systems-gate-on-build-mode
title: Performance — gate all build-tool systems on build mode being active
topic: viewer
status: done
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

## Done (2026-07-28)

One shared run condition gates the whole build-tool system set. Added to
`edit_tool.rs`:

- `edit_tool_active_or_settling(Res<EditToolState>, Local<u8>)` — true while
  `active`, then for a short **settling window** (`EDIT_TOOL_SETTLE_FRAMES = 3`)
  after it deactivates. The window is why a *run condition* works even though
  several build systems do their teardown *when inactive*: the teardown
  reconcilers (wire deselect, outline / face-cursor / gizmo despawn,
  texture-preview revert, create panel / cursor) depend on
  `mirror_floater_into_state` having cleared the selection **earlier the same
  frame**, and those systems live in other plugins with no cross-plugin ordering
  guarantee — a couple of extra frames let the edge propagate whatever order
  Bevy picked, then they stop being scheduled. The countdown is a pure
  `settle_tick` helper, unit-tested (`settle_window_runs_after_deactivation`).

Gated the `Update` systems of `EditToolPlugin` (all but the two **activation
drivers** — `toggle_build_floater_on_ctrl_b` and `mirror_floater_into_state` —
which stay ungated so `Ctrl+B` / the object-menu *Edit* action can still turn
build mode *on*), `EditParamsPlugin`, `EditTexturePlugin` (incl. the
align / preview / face-material-mode systems), `EditSelectionPlugin` (incl. the
face-cursor highlight), `EditLinkPlugin`, `EditGizmoPlugin`, and — beyond the
listed set, as a clean build-only extra — `EditCreatePlugin`.

Two systems are deliberately **not** gated:

- `apply_drag_hover_highlight` (`edit_selection`) — the inventory drag-drop
  hover outline is usable *outside* build mode (drop an item onto an in-world
  object), so it stays ungated; it owns a distinct `DragHoverOverlay` component,
  so pulling it out of the selection chain changes no behaviour.
- The whole `EditContentsPlugin` — its **Object Contents** floater opens
  standalone via right-click → *Open* without build mode, so its systems must
  keep running. (Its build-floater Contents-*tab* portion could be
  partial-gated later; left whole here on purpose — a per-surface split is more
  intricate than the win warrants.)

Behaviour is identical to before for the pure-work systems (they each already
bailed on `!active`, so running-then-bailing every frame vs. not being scheduled
is the same result); the only change is that the teardown reconcilers stop after
the settling window instead of reconciling a cleared state forever. Live-checked
on OpenSim: `Ctrl+B` opens/closes build mode, select shows the gizmo + outline,
close tears them down with no ghosts.

Not measured as a frame-time delta — on an idle scene the saved work is below
diagnostics noise, and Bevy 0.19 exposes no per-system scheduler count to read;
the win is structural (systems no longer scheduled at all outside build mode).
