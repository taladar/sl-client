---
id: viewer-avatar-radar
title: Avatar radar (nearby-avatar list)
topic: viewer
status: blocked
origin: user request (2026-07)
blocked_by: [viewer-ui-virtualized-list, viewer-name-tags-display-names]
refs: [viewer-minimap, viewer-social-people-panel, viewer-beacons-control]
---

Context: [context/viewer.md](../context/viewer.md).

The Firestorm-style radar: a list of who is nearby, with distance, sortable and
filterable, updating live as avatars enter and leave range — plus the entry /
exit notifications (chat line, toast, or nothing, per preference) that make it a
presence tool rather than just a table.

It shares its data source with [[viewer-minimap]] (the coarse-location tracking
in `avatars.rs`, including neighbour regions after `viewer-r24`) and its name
resolution with [[viewer-name-tags-display-names]] — display name plus username,
legacy fallback — so it should consume both rather than re-deriving either.
Range matters: coarse locations cover the region and its neighbours, while full
`ObjectUpdate` avatars only exist inside the interest radius, so decide up front
which set the radar reports and how it labels avatars it knows only coarsely.

Scope: the list and its columns (name, distance, and whichever of typing / away
/ group / age are cheap to know), sort and filter, range rings / thresholds, the
enter-leave event stream and its notification policy, and per-row actions
(profile, IM, track, mute) — the actions themselves land in
[[viewer-social-people-panel]] and the track sets a beam via
[[viewer-beacons-control]].

Reference (Firestorm, read-only): `fsradar`, `fsfloaterradar`.

Deps: [[viewer-ui-virtualized-list]] (the scrolling list) and
[[viewer-name-tags-display-names]] (shared name resolution).
