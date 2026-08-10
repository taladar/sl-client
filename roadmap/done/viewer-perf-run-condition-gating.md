---
id: viewer-perf-run-condition-gating
title: Gate idle systems with run conditions (pause off-screen/inactive work)
topic: viewer
status: done
origin: performance survey of the implemented viewer (2026-07-22), user request
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

The systemic finding of the 2026-07-22 performance survey: the viewer runs
several hundred `Update` systems every frame, and across the whole 99k-line
`sl-client-bevy-viewer` crate there are only **9 `run_if` occurrences**
(7 of them the keyboard gate in `lib.rs`, 1 in `nearby_chat_bar.rs`, 1 in
`skin.rs`). Everything else relies on an internal early-return — which
still pays scheduler dispatch, system-param fetch (resource/query +
archetype access checks), and change-tick bookkeeping every frame for
every system. At 300+ systems × 60 fps that is a fixed idle floor that
scales with system *count*, not with activity.

The strategy (user suggestion): systems whose subject is not currently on
screen or not currently active should not run at all — closed floaters'
refresh systems, camera-mode drivers outside their mode, debug/demo
affordances in normal sessions, world-streaming systems before login
completes.

## Inventory of ungated clusters

- **World/session/appearance/animation/environment clusters**
  (`lib.rs:1043`, `1219`, `1303`, `1344` blocks): all run from frame 0,
  before the circuit is even up. Gate on `resource_exists::<SlState>` /
  an `agent_in_world` condition.
- **Event-fed systems** that only act on drained messages
  (`ingest_environment`, `capture_login_outcome`, look-at/point-at
  receivers): gate on `on_message::<SlEvent>()` /
  `on_message::<TextureDecoded>()` etc. **Caveat (2026-08-10, the
  budgeted-drain pass):** several former candidates now carry cross-frame
  backlogs and must NOT be gated on messages alone — `update_objects`
  (`PendingObjectEvents`), `apply_object_meshes` / `apply_object_sculpts`
  (`PendingDecodedMeshes` / `PendingDecodedSculpts` +
  `GeometryApplyBudget`), `drain_patch_rebuilds`, `drain_skeleton_merge`,
  `apply_avatar_appearance` (its debounced `appearance_pending` ledger),
  and `layout_tag_text` (its budget-deferred retry queue). A gate for
  those must also fire while their backlog is non-empty. `drive` itself is
  now a thin channel pump to the session network thread
  ([[viewer-perf-session-network-thread]]) — cheap, but still gateable on
  the link existing.
- **Camera-mode drivers** (`camera.rs:454-466`): `orbit_third_person`,
  `aim_look`, `drive_flycam` all run every frame; two of three
  early-return (flycam fetches 11 params for nothing). Make `CameraMode`
  a Bevy `States` and use `run_if(in_state(..))`, or
  `run_if(resource_equals(..))`.
- **Debug/demo systems** registered unconditionally:
  `log_suspicious_objects` (`objects.rs:825`), `focus_camera_on_particles`
  / `focus_camera_on_volume_shape` (`lib.rs:1281,1287`),
  `toggle_volume_morphs`, `repeat_debug_animation` (`session.rs:79`), the
  text/text-input demo systems (`lib.rs:1255-1266`). The pattern to copy:
  `capture_screenshots` is registered **only when `--screenshot-dir` is
  set** (`lib.rs:1432-1437`). Register env/flag-driven debug systems
  conditionally the same way.
- **UI per-frame refreshers that run while their panel is closed**
  (bounded work, but pure waste): `update_status_readouts` /
  `update_parcel_icons` (`status_bar.rs:299-300`), `refresh_people`
  (`people.rs:1171`), `refresh_conversations` tab-label/blink work
  (`conversations.rs:852`), `update_gear_conditions` (`inventory.rs:170`).
  No panel refresh system anywhere is gated on its floater being open —
  the UI layer relies purely on change-detection gating.

## The gate idiom for UI panels

A run condition keyed on the floater's `UiPanelShown(true)` (an
exists/any query over the panel entity), plus a **one-shot forced refresh
on the open transition** so a panel opens up to date —
`refresh_inventory_on_show` (`inventory.rs:1720`,
`Changed<UiPanelShown>`) is the existing in-tree shape to standardize.
Other patterns already present and worth copying: `skin.rs:218`
(`run_if(resource_changed::<SkinSelection>)`), the `virtual_list.rs:257`
zero-viewport-height early-out (closed virtualized lists already cost
only a size read).

