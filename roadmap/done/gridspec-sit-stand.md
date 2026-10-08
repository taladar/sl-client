---
id: gridspec-sit-stand
title: Sitting and standing: placement, refusals and alerts on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-sit-stand-actions, gridspec-logout,
  gridspec-seated-crossing, gridspec-neighbours-crossing,
  server-world-sit-and-attach, gridspec-agent-movement, gridspec-animations]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-08)

Measured and written up in `book/src/gridspec/movement.md` § Sitting.

- **Discover.** A new `sit-stand` conformance case, two avatars: the first
  rezzes a half-metre cube on the ground beside itself and asks to sit on
  it from there, at a point off its centre, from the ground, from six spots
  it walks to and — once a script has given the cube a sit target — from
  two it flies to; the second asks for the seat while the first is on it;
  the first teleports from the seat; and it asks for an object the region
  does not have, and for one of a neighbouring region where one is in
  view. Four runs on aditi, five on OpenSim.
- **Why aditi "did not answer the sit".** It answers a scriptless seat from
  7.5 m and says nothing at all from 9.3 m or further — no response, no
  alert. `logout-seated` had rezzed its seat a metre and a half from where
  its flight to the build location was *reckoned* to end, and the avatar
  comes down up to eleven metres from there. The scene settling
  (`settle_scene_with_avatar`) now reports where the avatar stands, and
  `logout-seated` passed on aditi: the observer is sent the seat, the
  avatar's `KillObject` 0.6 s after the request and the seat again; the
  next login stands 1.04 m from the seat (`book/src/gridspec/session.md`).
- **Findings.** Both grids set `AutoPilot` on every response and have the
  avatar on the seat before an `AgentSit` could arrive. A seat with a sit
  target is answered from 52 m on Second Life and 40 m on OpenSim; OpenSim
  answers a scriptless one from 20 m too. The response's seat position is
  not where the avatar is put: a third of a metre low for a scriptless seat
  on Second Life, 0.35 m low (the script's own target) for a scripted one
  on OpenSim. A taken seat and a taken sit target are both shared without a
  refusal. Standing up puts the avatar 0.34 m in front of where it sat on
  Second Life and 0.65 m in front and 0.57 m up on OpenSim. A teleport
  unseats on both. A sit on an unknown object is `SitFailNotSameRegion` on
  Second Life and silence on OpenSim.
- **Fake grid.** A `SitPolicy` row per flavour: the alert or the silence
  for an unknown object, which position the response states, and where
  standing up puts the avatar (it used to go back to where it logged in).
  Its response now sets `AutoPilot`, as both grids do. `sit-stand` runs
  offline against both flavours for the legs that need no second resident,
  no script and no walk. Seating on the request rather than on the
  `AgentSit`, a scriptless seat's placement and the distance are
  [[server-world-sit-and-attach]]'s, which now carries the figures.
- **Viewer.** It already draws a seated avatar from the avatar's own
  update and reads the response for the camera alone, which is what both
  grids need. The session now ends a pending sit on OpenSim's two unnamed
  refusals, known by their wording (from its source; not provoked live). A
  new `e2e_sit` sits and stands the viewer's avatar through the pie against
  each flavour and checks the refusal of a vanished seat.
- **Not done here.** The exact distance Second Life stops at and what
  moves it; every refusal but the unknown object; a seat with a camera or
  forced mouselook, several sit targets, a moving or phantom seat, an unsit
  by script. The neighbour-region refusal was measured again on OpenSim
  (the same words, unnamed) and not on aditi, where no neighbour's object
  was in view of the build spot: the earlier measurement stands in the
  table. Second Life lists no built-in
  animation for a seated avatar ([[gridspec-animations]]).
- **The avatar that could not be walked** (below) is
  [[gridspec-agent-movement]]'s hold at a landing point, ended by
  `FINISH_ANIM`; it has nothing to do with sitting.

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
