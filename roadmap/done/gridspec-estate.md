---
id: gridspec-estate
title: Estate info, access, covenant and estate actions on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-region-restart-schedule,
  test-kick-user, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim: silent refusal without rights, four trailing access lists, deferred
deltas; kick text "You have been kicked out". Restart notices differ (SL
named `RegionRestart*` alerts, OpenSim plain text) and our code handles
neither. The fake grid answers none of the estate actions.

## Discover

Estate cases as OpenSim estate owner with two avatars (kick, eject, freeze,
teleport home, estate message, restart); aditi covenant leg (no rights needed);
restart notices observed opportunistically.

## Document

`book/src/gridspec/estate.md`.

## Fake grid

Small restart alert row in this task; actions —
[[server-fake-grid-estate-actions]].

## Viewer

One countdown from both restart forms; kicked / frozen / ejected handling.

## Capabilities done in this task

[[protocol-cap-estate-access]]: the estate access lists over `EstateAccess`,
with the ban details only the capability carries.

## Done (2026-10-17)

The census `estate` (2av) measures the reads (`getinfo`, the covenant
summary, the `EstateAccess` capability raw) by both avatars, a resident's
refusal of every estate command and of freeze and eject, and — where A holds
estate rights — the region and estate messages, the access lists with B in the
region, the covenant, freeze (to its wearing off), eject (with a ban), sending
home one and all, a kick and a real restart, recording what each avatar is
told. Held to `Measured` constants; written up in `book/src/gridspec/estate.md`.

- **OpenSim**, everything, as the estate owner in the Default Region. Estate
  commands from a resident are refused silently and its `EstateAccess` read
  `401`; the messages are non-modal agent alerts (`/text`) to everybody; an
  allowed or banned change answers both lists; a ban sends B home (kicked
  when B has no home); restart notices at 30 s and 15 s, then "The simulator
  is going down." to everybody. The census ends by restarting the Default
  Region, which comes back by itself.
- **Second Life**, what a resident without estate rights sees only
  ([[gridspec-aditi-test-land]]): silence for every estate command, `200`
  with four `undef` lists from `EstateAccess`, keyed alerts refusing freeze
  and eject, and Mauve's covenant summary (mainland, no covenant).

Client: `Command::SendRegionMessage` (the region message now names its
sender's legacy name, not its id), `Command::SendEstateMessage`
(`instantmessage`, the estate-wide message the client lacked),
`Command::SetEstateCovenant`, and `EstateAccess` adopted
([[protocol-cap-estate-access]]). Restart notices from both grids read into
`Event::RegionRestart` (`sl_proto::RegionRestartNotice`); `AlertInfo`'s
parameters are read as the serialized LLSD Second Life sends
(`AlertInfo::parameters`), where the viewer had parsed `KEY=value`.

Fake grid: `EstatePolicy` (`EstateAccess` refused to residents on OpenSim,
four `undef` lists on Second Life; the restart notice keyed or spelled), the
capability served from the estate fixture with who banned whom, both lists
after an allowed/banned change, the region and estate messages and the first
restart notice sent to the whole region, and no reply to a covenant change.
The actions themselves (kick, eject, freeze, home, the countdown and the
restart) are [[server-fake-grid-estate-actions]]; the census runs on the live
grids only until then.

Viewer: one restart countdown card from either grid's notice
(`sl-viewer-notices/src/region_restart.rs`), in place of the notice's toast,
closed by a cancellation, a region change or the session's end; About
Region's Access tab reads the lists over the capability where it answers,
re-reads it on a `setaccess` reply as the reference does, and shows a ban's
last login, date and banner. e2e: `a_restart_notice_counts_down_and_a_
cancellation_closes_it` (both flavours) and `the_banned_list_names_who_set_
each_ban` in `e2e_windows.rs`. A kick still ends the viewer
([[viewer-disconnect-screen]]).
