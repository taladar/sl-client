---
id: server-lsl-lib-avatar-control
title: Library tranche — avatars, animation, sitting, controls and camera
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-agent-movement,
  server-world-sit-and-attach]
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
