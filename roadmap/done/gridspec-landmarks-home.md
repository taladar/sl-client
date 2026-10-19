---
id: gridspec-landmarks-home
title: Landmarks and the home location on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-places-landmarks]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Firestorm sets home via the `HomeLocation` cap first (OpenSim serves it), our
client only UDP; the fake grid mints empty landmark bodies and copies OpenSim's
Set-Home rule.

## Discover

An `sl-repl` probe / case: set home (permission rule, alert text, cap reply),
create a landmark and read its body; both grids.

## Document

`book/src/gridspec/teleport.md` § Home and landmarks.

## Fake grid

Small — landmark bodies, the cap, a Set-Home flavour row.

## Viewer

Cap first, UDP fallback.

## Capabilities done in this task

[[protocol-cap-home-location]]: setting home over `HomeLocation`, with the
reply each grid sends.

## From gridspec-teleport (2026-10-07)

Left for this task: the progress lines of a *successful* landmark teleport on
Second Life (`sending_landmark` was seen ahead of a refusal; the fake grid's
Second Life flavour assumes `resolving` and `Sending to destination.` follow,
as they do for home), and Second Life's refusal of a home teleport by an agent
with no home (OpenSim says `Home set not`; the fake grid's Second Life flavour
still sends `invalid_tport`). Both are rows of
`ImitatedGrid::teleport_policy`.

## Outcome (2026-10-19)

Discovered with the `landmarks-home` census (1av, on all four grids and in the
offline list): Set Home over UDP and over `HomeLocation` (a corner, outside the
region, a body naming no location, where the avatar stands, where a home
teleport lands), a home teleport, a landmark made as the reference makes one
and read back, and two teleports to it. Documented in
`book/src/gridspec/teleport.md` § Home and landmarks, linked from
`book/src/content/teleport.md`. Findings:

- Second Life refuses a resident on land it does not own over both roads —
  even where its home teleport put it — with "You can only set your 'Home
  Location' on your land or at a mainland Infohub.", keyed
  `HomeLocationLimits` beside the capability and `ListValidHomeLocations`
  over UDP. Its capability answers `success` as the string `"false"` and
  echoes the position; outside the region it says nothing; a body naming no
  location is answered `success` `"true"` with an undefined position. A second
  identical refusal 6 s after the first carried no alert (one 18 s after did).
- OpenSim lets the land's owner and an estate manager set home anywhere in the
  region; the answer is `text/html`, names the position only when stored, and
  is accompanied by an `AlertMessage` ("Home position set." keyed
  `HomePositionSet`, or a plain refusal / "Set Home request failed").
- A local home or landmark teleport carries the kind line on Second Life and
  its kind flag alone (no `WITHIN_REGION`); OpenSim adds `SET_LAST_TO_TARGET`.
- Landmark bodies: Second Life `region_id` + `local_pos` to two decimals;
  OpenSim adds `region_handle` and prints floats as they are. Masks differ per
  grid. Aditi's sandbox telehub catches every teleport into it, and refuses
  one that would not get the avatar closer (`CouldntTPCloser`).
- Not measurable: a successful Set Home on Second Life (no test avatar owns
  land or stands at an Infohub, [[gridspec-aditi-test-land]]), its refusal of a
  home teleport with no home, a landmark teleport to another region.

Capability: [[protocol-cap-home-location]] — `Command::SetStartLocation` takes
`HomeLocation` where granted (both runtimes) and reports
`Event::HomeLocationAnswered`; UDP otherwise.

Fake grid: small — `SimCaps` serves `HomeLocation` per `HomeLocationStyle`;
`ImitatedGrid::home_policy` gives the set-home rule, the notices on each road,
the repeat quiet and the landmark body and masks; every account's home is
stored grid-wide and used by a home teleport and OpenSim's login `home`;
`TeleportPolicy::kind_flags` and the local kind line follow the measurement.
Telehub routing is not modelled (the census holds no arrival position).

Viewer: the runtime takes the capability first; the viewer's notification
catalogue already shows the three keyed alerts. Fake-grid end-to-end tests hold
the capability road on both flavours, the UDP fallback and a home teleport.
The Set Home menu entry is [[viewer-places-landmarks]]'s, given the findings.
