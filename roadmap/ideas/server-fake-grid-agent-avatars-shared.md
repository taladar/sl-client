---
id: server-fake-grid-agent-avatars-shared
title: Fake grid — logged-in agents see each other's avatars
topic: server
status: ideas
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-sweep-two-avatars, server-world-agent-movement, gridspec-logout,
  server-world-sit-and-attach]
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
agent is one more such write. Where the avatar stands is the login point
until [[server-world-agent-movement]] moves it.

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
