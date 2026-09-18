---
id: viewer-audit-plugins-own-their-schedule
title: Most viewer crates export loose systems instead of owning a plugin
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 13
refs: [viewer-audit-plugin-resource-registration, viewer-audit-system-ordering-claims]
---

Context: [context/viewer.md](../context/viewer.md).

Only `PhysicsPlugin`, `MediaPrimPlugin`, `CameraPlugin`, `InputActionPlugin`,
`InputContextPlugin`, `ParcelBordersPlugin`, `SlTonemapPlugin` and
`TransparencyOrderPlugin` exist. The rest of the world and feature crates export
loose `pub fn` systems, wired by a single ~900-line `add_systems` in
`sl-client-bevy-viewer/src/lib.rs:1927+` — whose comments repeatedly say tuples
are nested "to stay within Bevy's per-tuple system limit".

Two consequences:

- cross-crate invariants live in the **binary** rather than in the crate that
  owns them, which is directly why [[viewer-audit-plugin-resource-registration]]
  and [[viewer-audit-system-ordering-claims]] exist;
- no crate can be dropped into a test `App` on its own, which is a large part of
  why the viewer crates' test ratios are what they are.

`PhysicsPlugin` (`sl-viewer-world-view/src/physics.rs:124`) is the model: 14
systems with explicit, commented `.after()` / `.before()` edges.

Scope: give each crate a plugin that registers its own resources, systems and
ordering edges. The binary then composes plugins rather than systems.

## Done (2026-09-18)

Thirteen plugins, and the binary composes rather than schedules.

The world fold, which was the bulk of it:

- `sl_viewer_world_objects::WorldObjectsPlugin`
- `sl_viewer_world_scene::WorldScenePlugin`
- `sl_viewer_world_avatar::WorldAvatarPlugin`

and the surfaces `run_session` was still wiring by hand:

- `sl_viewer_world_view::session::SessionDriverPlugin` (the `SlEvent` fold, the
  draw-distance and interest-camera reports, the whole quit path, the exit save)
- `sl_viewer_settings::SettingsPersistPlugin`
- `sl_viewer_notices::notification_host::NotificationSourcesPlugin` — split from
  `NotificationHostPlugin` because the host must stay addable by an app with no
  session (the login-free gallery renders toast specimens with it)
- `sl_viewer_chat::chat::ChatOverlayPlugin`
- `sl_viewer_people::mutes::MutesPlugin`
- `sl_viewer_ui_core::ui_text::TextDemoPlugin`
- `sl_viewer_ui_widgets::ui_text_input::TextInputDemoPlugin`
- `sl_viewer_platform::ui_perf::UiLayoutGatePlugin`
- `sl_viewer_world_avatar::avatar_dump::AvatarDumpPlugin`

### How a cross-crate edge is stated now

Three shapes, and which one applies is decided by the tier, not by taste:

- **downwards, by name** — the avatar layer may name
  `sl_viewer_world_objects::objects::apply_object_meshes`, because that crate
  sits below it;
- **sideways, by phase** — `WorldPhase`, which is what `sl-viewer-world-api`
  exists to provide;
- **upwards, by set** — new:
  `sl_viewer_social::groups::GroupsSystems::Ingested`, so the world's name tags
  can wait for the group titles the People surface fills in without naming a
  crate above them.

`WorldScopedPlugin` took over the `Detect`-after-`SessionDrained` edge, which is
universal rather than the binary's.

### Registrations that went home

`LocalChatNotice` to its five writers, `RequestBlock` / `RequestFriendship` to
the avatar pie that raises them, `UiAction` to `ViewerUiPlugin` (the scaffold
every writer already requires), `AvatarControls` to the movement driver **and**
the locomotion reader, `HudState` to `HudScreenPlugin`, `RlvEnvironmentSlot` to
the environment fold that reads it. The idiom throughout: a system's own plugin
declares the reads it makes, `init_resource` being idempotent, so a host that
leaves the owner out still validates.

### Tests

The six pipeline-ordering tests moved next to the pipelines they constrain, on a
`ScheduleOrder` harness shared from `sl_viewer_world_api::schedule_order`; three
more were added for the terrain fold, the bake inputs and the attachment binds.

`viewer_plugins.rs` 1349 → 542 lines; `run_session`'s hand-wiring is gone.
