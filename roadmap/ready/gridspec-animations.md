---
id: gridspec-animations
title: Animations as each grid broadcasts them
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-agent-movement, gridspec-sit-stand]
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
