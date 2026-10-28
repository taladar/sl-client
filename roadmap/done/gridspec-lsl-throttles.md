---
id: gridspec-lsl-throttles
title: Forced delays and throttles on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-call-cost-sizes]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Forced delays come from the documented `sleep` attributes, never measured;
OpenSim scales them by `ScriptDelayFactor`.

## Discover

Real delays for `llInstantMessage`, `llGiveInventory`, `llRezObject`; rez, HTTP,
`llCastRay`, region-say throttles (timing probes per the frame rules).

## Document

`book/src/gridspec/lsl.md` § Throttles.

## Fake grid

Feeds each library tranche.

## Viewer

None.

## Done (2026-10-28)

Six probes, `throttle/delays`, `throttle/email`, `throttle/rez`,
`throttle/cast-ray`, `throttle/chat` and `throttle/http-window`, run on aditi,
YEngine and XEngine; the reading is `book/src/gridspec/lsl.md` § Forced delays
and throttles. The forced delays of 31 calls were set against the keyword
file's: Second Life differs on `llTextBox` (0) and `llGiveInventory` (2 s),
OpenSim on `llSetRemoteScriptAccessPin`, `llModPow`, `llLoadURL` (10 s),
`llParcelMediaQuery` and `llMapDestination`, and OpenSim sleeps a second after
every library error. Neither grid throttles rezzing (300 in a row), raycasts
or chat on a channel other than 0; OpenSim throttles channel 0 after eleven
lines a second. Second Life's HTTP throttle counts refused requests, so
retrying keeps a script shut out until 20 s of silence. The fake grid runs no
scripts, so its half is the new [[server-lsl-forced-delays-per-flavour]], with
notes in the comms, HTTP, detection, task-inventory and email tranches. The
viewer needs nothing: a flood of dialogs or chat arrives as it is sent, and
collapsing it is `viewer-anti-spam-filter`'s.
