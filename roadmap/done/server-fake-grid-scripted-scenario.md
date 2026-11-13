---
id: server-fake-grid-scripted-scenario
title: A scripts scenario — one prim per scripted behaviour
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-fake-grid-script-engine-wiring, server-lsl-lib-prim-state,
  server-lsl-lib-detection-sensors, server-lsl-lib-avatar-control,
  server-fake-grid-money-ledger, server-lsl-lib-task-inventory,
  server-world-collision-and-physics]
refs: [viewer-fake-grid-render-catalogue, test-fake-grid-lsl-offline-cases,
  test-firestorm-fake-grid-crosscheck, server-fake-grid-scripted-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

Blocked 2026-11-04: half the roster below calls functions no tranche has
implemented yet (the library implemented 198 of ~425 functions that day).
The timer prim needs `llSetPos` / `llSetColor` / `llTargetOmega` / `llSetText`
([[server-lsl-lib-prim-state]]), the sensor prim `llSensorRepeat`
([[server-lsl-lib-detection-sensors]]), the pose ball `llStartAnimation`
([[server-lsl-lib-avatar-control]]), the vendor a `money` event
([[server-fake-grid-money-ledger]]), the rezzer and the notecard reader
`llRezObject` and `llGetNotecardLine` ([[server-lsl-lib-task-inventory]]),
and the physical prim `llSetStatus` and collisions
([[server-lsl-lib-physics-vehicles]]). Touch, dialog, listen, sit target,
pay price, link messages and the broken-compile specimen run today.

`sl-fake-grid`'s `fixtures::scenarios` already offers `stock`,
`catalogue` (one prim per rendering feature plus an NPC),
`catalogue-eep` and `border`, each naming its **landmarks** so a camera
can be aimed at anything worth looking at, and `scripts/fake-grid.sh`
and `sl-crosscheck` both take a scenario by name. A `scripts` scenario
is the same idea for behaviour instead of rendering: one prim per
scripted thing, each at a named landmark, each with a script in its task
inventory.

The roster, roughly one per library tranche, so the scenario doubles as
the coverage demo:

- a **touch** prim that says who touched it, which face and where;
- a **dialog** prim that menus the toucher and reacts to the button;
- a **listen** prim that answers a phrase on channel 0 and a command on
  a hidden channel;
- a **timer** prim that moves, spins (`llTargetOmega`) and changes
  colour on a cycle — the one a cross-check frame pair can compare;
- a **sensor** prim that reports avatars entering and leaving its cone;
- a **sit** prim (a pose ball) that seats, animates and reports its
  sitter;
- a **vendor** prim with a pay price that raises `money`;
- a **rezzer** prim that rezzes a scripted child with a start parameter;
- a **notecard reader** that reads its own contents over `dataserver`;
- a **physical** prim that falls, collides and reports the collision;
- a **broken** script whose source does not compile, so the editor's
  error path has a specimen;
- a **linkset** of three prims exchanging `llMessageLinked`.

**Half of that roster needs an avatar, and an NPC is not one.** A
permission grant, a payment, a dialog answer, a sit and a played
animation all require something on the other end of a circuit that can
be *asked* and can *answer*; the scenario's NPC fixtures are objects
with an appearance and no client behind them, so they can be sensed and
looked at and nothing more. So the scenario has to name, alongside its
prims, the **avatars it needs and what each of them does** — and the
thing that plays them is
[[server-fake-grid-scripted-avatars]]. Keep the two separable: the prim
half must still come up and be drivable by a human in the viewer with no
scripted avatar present, because that is the mode in which a person
eyeballs it.

Two constraints the existing scenarios already meet and this one must:
it is **deterministic** ([[server-world-determinism-contract]]) — every
timer, every `llFrand` and every id is a function of the seed — and it
is **self-describing**: the landmarks list is what a camera, a
screenshot run and a reader of the startup log all use.

The scripts themselves are source in the repo (`sl-fake-grid`'s
fixtures, or a `scripts/` directory beside them), not strings buried in
Rust, so they can be opened in the viewer's editor, edited, saved back,
and diffed against a run on the local OpenSim
([[test-lsl-differential-opensim]]).

Acceptance: `scripts/fake-grid.sh --scenario scripts` comes up with
every prim rezzed and running; the Bevy viewer logged into it can touch,
menu, sit on and pay the fixtures by hand; the same scenario run with
its scripted avatars exercises the grant, pay, sit and animate paths
unattended; and `sl-crosscheck --scenario scripts` produces a frame pair
from both viewers.

## From server-lsl-lib-prim-state (2026-11-04)

The prim tranche's acceptance left the rendered half here: a prim whose script
changes colour, texture, text, scale and particles on a timer, and both
viewers' frames agreeing. `sl-fake-grid/tests/scripts.rs`'s
`a_script_dresses_its_prim` already checks the update such a script sends.

## From server-lsl-lib-task-inventory (2026-11-07)

A rezzer is ready to stand in the scenario: a scripted prim holding an object
the grid took (dropped in by a viewer, or by the scenario through the take
path) rezzes it with its scripts and a start parameter.

## From server-world-collision-and-physics (2026-11-13)

The physical prim is ready to stand in the scenario:
`llSetStatus(STATUS_PHYSICS, TRUE)` makes a prim fall, land and rest, and it and
what it lands on hear `collision_start`, `collision`, `collision_end` and
`land_collision_*` as each grid raises them (`tests/scripts.rs`,
`a_falling_box_collides`). This task's block on
[[server-lsl-lib-physics-vehicles]] was for exactly that and is replaced by the
collision task; the scenario needs nothing else of that tranche.

## Done (2026-11-13)

Two named scenes, `scripts` and `scripts-avatars`
(`fixtures::scripts_scene`, book `tools/fake-grid.md` § The scripts scene).
The row, west to east at `y = 132`, one landmark each: a sensor, a touch
prim, a dialog prim, a listener (channel 0 and 42), a vendor, a pose ball
with its own pose, a rezzer holding a scripted `Spark`, a notecard reader, a
timer prim (spin, colour, height, floating text), a physical box over a
plinth, a three-prim linkset and a broken-script prim. The scripts are
`.lsl` files beside the module, the notecard a `.txt`.

The rezzer needed something the grid lacked: an object item a fixture can
state *with* its scripts. A fixture's object item rezzed from its body only,
which carries no script, and only a linkset the grid took came back scripted.
`HeldObject` (on `RegionFixture` and `Scenario`) is that linkset, filed when
the grid starts as though taken, its scripts starting afresh with `on_rez`.

`scripts-avatars` brings `visitor()`, which exercises the grant, pay, sit and
animate paths (and every other behaviour of the row) unattended;
`visitor_on(grid)` drops the payment on OpenSim, which raises no `money`
event. Writing it found a scripted-avatar defect: a line that arrived while a
step *acted* was kept only if the acting step answered it, so an `AwaitChat`
after a `Walk` lost the sensor's line said mid-walk. During an act the step
about to run is now the next one.

`tests/scripts_scene.rs` plays the visitor on both flavours (every
behaviour's line in order, no fault, no stubbed call, two seeded runs alike)
and the bare scene with nobody in it. The scenario's prims have no
determinism exception: the binary's grid is not seeded, a test's is.

The viewer drives the scene too:
`sl-client-bevy-viewer/tests/e2e_scripts_scene.rs` touches, menus, chats with,
rezzes from, reads, pings, clicks to pay, sits on and drops every prop on both
flavours, and opens the broken script from the Build window's Content tab and
saves it to be shown its error at line 8. Two fake-grid changes came out of it.
A prim whose running script has a `money` handler now carries
`FLAGS_TAKES_MONEY`, which a viewer's pay click action needs and the fake grid
never set (`a_money_handler_flags_the_prim_as_taking_money`). The broken prim
and its script grant everyone modify, a fixture affordance (OpenSim honours such
a mask, SL never lets an owner grant it), so whoever logs in can save it. This
viewer has no pay window yet ([[viewer-money-economy-ui]]), so the stage checks
the pay-unavailable notice; paying by hand waits on that. The cross-check frame
pair is filed as two bugs: [[viewer-harness-status-counts-before-last-frame]]
and
[[viewer-capture-world-only-draws-hover-text-and-tags]].
