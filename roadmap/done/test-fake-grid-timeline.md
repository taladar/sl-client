---
id: test-fake-grid-timeline
title: Scripted scenario timelines with markers
topic: test
status: done
origin: test-harness plan (2026-08-30)
points: 5
refs:
  [
    viewer-fake-grid-render-harness,
    viewer-environment-not-refetched-on-region-info,
    protocol-experience-environment-push,
  ]
---

Done 2026-09-09.

Context: [context/testing.md](../context/testing.md).

Everything else in `sl-fake-grid` answers something the client asked for. A
timeline is the other half — what happens to a session because **time passed**,
which is what a test of anything *moving* needs.

## What landed

`sl_fake_grid::timeline`: `Timeline { steps: Vec<Step { at, action }> }`, stated
by `Scenario::timeline` (and `RegionFixture::timeline`, so a scene can carry its
own script), run by one task per session that waits for the agent's arrival and
then walks the steps.

`At` is `AfterArrival(d)`, `AfterPrevious(d)`, `OnEvent(pred)` — tested against
every `ServerEvent` since the script started, so a step waiting for something
that already happened runs at once — or `OnMarkerAck`. `Action` is the
eighteen the task named, plus `ConfigureRegion` (see below). Every wait is a
`tokio` sleep and every stamp comes from the injected clock.

The two timeout policies differ **on purpose**, and each is documented where it
is decided. `OnMarkerAck` warns and carries on: it orders two things the client
would otherwise see in either order, and an ordering nicety that could strand a
run would be worse than the reordering it prevents — the call `teleport.rs`
already makes about its own `TeleportStart`. `OnEvent` warns and *stops*: there
the wait is the whole content of the step, and the rest of a script written for
a world where that event happened means nothing in a world where it did not.

## The cursor travels, and that is most of the work

A script that says "teleport, then move the prim you find there" has to outlive
its session: a teleport destination is always a second `SimSession`. So
`hand_over` moves the steps that have not run yet onto the destination once the
client has arrived — called by `teleport.rs` and `crossing.rs`, so a
**client-initiated** hop carries the script as surely as a scripted one. The
session left behind keeps the prefix it ran; a finished script hands over
nothing, which is what leaves a destination region's own timeline alone.

Three things this needed that are not obvious:

- The runner **parks** on a notification instead of exiting when it runs out of
  steps, so it does not matter whether a script is handed to a session before or
  after that session's own arrival. Without it the hand-over had an ordering
  hazard in both directions.
- A step is peeked, waited for, and only **then** claimed. Claiming before the
  wait would let a hand-over during a long wait leave the step behind.
- The cursor carries a **generation**, bumped by every hand-over. Without it a
  runner waiting on step *n* of one script could execute step *n* of the script
  that replaced it — same index, different step.

`SharedSim::with_region` is new alongside it: mutate the region's world, send,
and publish the `RegionChange`es under the region lock, so a scripted rez is a
rez the region's *other* avatars see rather than a picture painted on one
circuit. Lock order is the crate's own — session, then region.

## What the environment leg turned up

The acceptance list wanted a kill, a move and an environment-change full-stack
test. The first two were straightforward. The third looked blocked, and the
reason it looked blocked was wrong twice over:

- There **is** a live environment push in the protocol — `PushExpEnvironment`,
  an experience's `llSetEnvironment` — and neither end of it exists here. That
  is its own item, [[protocol-experience-environment-push]].
- And an estate settings change needs no push at all: the reference viewer
  re-reads `ExtEnvironment` on every `RegionInfo`, unconditionally
  (`LLViewerRegion::processRegionInfo` → `LLRegionInfoModel`'s update signal →
  `LLEnvironment::requestRegion()`), and it could not compare a field if it
  wanted to — a `RegionInfo` carries no environment fields. **Our** viewer
  re-read only on the handshake, so an estate that changed its sky never
  reached an avatar already standing there:
  [[viewer-environment-not-refetched-on-region-info]], fixed here.

So the timeline grew `Action::ConfigureRegion` — the estate floater's Region
tab, which is what sends that `RegionInfo` — and it sits beside
`SetEnvironment` because the pairing is the protocol, not a convenience.

## What holds it up

`sl-fake-grid/tests/timeline.rs` drives the real `sl-client-tokio` client:
a script moves the stock object, marks it, and kills the object behind the
client's own acknowledgement of that marker, with the *client's* event order
asserted; a script survives the teleport it asked for, into a region ten away so
the destination cannot be a neighbour circuit the login already opened; and a
scripted wait is really waited out, measured from the grid's own `AgentArrived`
(the client's handshake is not a safe baseline — it beat the grid's arrival by
four milliseconds).

Two full-stack tests in `sl-client-bevy-viewer/src/full_stack_test.rs`, both
driven by a scenario timeline with **nothing** done by hand: the viewer says a
line, the script's `OnEvent` step fires on it, and the script's marker is what
the test waits for before it looks again. `a_scripted_move_puts_the_object_where
_the_script_said` asserts both discs — the one the box left is empty and the one
the script named is painted, because an object that merely vanished would pass
the first alone. `a_scripted_environment_change_darkens_the_sky_unasked` drops
the existing sky test's one piece of cheating: there is no
`harness.command(RequestEnvironment)`, so the viewer has to decide by itself
that its sky is stale.

`ViewerHarness::say` is the new half of the harness the `OnEvent` cue needed: a
line the viewer says is the one grid-side event a rendering test can raise at
the exact moment it is ready, rather than a plausible number of milliseconds
after the arrival.
