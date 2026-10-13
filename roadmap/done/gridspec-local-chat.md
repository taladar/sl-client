---
id: gridspec-local-chat
title: Local chat and typing on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-comms, gridspec-lsl-comms,
  gridspec-lsl-runtime-errors, gridspec-mute-list]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-13)

Measured and written up in `book/src/gridspec/chat.md`.

- **Discover.** Two new two-avatar cases. `local-chat`: avatar A speaks,
  owns a probe prim and climbs away in three steps while B stays and gives
  the probe its orders — every chat type a viewer may write, typing,
  channels, text limits, a burst, each of a script's say functions, a
  viewer's line on a channel a script listens on, and all of it again at
  13–19 m, 37 m and 122 m, once with a camera left beside the other avatar.
  `chat-border`: the two avatars either side of a region border, a whisper,
  a say, a shout and the typing notices each way at three distances. Held
  to 173 and 32 answers; no answer changed between two runs but one
  (below).
- **Findings.** Both grids keep the three ranges (10, 20, 100 m) between
  the avatars, never from a camera, and **neither ever marks a line less
  than fully audible** — out of range it is not sent. Second Life marks a
  **typing notice** barely audible between a say's reach and a shout's and
  sends none beyond; OpenSim sends typing to the whole region. Second Life
  drops a viewer's line of any type but whisper, say, shout and typing;
  OpenSim passes every type on to the whole region. A viewer's line on the
  debug channel keeps its type and range on Second Life and becomes a
  region-wide debug line on OpenSim; a negative channel reaches a script on
  OpenSim only. Second Life cuts a text so that it and the speaker's name
  come to 1151 bytes, OpenSim to 1000 characters and 1023 bytes. Second
  Life's `RegionInfo` states the ranges. A script's `llOwnerSay` and
  `llRegionSayTo` reach their one avatar anywhere in the region; a debug
  line goes to everybody in reach, owner or not; OpenSim's `llShout` names
  nobody as owner and its `llOwnerSay` names the prim as owner and the
  region's middle as the place. **Both grids carry a line over a border**
  by the same ranges: Second Life states it in the hearer's region's
  coordinates, OpenSim sends it down the hearer's child circuit in the
  speaker's.
- **Fake grid.** `ChatPolicy` (`chat.rs`, `ImitatedGrid::chat_policy`):
  the viewer types, the debug and negative channels, the typing reach and
  its barely-audible band, the debug range, the two text cuts, the stated
  ranges, and the two ways over a border. `local-chat` runs everything but
  its script legs offline on both flavours (112 answers), `chat-border`
  all of it.
- **Viewer and client.** `sl-proto` read local chat on the root circuit
  only, so on OpenSim a neighbour's line never arrived: `ChatFromSimulator`
  is now read on any circuit. `Event::ChatTyping` states the notice's
  audibility and position, and the viewer shows neither a line nor a start
  of typing that is not fully audible, as the reference does not. An
  `e2e_pilot` test types a line on the debug channel and reads it in a
  second viewer as each fake flavour says it.

**Differences from the plan.**

- "Audibility fading on SL" turned out to be typing only; there is no
  partial audibility of a line to render, so the viewer part is the gate
  above rather than a faded style.
- What a script's line *looks like* per grid (OpenSim's owner and position
  oddities) is documented and not built into the fake grid: it runs no
  scripts, and whoever says an object's line through `FakeGrid::say` states
  the whole of it. That is [[server-lsl-lib-comms]]'s to build to the table.

**Not held:** Second Life's typing notice over a border past a say's reach
(came once in four). **Not measured:** where between 18.7 m and 36.5 m a
debug line stops on Second Life (the fake grid takes 20 m); a script's
run-time error line ([[gridspec-lsl-runtime-errors]]); a line from an
attachment or a child prim; what a listen hears ([[gridspec-lsl-comms]]); a
parcel that hides its avatars; a muted speaker ([[gridspec-mute-list]]);
which circuit Second Life sends a neighbour's line down.

## Known already

OpenSim drops out-of-range lines and marks every delivered one fully audible,
and relays typing without distance; the fake grid models ranges.

## Discover

The chat cases on both grids; audibility fading on SL, owner / region say, debug
channel visibility, cross-border shout, object `SourceType` (scripted prim on
each grid).

## Document

`book/src/gridspec/chat.md`.

## Fake grid

Small — per-flavour audibility / ranges in this task.

## Viewer

Partial audibility rendered if SL sends it.
