---
id: server-fake-grid-scripted-scenario
title: A scripts scenario — one prim per scripted behaviour
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
blocked_by: [server-fake-grid-script-engine-wiring]
refs: [viewer-fake-grid-render-catalogue, test-fake-grid-lsl-offline-cases,
  test-firestorm-fake-grid-crosscheck, server-fake-grid-scripted-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

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
