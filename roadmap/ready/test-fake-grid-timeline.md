---
id: test-fake-grid-timeline
title: Scripted scenario timelines with markers
topic: test
status: ready
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-fake-grid-render-harness]
blocked_by: [test-fake-grid-determinism, viewer-fake-grid-render-harness]
---

Context: [context/testing.md](../context/testing.md).

There is no "at t = 2 s do X". Add `Scenario::timeline: Timeline {
steps: Vec<Step { at: AfterArrival(d) | AfterPrevious(d) | OnMarkerAck |
OnEvent(pred), action }> }` with `Action::{MoveObject, UpdateObject,
RezObject, KillObject, AnimateAvatar, SetAppearance, Attach, Detach,
Chat, Im, SetEnvironment, ChangeParcel, Teleport, CrossRegion, SimStats,
SimulatorTime, Marker(name), Custom}`, run by a per-session task on the
injected clock (`tokio::time::sleep`, pausable), executing through
`with_sim`; the cursor is handed to the destination session on a teleport
or crossing so a script continues across regions. `Marker` is a
`GenericMessage` (method `sl-fake-grid-marker`) the viewer harness waits
for, which is how a test synchronises without sleeping.

The marker message itself already exists (2026-09-03):
`sl_fake_grid::{MARKER_METHOD, marker, marker_name}` in `src/marker.rs`,
with `ViewerHarness::{mark, wait_marker}` as the two ends of it — the
full-stack harness needed a marker before this task could give it one, so
it sends its own by hand. `Action::Marker(name)` emits exactly that
message from a scenario step; nothing on the receiving side changes.

Acceptance: the kill / move / environment-change full-stack tests.
