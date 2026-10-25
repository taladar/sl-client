---
id: server-lsl-lib-avatar-control
title: Library tranche — avatars, animation, sitting, controls and camera
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-agent-movement,
  server-world-sit-and-attach, gridspec-lsl-avatar-control]
refs: [server-lsl-lib-money-permissions, server-lsl-lib-detection-sensors,
  server-fake-grid-scripted-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

Everything a script does *to* or *about* an avatar. Most of it is gated
by `llRequestPermissions` ([[server-lsl-lib-money-permissions]]), and
most of the wire senders already exist.

- **Queries**: `llGetAgentSize`, `llGetAgentInfo` (the `AGENT_*`
  bitfield — walking, flying, in-air, typing, sitting, away, mouselook,
  script-controlled), `llGetAgentLanguage`, `llGetAgentList`,
  `llKey2Name`, `llGetDisplayName`, `llGetUsername`,
  `llRequestAgentData` / `llRequestDisplayName` / `llRequestUsername`
  answering over the `dataserver` event, `llGetObjectDetails` on an
  avatar key.
- **Animation**: `llStartAnimation`, `llStopAnimation`,
  `llGetAnimation`, `llGetAnimationList`, `llStartObjectAnimation` /
  `llStopObjectAnimation` (animesh — the fake grid already models it as
  `ObjectAnimationFixture` and has `send_object_animation`),
  `llSetAnimationOverride` / `llResetAnimationOverride` /
  `llGetAnimationOverride`. `send_avatar_animation` exists.
- **Sitting**: `llSitTarget`, `llLinkSitTarget`,
  `llAvatarOnSitTarget`, `llAvatarOnLinkSitTarget`, `llUnSit`,
  `llSetSitText`, `llSetTouchText`, `llForceMouselook`,
  `llSetCameraEyeOffset` / `llSetCameraAtOffset` (which `sl-proto`
  already carries on the sit response), `llSitOnLink`.
- **Controls and camera**: `llTakeControls`, `llReleaseControls`,
  `llSetCameraParams`, `llClearCameraParams`, and the
  `control(key id, integer level, integer edge)` event.
  `send_script_control_change`, `send_set_follow_cam_properties` and
  `send_clear_follow_cam_properties` all exist; the client mirrors the
  grant and takes no autonomous action (`PermissionRole::Cooperation`),
  so the viewer half is the one place a test has to look at the viewer.
- **Moving an avatar**: `llTeleportAgent`, `llTeleportAgentHome`,
  `llTeleportAgentGlobalCoords` (the grid's teleport machinery already
  exists and the client already treats a scripted teleport as an
  ordinary one), `llPushObject` on an avatar, `llMoveToTarget` on an
  attachment.
- **Attachments**: `llAttachToAvatar`, `llAttachToAvatarTemp`,
  `llDetachFromAvatar`, `llGetAttached`, `llGetAttachedList`, and the
  `attach(key id)` event.

Every one of these needs an avatar with a **circuit** behind it: an NPC
fixture can be sensed and named but cannot grant a permission, sit, or
be animated in a way anything observes. So the tests here run against a
second real session — a conformance case driving its own, or a scripted
avatar ([[server-fake-grid-scripted-avatars]]).

Acceptance: a scripted pose ball seats an avatar, plays an animation on
it and reports the sitter; `llTakeControls` after a granted permission
routes the viewer's movement keys to the script's `control` event
(checked live in the Bevy viewer); and a scripted teleport moves the
agent through the ordinary teleport phases.

## Measured (gridspec-lsl-avatar-control, 2026-10-24)

Build to `book/src/gridspec/lsl.md` § Animations, sitting, controls and
camera, from the `avatar/*` probes' results on aditi, YEngine and XEngine
(which agree with each other everywhere). The rows a flavour must split on:

- `llRequestPermissions(agent, 0)`: Second Life does nothing, OpenSim
  releases and fires `run_time_permissions(0)`.
- `CONTROL_CAMERA` from an object the avatar neither wears nor sits on:
  Second Life refuses unasked (a `DEBUG_CHANNEL` complaint, no question, no
  event); OpenSim asks.
- A call made without its permission: Second Life complains on
  `DEBUG_CHANNEL`, OpenSim is silent; likewise `llStartAnimation` of a
  missing animation.
- The `ScriptControlChange` shapes (Second Life's release-all and nudge bit,
  accept-off as the complement), the follow-camera rule order, the
  sit-target response offset (0.35 m), `llGetLinkKey` past a lone prim
  (`NULL_KEY` against the prim's key) and `llGetAnimationOverride` after a
  reset (`stand` against `Standing`).

Both grids honour an answer's mask as it came, more than was asked
included, and drop `TAKE_CONTROLS` and `CONTROL_CAMERA` when a sitter
stands.

## From gridspec-lsl-events (2026-10-25)

`events/state-drops` (`book/src/gridspec/lsl.md` § Events): a state change
keeps taken controls — the new state's `control` runs on a press and the
grid sends the viewer no `ScriptControlChange` — and keeps the permissions
and their key, on every grid.
