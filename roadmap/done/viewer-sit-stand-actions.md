---
id: viewer-sit-stand-actions
title: Sit / stand actions
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-sit-stand
blocked_by: [viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

**Largely completed** by [[viewer-sit-target-and-stand-button]] (done
2026-08-05, verified on aditi): sit-target placement (parenting + hips drop),
the scripted sit camera, `ForceMouselook` drop-in / restore-on-stand, the Stand
affordance, the "Sit Here" / ground-sit / stand actions, and the no-sit-target
offset. What may remain here is any residual polish (e.g. sit-animation nuances,
unsit-on-teleport edge cases) — audit against that task before reopening scope.

Sitting and standing: sit on an object (click / context "Sit Here"), ground-sit,
and stand up — driving the sit-target placement, the sit animation, and the
camera adjustment, then releasing on stand. Sitting on an object needs a
**picked object** ([[viewer-object-selection-core]]). The sit request is
permission- and target-aware (scripted sit targets, unsit on teleport).

Also honour `ForceMouselook` on the `AvatarSitResponse` `SitTransform`
(decoded, [[protocol-38]]): when a scripted sit target forces mouselook,
drop the camera into mouselook on sit and restore it on stand
(script-interface survey 2026-07-23).

Sit state is already modelled (`SitState` in `sl-proto`); this task is the
viewer action + UI (the stand button, the sit context action) and the pose /
camera handling.

Reference (Firestorm, read-only): `llagent` (sit / stand handling),
`llmoveview` (stand button), `llviewermenu` (sit context action); messages
`AgentRequestSit` / `AgentSit` / `AgentSitOnGround` and the `AvatarSitResponse`
reply.

Builds on: `SitState` (`sl-proto`), the animation playback (`animations.rs`),
and the camera system.

## Parity-audit addendum (2026-08-19)

When this lands, also register `toggle_sit` as a bindable `Action` in
the input action map (the reference exposes it in the rebind UI), so a
key can sit/stand; the reference default chord is Alt+Shift+S.

Parity-audit extension: a block-left-click-sit option
(`FSBlockClickSit`, Firestorm tab; modes off / on / on-unless-scripted)
so a stray click on click-to-sit furniture doesn't seat the avatar.

## Status (2026-09-24): the residue

The sit itself is done ([[viewer-sit-target-and-stand-button]]). What this file
still carries is the two parity-audit additions, neither of which is in the
code:

- `toggle_sit` as a bindable `Action` in the input action map (the reference's
  Alt+Shift+S).
- The block-left-click-sit option (`FSBlockClickSit`: off / on /
  on-unless-scripted).

## Done (2026-11-03)

The residue landed, together with [[viewer-left-click-click-actions]] (which
the block option needed — a left click did not sit at all before it):

- `Action::ToggleSit` in the input action map: stand up when sitting (on an
  object or the ground), sit on the ground when standing. Bound to no key by
  default, as the reference's `toggle_sit` is not in its `keys.xml`; the
  rebinding UI ([[viewer-input-rebinding-ui]]) is where a key gets bound.
- The reference's `Alt+Shift+S` is a **menu accelerator** there, not a
  binding, so it is one here too: Avatar ▸ Movement ▸ **Sit Down** / **Stand
  Up**, the two drawn one at a time (`Self.ShowSitDown` / `ShowStandUp`), on
  one chord. The self pie, the menu and the action share `sit_down` /
  `stand_up` in `avatar_menu`.
- Block left-click sitting: `FSBlockClickSit`, a checkbox on the camera &
  movement preferences tab. **Not three modes**: the reference's setting is a
  plain boolean (Firestorm's `settings.xml`, `lltoolpie.cpp`), so the
  off / on / on-unless-scripted guess above was dropped for its real shape. A
  blocked sit click falls through to pay and then to the touch, as the
  reference's `switch` does.
