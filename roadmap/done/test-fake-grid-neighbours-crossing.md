---
id: test-fake-grid-neighbours-crossing
title: Neighbour child agents and a scripted region crossing
topic: test
status: done
origin: test-harness plan (2026-08-30); the CrossedRegion follow-up of viewer-fake-grid-teleport
points: 8
refs: [viewer-fake-grid-teleport, test-fake-grid-timeline]
blocked_by: [test-fake-grid-npc-avatars]
---

Context: [context/testing.md](../context/testing.md).

The fake grid has no movement authority, so a walked border crossing must
be scripted, mirroring `teleport_session`:

- `RegionConfig::neighbours: NeighbourPolicy { Adjacent (default), None,
  Named }`; on a root `AgentArrived`, `announce_neighbours` prepares a
  child session per adjacent region (role `Child`, its own burst on
  `CircuitOpened`: a region-stamped marker object, terrain, objects, no
  own avatar) and enqueues `EnableSimulator` +
  `EstablishAgentCommunication`.
- `FakeGrid::cross_agent(&agent, region, position, velocity)`: reject a
  non-adjacent target; reuse or announce the child; set its arrival
  position; `enqueue_crossed_region` (extend the LLSD with `Position`,
  `LookAt`, `RegionSizeX/Y`); wait for the destination's `AgentArrived`;
  the source kills its avatar object and becomes a child; non-adjacent
  old children are retired; a `CrossingNotice` is published.

The harness's `cross_to` belongs here rather than to
[[viewer-fake-grid-render-harness]], which shipped without it (2026-09-03)
for the plain reason that there is nothing to call yet: the grid has no
crossing. It is the one method of that task's planned `ViewerHarness` API
that is absent, and adding `FakeGrid::cross_agent` above is what makes it
writable — as a sibling of `teleport_to`, which is there and tested.

The continuity test will force the viewer-side work: re-basing existing
entities when the current region handle changes, exactly one avatar
entity per agent across circuits, attachments re-parenting, the camera
keeping its world pose, terrain keyed by handle left alone. Assert the
subject prim's projected centroid within two pixels before and after,
ground on both sides of the border, and no teleport/disconnect events.

## Done (2026-09-03)

`sl-fake-grid/src/neighbours.rs` and `src/crossing.rs`, the plan as
written except where noted below. `NeighbourPolicy` (`Adjacent` default /
`None` / `Named`) on `RegionConfig`, `GridCore::neighbours_of`, a
per-session announcer task, `world::push_child_world`,
`FakeGrid::{cross_agent, crossings, neighbours_of}`, `CrossingNotice`,
`Error::{NotAdjacent, CrossingTimedOut}`. In `sl-proto`:
`CrossedRegionInfo` + the full three-block `crossed_region_to_caps_llsd`,
and `SimSession::make_child_agent`. In the viewer:
`ViewerHarness::{cross_to, wait_neighbour}`.

Tests: `a_neighbour_is_announced_and_streams_its_scene`,
`a_border_crossing_promotes_the_child_circuit` and
`a_crossing_is_refused_where_there_is_no_border` (tokio, real client);
`a_neighbour_region_is_rendered_across_the_border` and
`a_border_crossing_keeps_the_picture_still` (the full-stack pixel tier);
`crossed_region_round_trips` and `child_circuit_generic_message_is_surfaced`
(sl-proto); `crossed_region_two_sims` extended with the source's demotion.

### Six deviations, all deliberate

**The blocker on [[test-fake-grid-timeline]] was backwards** and is
dropped. The timeline's `Action::CrossRegion` *calls* `cross_agent`; it is
downstream of this, not upstream, and nothing here needed a clock. The
loose reference stays in `refs`.

**The source must not kill its avatar object.** The plan said it should.
OpenSim's `MakeChildAgent` sends that kill only to *other* root presences
that cannot see the region the avatar walked into — never to the crossing
agent's own client, whose avatar is one object across every circuit it
holds. Doing it here would be worse than unfaithful: this viewer's
`AvatarState` is keyed by **agent**, with a scoped-id side index, so a
kill arriving on the old circuit after the new one has streamed the body
resolves to the agent and despawns the avatar outright. The source is
demoted with the new `SimSession::make_child_agent` and sends nothing.

**The child's "region-stamped marker" is a message, not an object.** A
marker *prim* would be world content, which is a fixture's business and
not the session driver's — and the region's own fixtures already identify
it. What the harness actually lacked was a synchronisation point, so the
child burst ends with `neighbour:<region name>` on the existing marker
envelope (`sl_fake_grid::{neighbour_marker, neighbour_marker_region}`,
`ViewerHarness::wait_neighbour`). The subject a *picture* needs is the new
`border` scene instead — see below.