## Estimated impact

Medium. This does not reduce worst-case busy-scene cost, but removes a
meaningful fixed floor (hundreds of no-op dispatches per frame, some with
large param fetches) — most visible on idle scenes, at the login screen,
and on low-end machines — and it is the enabling refactor for the
targeted gating tasks ([[viewer-perf-inventory-view-visibility-gate]],
the status-bar throttle in [[viewer-perf-frame-churn-cleanups]]).
Per-system dispatch overhead is small (µs-scale), so measure with the
[[viewer-profiling]] Tracy setup (zone statistics show per-system
dispatch counts and self-time directly) before/after; a good first
milestone is "no system with zero work done appears in the frame trace".

Confidence: high on the inventory (verified against `lib.rs` and each
cited registration); medium on the total ms saved until profiled.

## Done (2026-08-10)

Scoped pass, everything gated with run conditions or conditional
registration; the reusable idiom is `floater_shown(id)` from
[[viewer-perf-inventory-view-visibility-gate]].

- **UI panel gates**: `refresh_conversations`, and (people pane, hosted
  in the conversations floater) `rebuild_friends_view` + `refresh_people`
  run behind `floater_shown(CONVERSATIONS_FLOATER_ID)`. Ingest / open /
  close / tab-spawn / presence-toast systems stay ungated, so IMs and
  friend presence keep flowing while closed; the revision latch and
  change ticks catch up on open. `update_parcel_icons` (status bar is
  always visible, so not floater-keyed) gates on a new
  `parcel_icon_inputs_changed` condition — `SlAgentParcel` change
  (written upstream only on a real diff), current-region identity
  change, or a freshly added icon.
- **Camera-mode drivers**: `orbit_third_person` / `aim_look` /
  `drive_flycam` run behind `resource_equals(CameraMode::...)`; internal
  early-returns keep the `context.is_world()` half. `focus_on_object`
  stays ungated (its movement-resets-focus branch applies in every
  mode). The Bevy `States` refactor was rejected: `NextState` applies
  between frames and would break the same-frame orbit-to-mouselook
  zoom-through.
- **Debug/demo systems** moved to conditional registration (the
  `capture_screenshots` pattern; predicates mirror each system's own env
  check): `log_suspicious_objects`, `log_avatar_interest_census`,
  `dump_camera_pose`, `focus_camera_on_particles`,
  `focus_camera_on_volume_shape`, `log_pose_gate_churn`,
  `spawn_notification_demo`, and `repeat_debug_animation` (CLI-driven:
  registered only when `--repeat-animation` + `--play-animation` were
  given). Key-toggled demos keep their cheap toggle systems and gate the
  appliers: `update_pipeline_overlay.run_if(pipeline_overlay_active)`
  (shown-or-just-changed, so the hide write still runs),
  `apply_text_demo_visibility` / `apply_text_input_demo_visibility` on
  `resource_changed`, `update_demo_value_readouts` on
  `any_with_component::<DemoValueReadout>`.

Deliberately out of scope, with reasons:

- **Pre-login/world-cluster gating** (the merged world/session block,
  sky/water, animation): no correct latch exists —
  `ViewerSession.agent_in_world` flips only when the own avatar object
  arrives (objects/terrain/environment stream earlier), `SlState` is
  private and always present, and a state gate over message consumers
  risks dropping 2-frame-expiry messages. Filed as
  [[viewer-perf-login-state-gate]], to land with the login screen where
  a real pre-login idle phase exists.
- **`on_message` gating of individual ingest systems**: a run condition
  is itself a param-fetching check costing about what the empty-reader
  early-out already costs; only whole-tuple gates would win, and the
  merged block interleaves pollers, simulations, and backlog drains with
  cross-tuple ordering — no clean group condition without risky
  restructuring.
- `layout_tag_text` (retry backlog in a `Local`, not observable by a
  condition) and the `drive` channel pump (OS channel, must keep
  pumping) stay ungated, as the task text already anticipated.

Verified on the local grid (release): cargo check/clippy/nextest clean;
a cold-login session runs none of the gated panel refreshers while their
floaters are closed (Tracy zone counts zero), and alternating
gated/ungated A/B runs (`SL_VIEWER_DISABLE_PANEL_GATE`) show identical
frame rate and Render medians — details in the
[[viewer-perf-inventory-view-visibility-gate]] commit. A separate
steady-state ~46 fps ceiling (present with and without these gates) is
filed as [[viewer-perf-steady-state-46fps-ceiling]].
