---
id: gridspec-sit-stand
title: Sitting and standing: placement, refusals and alerts on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-sit-stand-actions, gridspec-logout,
  gridspec-seated-crossing, gridspec-neighbours-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

A child-region sit is refused: SL with the named alert
`SitFailNotSameRegion`, OpenSim the same text unnamed. The fake grid seats at
a fixed offset and silently ignores unknown seats.

From [[gridspec-logout]] (2026-10-06): OpenSim sets `AutoPilot` on **every**
`AvatarSitResponse` (`ScenePresence.SendSitResponse` passes `true`), a seat
1.5 m away included. A logout while seated unseats there: the next login at
`last` stands 1 m from the seat.

**Aditi did not answer the sit at all**: three `logout-seated` runs rezzed a
0.5 m cube 1.5 m from the avatar (at its height, then at ground level), both
avatars saw it, and the `AgentRequestSit` for it drew neither an
`AvatarSitResponse` nor an alert before the session's sit timeout. Start
here: compare our `AgentRequestSit` with the reference viewer's on aditi
(`sl-conformance-trace`), and try a seat rezzed by somebody else and a seat
with a sit target. `logout-seated` passes on OpenSim and waits on this for
its Second Life half.

## Discover

A `sit-stand` case: sit with and without a sit target, from a neighbour, on
an occupied seat, on the ground; unsit at teleport / logout; both grids.

## Document

`book/src/gridspec/movement.md` § Sitting.

## Fake grid

Small refusals and fallback placement in this task; the full model is
[[server-world-sit-and-attach]].

## Viewer

Named vs unnamed refusal alert, fallback seat position; `e2e` on both flavours.

## From gridspec-neighbours-crossing (2026-10-07)

The seated crossing waits on this task ([[gridspec-seated-crossing]]).

One more thing about an avatar on aditi that would not do as it was told,
which may or may not be the same thing as the unanswered sit: the third test
avatar, standing at its region's landing point (Ahern, about 8/10), could not
be **walked**. Ninety seconds of `AT_POS` in its `AgentUpdate`s — once a
second from `sl-repl`, four times a second from the conformance steering —
turned it to face where it was told and moved it two centimetres; its
animations stayed a stand and `ANIM_AGENT_LAND`. `FLY | AT_POS` from the same
spot moved it at once. The first avatar walks in its own region. Whether the
landing point holds an avatar, or the avatar was left in a state that does,
is not known.
