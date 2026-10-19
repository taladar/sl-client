---
id: gridspec-animations
title: Animations as each grid broadcasts them
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-agent-movement, gridspec-sit-stand,
  viewer-stop-animating-me, server-fake-grid-agent-avatars-shared]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Play / stop complete on both grids with no divergence; the fake grid does not
decode `AgentAnimation`.

From [[gridspec-agent-movement]] (2026-10-08): Second Life's statements of
an avatar's own set carry two to five assets beside the locomotion state
that are no built-in (`301d5e80-…`, `a22f396a-…`, `dccb493a-…`,
`efc5dc57-…`, `22c296f5-…`, `1f6b3798-…` among them), changing from one
statement to the next, and on both aditi avatars a fall, its landing, a
hover and level flight were stated *only* as such assets (`46b54ef6-…`,
`8e6b62ad-…`, `b6abe991-…`). Whether those are the avatars' own animation
overrides or something the simulator adds is for this task to find out.
OpenSim states exactly one built-in at a time.

From [[gridspec-sit-stand]] (2026-10-08): the same on a seat. Second Life
stated a seated avatar's set as three to five such assets and no built-in
`sit` (on the ground it did state `sit_ground_constrained`, beside them);
OpenSim stated `sit`, and `sit` then `stand` on standing up.

## Discover

`animation_play_stop` plus a two-avatar observer leg; server-added built-ins;
`AnimationSourceList`.

## Document

`book/src/gridspec/avatars.md` § Animations.

## Fake grid

Small in this task: decode, per-agent set, echo; the observer broadcast via
[[server-fake-grid-agent-avatars-shared]].

## Viewer

Stop as drop-out.

## Done (2026-10-19)

The `animations` census (2av, all four grids; the observer and script legs
live only) starts and stops `clap`, twice each, two in one message and a swap
in one message, a made-up id, a one-shot left for fifteen seconds, `stand` and
`walk` by hand, typing, the away control and the nil-id reset, and a probe
script's `dance2` (granted, started, stopped, stopped by the agent, its cube
deleted while it plays) (`book/src/gridspec/avatars.md` § Animations).
Findings:

- The non-built-in assets in a Second Life avatar's set are **the test
  avatars' own animation overrider**: sourced to an attachment, plus one asset
  with a nil source that takes `stand`'s place as the default posture (an
  `llSetAnimationOverride`). The nil-id reset clears that override.
- Neither grid adds `type` or `away`, drops a one-shot by itself, or stops a
  script's animation when its object is deleted; both start a made-up id.
- OpenSim states the set after every request (a no-op too); Second Life only
  after a change. Second Life names a source for every entry to the agent
  itself (nil for the posture, the agent, the object) and only the first
  entry's (nil) to another viewer, and carries one empty
  `PhysicalAvatarEventList` block; OpenSim names only objects' sources, those
  entries first, and no physical block.
- The overrider's churn makes Second Life's restatements, sets, order and
  sequence relations unholdable; each leg holds its own animations only.

Fake grid: small, done — `sl-fake-grid/src/animations.rs` keeps each session's
set and states it back per `ImitatedGrid::animation_policy`; `SimSession`
decodes `AgentAnimation` (`ServerEvent::AgentAnimation`) and gained
`send_avatar_animation_with_events`. `animations` and `animation-play-stop`
run offline on both flavours. The observer broadcast waits on
[[server-fake-grid-agent-avatars-shared]], given the findings.

Viewer: it already reads a statement as the whole set (drop-out, restart by
sequence number) and stops its own run-out one-shots; a new e2e test
(`e2e_two_avatars`
`the_own_animations_are_stated_back_as_each_grid_states_them`) holds the round
trip on both fake flavours. *Stop animations* is missing:
[[viewer-stop-animating-me]].
