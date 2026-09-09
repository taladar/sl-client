---
id: viewer-environment-not-refetched-on-region-info
title: An estate's sky change never reaches a viewer that is already standing there
topic: viewer
status: done
origin: scoping the environment leg of test-fake-grid-timeline (2026-09-09)
points: 2
refs: [test-fake-grid-timeline, protocol-experience-environment-push]
---

Fixed 2026-09-09, by the work that found it — scoping the environment leg of
[[test-fake-grid-timeline]], whose full-stack test is what proves it.

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/environment.rs` started a fresh
`Command::RequestEnvironment` cycle on **one** event:

```rust,ignore
if matches!(event.0, SlSessionEvent::RegionHandshakeComplete) {
```

A handshake happens at login and at a border crossing. It does **not** happen
when the estate's environment is changed under an avatar that is already in the
region — so the sky the viewer was drawing stayed whatever it fetched on
arrival, for the rest of the session.

The reference viewer re-reads on `RegionInfo`, which is exactly the message a
simulator sends when the estate's Region/Terrain settings are saved:

- `indra/newview/llstartup.cpp:3817` routes `RegionInfo` to
  `LLViewerRegion::processRegionInfo`,
- which calls `LLRegionInfoModel::instance().update(msg)`
  (`indra/newview/llviewerregion.cpp:1158`),
- whose `mUpdateSignal` (`llregioninfomodel.cpp:208`) fires the callback
  `LLEnvironment` registered at `llenvironment.cpp:973`:
  `setUpdateCallback([this]() { requestRegion(); })`.

**It is unconditional**, and that is the part worth writing down: the signal
fires at the end of every `LLRegionInfoModel::update` without comparing a single
field against what it held. It could not do otherwise — a `RegionInfo` carries
no environment fields at all. It is a notice that the region's settings were
written, not a copy of them, so "did the environment part change?" is not a
question the message can answer and not one this viewer should ask either.

The live update therefore needed no new message: the grid already sends
`RegionInfo` (`SimSession::send_region_info`, published by the fake grid as
`RegionChange::RegionConfigured`).

## The fix

`request_environment` re-arms its request cycle on
`SlSessionEvent::RegionLimits` as well as `RegionHandshakeComplete`, keeping
the retry/backoff already there.
Three unit tests: the `RegionInfo` trigger, the handshake still working, and an
unrelated event arming nothing once a reply has ended the cycle.

The full-stack proof is
`a_scripted_environment_change_darkens_the_sky_unasked`
([[test-fake-grid-timeline]]): a scenario timeline writes a night environment,
saves the region, and marks — and the viewer's sky band has to darken with
nothing in the test asking it to re-read. Its sibling
`an_environment_change_to_night_darkens_the_sky` still issues the request by
hand, which is why it passed throughout and this bug survived: a test that asks
the viewer to do the thing cannot notice that the viewer never would.

## The separate thing this was not

There *is* a real environment push in the protocol — `PushExpEnvironment`, an
experience's `llSetEnvironment` injection — and it is a different feature with
its own item: [[protocol-experience-environment-push]].