**Teleports had to learn the same reuse.** `teleport_session`
unconditionally prepared a destination session. With neighbours announced
by default, a teleport to an adjacent region would hand the client two
simulators for one region handle and stream the destination's scene
twice. It now reuses the agent's existing session in the destination and
only abandons one it opened itself on a timeout. The Bevy teleport test's
destination moved from one region east to ten, because an adjacent
destination is already on screen before any teleport and the test's claim
("the arrival rebuilt the scene") would no longer be the one it makes.

**`ViewerHarness::cross_to` cannot use `grid(fut)`.** A grid-initiated
crossing waits on the client's `CompleteAgentMovement`, and the client
only sends one when the app steps a frame; `grid(fut)` blocks the thread,
so the two halves deadlock. `cross_to` steps the grid future *between*
viewer frames (`block_on(async { timeout(FRAME_PAUSE, &mut crossing).await })`)
— the first thing in this tier that is neither purely grid-side nor purely
client-side. The `async` block is load-bearing: `tokio::time::timeout`
arms a timer against the *ambient* runtime, so building it in the argument
position — outside `block_on` — panics with "there is no reactor running".

**A `GenericMessage` on a child circuit was being dropped** (`sl-proto`,
`dispatch_child`), which is why the neighbour marker never arrived at
first. Added, with `LargeGenericMessage`, alongside the coarse locations,
parcel overlay, sounds and appearance that were each added there for the
same reason: a neighbour region is entitled to speak, and the consumer
never learned it had. The root arm's `emptymutelist` special case is
deliberately not mirrored — the mute list is the agent's, not a region's.

### The border scene

Every other fixture answers a question about one region. Two of these
tests need a subject whose position is stated relative to a **border**, so
`fixtures::border()` (scenario `border`) rezzes one checkered 3 m marker
pillar at `x = 4`, floating five metres clear of the ground so a camera
framing it from over the border has only sky behind it. Both tests stand
the camera twelve metres inside the *western* region and look at the
*eastern* region's pillar.

The continuity test frames it **once**, before the crossing, and never
touches the camera again: what moves underneath it is the origin, by a
whole region, and the viewer's recentering has to cancel that out exactly.
It asserts the projected centre has not moved by more than two pixels, the
disc still carries its checker (drawn, not merely projected), and the
session raised no teleport and no disconnect on the way.

### The asset store was per region, and is now grid-wide

The neighbour test failed first time with the pillar's disc entirely free
of its own colours, which reads as "the region across the border was never
streamed". It had been streamed; what had not arrived was one JPEG2000
blob. A real grid's asset store is grid-wide and any region serves any id,
but each region's scenario had its own — and a viewer fetches every
texture over its **root** region's `GetTexture`, so a neighbour's prim
whose texture only the neighbour declared rendered untextured and every
checker oracle read that as absence.

Fixed rather than worked around, since it would have misled every
two-region pixel test [[viewer-fake-grid-render-catalogue]] adds. The new
`sl-fake-grid/src/assets.rs` holds one `GridAssets` store per grid, folded
from every region's fixture in builder order (a later region wins a
colliding id) and shared by every session; the arriving agent's own bakes
go in there too, so a second avatar's viewer can fetch the first's. A
`RegionFixture` still *states* what its own content references — that is
where a fixture author declares it — it just no longer owns a store.

It is a plain `std::sync::RwLock`, not an async one: the single writer
runs inside the driver's synchronous flush rule, and the readers hold it
for a `HashMap` lookup and a copy. Every path takes the session lock
before the asset lock, never the reverse. Poisoning is recovered from —
the store is bytes with no half-broken invariant, and refusing to serve
textures after an unrelated panic would hide that panic behind a far more
confusing symptom.

Covered by `an_asset_of_one_region_is_served_by_another` (the east
region's checker fetched over the *west* region's capability, decoded) and
the two unit tests in `assets.rs`.

### What this leaves for the next tasks

Two shapes of the problem are untouched here and have tasks of their own,
raised in review of this one:

- [[test-fake-grid-teleport-shapes]] — a teleport's destination comes in
  three structurally different shapes (same region, a region already in
  the neighbour set, anywhere else), and the failure half of that matrix
  is almost untested. The neighbour shape is the reuse path this task
  added, so it is new code with no test of its own.
- [[test-fake-grid-seated-crossing]] — crossing while **seated on a
  vehicle**, alone and with other riders. It needs a grid-side object
  handover this task did not build: a crossing moves the agent, not the
  prim it is sitting on.

### Live testing a crossing by hand

There is still no way to walk over a border with a viewer against the
standalone binary: the grid takes the client's word for where the avatar
is and never argues, so nothing notices a border being reached, and
`cross_agent` has no command-line trigger. `--scenario border --region
West --region East` does show the *neighbour* half by hand (the pillar
across the border is drawn from the region to its west). Scripting the
crossing itself is [[test-fake-grid-timeline]]'s `Action::CrossRegion`.
