---
id: protocol-avatar-render-info
title: AvatarRenderInfo — client request, SimCaps service and fake-grid answers
topic: protocol
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-avatar-render-info]
---

Context: [context/protocol.md](../context/protocol.md).

## What

The cap exists nowhere in the workspace. Add the client request and reporting,
the server-side cap, and the fake grid's per-flavour answers.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Capability (triage 2026-10-04)

`AvatarRenderInfo` (Second Life only) is two halves. The GET (every 15 s)
returns `{agents:{<id>:{weight}}, reportinglimit, overlimit}` — the
server-reported complexity and the "N residents render you as a jellydoll"
notice; that half is a feature. The POST (every 60 s) reports
`{agents:{<id>:{weight, tooComplex}}}` for the avatars this viewer sees; it is
what other residents' notices are built from, so it is sent like the GET, not
behind [[viewer-telemetry-opt-in]].

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.

## Measured (gridspec-avatar-render-info, 2026-10-21)

`book/src/gridspec/avatars.md` § Render info; the `avatar-render-info` case
holds both live grids. Already done there: the client requests the cap
(`CAP_AVATAR_RENDER_INFO` — Second Life grants it only when named), and the
server coverage table carries it as `Pending`. What is left, by what was
measured:

- **Client**: the reference's cadence (GET every 15 s from arrival, POST every
  60 s + 2 s per avatar, nothing when no avatar has a complexity), both
  runtimes; an event with each avatar's stated weight and the two counts;
  the POST's answer is always `200 {}`, so there is nothing to read back.
- **Fake grid, Second Life flavour**: grant it; answer every POST `200 {}`
  whatever it holds; state `{agents:{}, overlimit:0, reportinglimit:0}` before
  any report, a lone report's weight verbatim to every reader in the region,
  `reportinglimit` as the number of reports of the reader. The region's own
  reckoning past one report was not reducible to its inputs — pick a stated
  rule (latest report wins) and say it is the fake grid's; `PUT` / `DELETE`
  `405`. OpenSim flavour: refuse it (already in `OPENSIM_REFUSED_CAPABILITIES`).
- **Conformance offline**: the case's render-info legs on `FakeSl` /
  `FakeOpensim` need the attachment legs gated off on the fake grid (it wears
  nothing — `fake-grid-own-attachments-and-region-moves`), and the move to a
  quiet region skipped there.
- **Viewer**: show the stated weight beside our own reckoning (radar column,
  name tag) and the "N residents render you as a jellydoll" notice.

## Done (2026-10-21)

- **Wire** (`sl-wire` `avatar_render_info`): the statement
  (`AvatarRenderInfo`: weights sorted by agent, the two counts only when both
  are there, as the reference reads them) and the report
  (`AvatarRenderReport`, a weight past `i32::MAX` sent as `i32::MAX`), both
  directions.
- **Client** (`sl-proto` `session/render_info`): the session keeps the
  reference cadence. A runtime calls `start_avatar_render_info` with whether
  the region granted the cap each time it installs a capability map, and
  serves `next_avatar_render_info` each turn of its loop. The statement is
  read on arrival and every 15 s. The report goes 5 s after arrival, then
  every 60 s plus 2 s per avatar, and is never sent empty. A region change
  forgets both timers and the report. The application hands over its report
  with `Command::SetAvatarRenderReports`; a statement surfaces as
  `Event::AvatarRenderInfo`; the POST's `{}` is read only for failure. Both
  runtimes are wired. Two simplifications: the next read is timed from the
  request (the reference times it from the reply), and only the agent's own
  region is read and told (the reference serves every region it has
  capabilities for).
- **Fake grid**: `SimCaps` serves the cap (`CapsDispatch::AvatarRenderInfoRead`
  / `AvatarRenderInfoReport`, coverage row `Served`). The region holds the
  reports in `sl-fake-grid` `render_info` and states them back as measured.
  Its own rules, stated there: latest report wins, a report is held for five
  minutes, and `overlimit` stays `0` (it never moved on Second Life). OpenSim
  refuses the cap.
- **Conformance offline**: `avatar-render-info` runs on both fake flavours
  (in `OFFLINE_CASES`). The scripted cube's legs and the move to Ahern run on
  live grids only. `MEASURED_LIVE` holds what only a live grid states: the
  cube's legs and the region's own reckoning of B.
- **Viewer** (`sl-viewer-world-avatar` `avatar_render_info`): each avatar on
  the root circuit that `AvatarComplexityModel` has scored is reported with
  its score. `tooComplex` is set when the jelly reason is the budget or the
  area (`LLVOAvatar::isTooComplex`). Statements fold into
  `ReportedComplexity`, which the radar's new Reported column and the name
  tag's `Complexity: N (reported M)` show. `AgentComplexityWithVisibility` is
  ported (catalogue, strings, coverage row) and raised by `OverLimitNotifier`,
  a port of `LLAvatarRenderNotifier::updateNotificationRegion`: not in the
  first 70 s, not within 300 s of the last notice unless it is still showing,
  never in mouselook, and only when the jellied share moves by more than a
  tenth. It has only been seen in unit tests, since no region raised
  `overlimit`.
- **Viewer e2e** (`e2e_two_avatars`):
  `the_region_states_back_the_complexity_the_viewer_reports` checks the round
  trip on the fake grid: the viewer's report of the catalogue NPC comes back
  as the Reported cell, equal to Cost, and as the name tag's `(reported N)`.
  `a_live_region_states_each_avatars_complexity_where_it_grants_render_info`
  checks Beta's Reported figure and tag on Second Life, and the empty column
  on OpenSim.

The extra column widens the radar's default to 760 px. That width exposed a
bug in the shared ellipsis rule (`sl-viewer-ui-core` `ui_ellipsis`). At a
fractional UI scale the shown and hidden layouts round apart by a pixel, so a
value in that pixel flipped the marker every frame (the radar's name cell,
right to left at 1.25). A shown marker now stays until the value fits by more
than that pixel.

Not ported: the reference's own-complexity-change notice (`AgentComplexity`,
`updateNotificationAgent`) and `HUDComplexityWarning`, neither of which reads
this capability.
