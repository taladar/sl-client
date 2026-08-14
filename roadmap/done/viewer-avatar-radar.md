---
id: viewer-avatar-radar
title: Avatar radar (nearby-avatar list)
topic: viewer
status: done
origin: user request (2026-07)
refs: [viewer-ui-virtualized-list, viewer-name-tags-display-names,
  viewer-minimap, viewer-social-people-panel, viewer-beacons-control]
---

Context: [context/viewer.md](../context/viewer.md).

## Done (2026-08-14)

Implemented as `sl-client-bevy-viewer/src/radar.rs` (floater, sweep / alert
/ view systems, `RadarPlugin`) over the pure, unit-tested
`radar_model.rs` (per-sweep set diff + chat / draw / sim
threshold-crossing detection mirroring the reference's `mLastRadarSweep`,
per-agent first-seen / age / payment bookkeeping, formatting, filter,
multi-key sort, counts — 27 tests).

- **Data sources, all shared:** `AvatarState::map_avatars()` (full +
  coarse merged, altitude-unknown sentinel → unknown distance, shown as
  `>draw-distance`), batched name resolution, `NameTagStatuses` typing,
  the away animation via `AnimationPlayback`, `is_seated`, `title_of`,
  grid-overridable `ChatRanges`, and `MapTracking` for the Track action.
  Age / payment info from a throttled request-once
  `RequestAvatarProperties` trickle (5/sweep), `born_on` parsed leniently
  (SL `MM/DD/YYYY` + ISO).
- **Floater:** standalone (like the minimap; World ▸ Radar + a bottom
  toolbar button beside Mini-map), deferred content, geometry persisted.
  Name filter (`ui_search`), limit-by-range checkbox + metres field
  (`RadarLimitByRange` / `RadarNearMeRange`, default off / 162), counts
  line (total / in region / in chat range), the shared `ui_table` with
  built-in sort (default range-ascending, sort + widths persisted),
  virtualized rows, trailing Profile / IM buttons, double-click →
  profile. Row context menu: Profile, IM, Start / Stop Tracking,
  Teleport To (disabled while altitude unknown), Offer Teleport, Add
  Friend, Block / Unblock. Selection is agent-keyed (survives the 1 Hz
  re-sort). Row styling: friend / muted name colours; range cell
  green / yellow by chat / shout band; hollow region dot for a
  coarse-only avatar; T / S / A status letters.
- **Alerts (reference parity):** the model sweeps every 1 s whether or
  not the floater is open; chat / draw / sim enter + leave each gated by
  its own opt-in setting (`RadarReport*`, default off) plus a
  young-account alert (`RadarAgeAlert` / `RadarAgeAlertDays`, once per
  entry); output per `RadarAlertOutput` — Nearby Chat (overlay +
  transcript with a clickable name, via the new
  `conversations::NearbyChatNotice`) or a `RadarAlert` toast; each
  reported batch plays the new `UiSound::RadarAlert`. Preferences ▸
  Alerts hosts the toggles, output combo and age-days field.
- **Live-verified on the local grid** (second avatar held via an
  sl-repl-tokio `sleep`/`logout` script): row appears with name → friend
  colour, age 59 from the profile reply, seen clock and 5.02 m chat-band
  range; counts line updates; "entered the region (5.02 m)." +
  "entered chat range (5.02 m)." then "left chat range." + "left the
  region." in Nearby Chat; row + counts clear on logout.

Deliberate divergences (documented in the module docs): no voice / notes
columns, no per-column show / hide bitmask, no LSL-bridge altitude
correction, no Phoenix script-channel alerts, no camera-zoom action (no
focus-other-avatar camera primitive yet — follow-up), friend / muted name
styling is colour-only, and the counts live on their own line rather than
inside the Name header. Interactive checks (sort clicks, filter typing,
context-menu actions, toast mode, neighbour-region coarse styling, the
alert sound) remain for a live session.

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
