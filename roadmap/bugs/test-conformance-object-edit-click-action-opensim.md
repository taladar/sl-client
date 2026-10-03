---
id: test-conformance-object-edit-click-action-opensim
title: object-edit on OpenSim never sees the sit click action re-broadcast
topic: test
status: bugs
origin: test-phase-z-deferred-04 regression runs (2026-10-03)
refs: [test-object-edit]
---

Context: [context/test.md](../context/test.md).

## Observation

`sl-conformance run --grid opensim object-edit` fails every run with "the
object never re-broadcast with the sit click action within the step window"
(`object_edit.rs`, the `SetObjectClickAction` → `confirm_object_update` step,
matching `object.click_action == ClickAction::Sit.to_code()`). Reproduced on
the committed code at `bbe7854e` as well as with the shared rez helpers, so it
is independent of them.

## Next step

Check what OpenSim does with `ObjectClickAction` (does it re-broadcast the
object at all, and in which update form — a terse update carries no click
action) and what our decoder does with the click action field of the update
that does arrive; then fix whichever side is wrong (the case's expectation or
the client).
