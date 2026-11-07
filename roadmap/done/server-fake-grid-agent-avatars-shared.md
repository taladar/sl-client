---
id: server-fake-grid-agent-avatars-shared
title: Fake grid — logged-in agents see each other's avatars
topic: server
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-sweep-two-avatars, server-world-agent-movement, gridspec-logout,
  server-world-sit-and-attach, fake-grid-own-attachments-and-region-moves]
blocked_by: [gridspec-avatar-presence, gridspec-animations]
---

Context: [context/testing.md](../context/testing.md).

Two stage viewers logged into one fake region do not see each other: each
session streams the region's objects and NPCs, but never another session's
avatar, so a viewer's radar, minimap, name tags and world locators find the
catalogue NPCs and nobody else ([[test-e2e-sweep-two-avatars]] is waiting on
this). A real simulator sends every avatar in the region to every agent: the
avatar's `ObjectUpdate` (with its name values), its `AvatarAppearance`, its
`AvatarAnimation`, and a `KillObject` when it leaves. The fake grid already
publishes region writes to every session's watcher; an arriving or leaving
agent is one more such write. Where the avatar stands is the region's avatar
controller's ([[server-world-agent-movement]], since 2026-11-06), which sends
its terse updates to the agent's own session alone today: the others are
this task's to tell, at the same cadence.

`logout-seated` ([[gridspec-logout]]) is the first conformance case waiting
on this: both live grids send an observer the `KillObject` of an avatar that
logged out (OpenSim 30 ms after the `LogoutRequest`), and the case is
live-only because the fake grid has no observer to send it to. Once agents
are shared — and a sit re-parents the avatar for the others
([[server-world-sit-and-attach]]) — add it to the offline list for both
flavours.

## From gridspec-avatar-presence (2026-10-08)

Measured in `book/src/gridspec/avatars.md`; build to that chapter.

- **Arrival**, as the others see it: the object, then `AvatarAppearance`
  and `AvatarAnimation` within 30 to 65 ms, on both grids. Second Life sends
  the appearance twice and the animations five to seven times in the first
  fifteen seconds, and goes on re-sending an idle avatar's animations;
  OpenSim sends each once.
- **Departure**: a `KillObject` 0.3 to 0.4 s after a teleport's request
  and 0.6 s after a `LogoutRequest` on Second Life, 0.2 s and 15 ms on
  OpenSim.
- **No range**: neither grid withholds a same-region avatar by distance or
  draw distance.
- **The coarse feed** is already sent per flavour, listing NPCs and the
  agent; it has to list the other sessions' agents too, drop one with the
  next update after it leaves, and go down a child circuit with the
  neighbour's own list.
- **A neighbour's avatars** are streamed down the child circuit as full
  objects, and killed there when the avatar crosses into the agent's own
  region (before its object arrives on Second Life, with it on OpenSim).
- **Parcel hiding** (OpenSim; Second Life unmeasured): one-way, 60 ms, the
  coarse feed untouched.

`avatar-presence` declares both fake flavours but runs only its
single-resident leg there; drop the `is_fake` branch once residents see each
other.

## The world map's agent locations (2026-10-08)

Measured by [[gridspec-world-map]] (`book/src/gridspec/world-map.md`
§ Agent locations), and what shared avatars have to feed: a
`MapItemRequest` for agent locations never counts the asker; OpenSim sends
one item per other avatar at its whole-metre position with `Extra` 1 and
keeps another region's answer for two minutes; Second Life sends a count of
the others at a coarse position and is current within two seconds. Until
then `world_map::item_answer` sends only the item both grids send for a
region with nobody to show.

## From gridspec-animations (2026-10-19)

Measured in `book/src/gridspec/avatars.md` § Animations. What another viewer
is told of an avatar's animations:

- every statement the avatar's own viewer is sent, the other is sent too, with
  the same sequence numbers;
- Second Life names only the **first** entry's source (nil) to the other
  viewer and carries one empty `PhysicalAvatarEventList` block; OpenSim names
  none and carries none;
