---
id: gridspec-object-update-stream
title: Object update forms, the interest list and kills on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-world-ecs-store]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

The fake grid sends full `ObjectUpdate`s of every object at arrival; never
compressed, terse or cached, ignores camera and draw distance. OpenSim sends
compressed and cached updates. Our `RegionHandshakeReply.flags` is 0 where
Firestorm sends cache / self-appearance bits.

## Discover

Extend `object-update-decode` to count each message type and record the
handshake flags sent; vary the flags; move the camera and draw distance and
count adds / kills (one kill per linkset or per prim?); both grids.

## Document

`book/src/gridspec/objects.md` § Updates.

## Fake grid

Large — [[server-fake-grid-object-update-forms]] and
[[server-world-update-scheduling]].

## Viewer

Cache probes answered, compressed / terse decoded, out-of-range kills re-added.

## Capabilities done in this task

[[protocol-cap-interest-list]]: the `InterestList` mode, measured by what
the update stream carries in `default` and `360` modes.
