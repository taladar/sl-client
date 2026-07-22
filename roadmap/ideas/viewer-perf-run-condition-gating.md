---
id: viewer-perf-run-condition-gating
title: Gate idle systems with run conditions (pause off-screen/inactive work)
topic: viewer
status: ideas
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
  (`update_objects`, `apply_object_meshes`, `apply_object_sculpts`,
  `ingest_environment`, `capture_login_outcome`, `drive_session`,
  look-at/point-at receivers): gate on `on_message::<SlEvent>()` /
  `on_message::<TextureDecoded>()` etc.
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