- the agent's own set is kept per session already (`crate::animations`): the
  broadcast is that set, re-shaped per flavour for an observer.

Add the `animations` census's `*_seen` keys to the held fake columns once this
is built.

## From gridspec-appearance-baking (2026-10-19)

Measured in `book/src/gridspec/appearance.md` § Baking. What another viewer
is told of an avatar's appearance:

- Second Life sends every viewer near the agent an `AvatarAppearance` after
  each accepted bake, the same as the agent's own;
- OpenSim sends every *other* viewer the publish of an `AgentSetAppearance`
  verbatim, and the publisher nothing;
- the published bakes are kept per session already
  (`sl-fake-grid` `bakes::PublishedBakes`): the broadcast is that appearance.

Add the `appearance-baking` census's `*_seen_*` keys to the held fake columns
once this is built.

## Done (2026-11-07)

Each region keeps its residents (`world::Resident`): what its other viewers
are shown of each agent — body, appearance, animations, where it stands —
from the arrival to the departure, and new `RegionChange`s carry what
happens to it (`ResidentArrived`, `ResidentShown`, `ResidentMoved`,
`ResidentAnimated`, `ResidentAppearance`, `ResidentLeft`); each session's
watcher streams them (`driver::stream_resident`).

- **Arrival**: a newcomer's burst sends everybody already there after the
  NPCs, however far off; the newcomer is introduced to the others —
  object, appearance, animations, then what it wears (never a HUD;
  `seating::shown_to` now shows another resident's attachments).
- **Movement**: every terse update the avatar controller sends an agent of
  itself goes to the others at the same cadence; a posture change's
  statement too. A sit, a stand (viewer, script or `seat_on`) and a
  teleport within the region re-send the object.
- **Animations**: every statement the agent is sent, the others are sent
  under the same numbers in the observer's shape (`animations::Observed`:
  Second Life one nil source and one empty physical block, OpenSim none),
  over a new `SimSession::send_avatar_animation_statement`.
- **Appearance**: Second Life's record after each accepted bake; OpenSim's
  publish relayed as sent, to the others only.
- **Departure**: a logout (the reaper, via `seating::leave`) and a walk over
  a border (`crossing`) kill it for the others; a teleport away kills it as
  the finish goes out on Second Life and not at all on OpenSim, as the
  `attachments` census held them
  ([[fake-grid-own-attachments-and-region-moves]]).
- **Coarse feed**: the NPCs, the other residents and the agent, refreshed
  on every resident change. A child circuit now gets its region's own feed
  (`SimSession::start_child_coarse_feed`; a demoted root keeps its feed
  without its own entry) and its residents as full objects.
- **Parcel privacy**: `PresencePolicy::hides_on_private_parcels`, on
  `FakeOpensim` only (Second Life unmeasured): killed for viewers off the
  parcel as it steps in, introduced again as it or they cross the line.
- **World map**: agent locations from every region's roster
  (`MapPolicy::agent_items`): OpenSim an item each at its whole metre,
  Second Life a count per 64 m square (inside the measured 11 to 128 m).
- **Conformance**: `avatar-presence` runs whole offline (its `is_fake`
  branch dropped); `logout-seated` joined the offline list; the
  `animations` and `appearance-baking` seen keys are held on the fake
  flavours, but for `idle_seen_sources`, which a one-entry set cannot
  answer.
- **Tests**: `client_end_to_end` — residents shown to each other (arrival
  both ways, an animation in the observer's shape, the logout's kill), a
  walking resident moved and listed coarsely, a resident next door shown
  down the child circuit and killed here, a private parcel hiding its
  resident on OpenSim only; `world_map` unit test of the items.

Not imitated: the timings (everything goes in the breath that caused
it); Second Life's repeats of an
idle avatar's animations and of an arrival's appearance; OpenSim's
two-minute map cache for another region. `parcel-privacy` waits on a
parcel divide ([[server-fake-grid-parcel-divide-join]]).
