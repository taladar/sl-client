---
id: test-conformance-object-edit-click-action-opensim
title: object-edit on OpenSim never sees the sit click action re-broadcast
topic: test
status: done
origin: test-phase-z-deferred-04 regression runs (2026-10-03)
refs: [test-object-edit, gridspec-object-edit]
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

## Resolved (2026-10-10)

The case's expectation was wrong. OpenSim's `SceneGraph.PrimClickAction`
stores the value, marks the group changed and sends nothing — and so does
its `MakeObjectSearchable` for the search flag — while Second Life answers
both with a full `ObjectUpdate`. [[gridspec-object-edit]] rewrote
`object-edit` as a census that records what each edit is answered with and
asks for the object again where it is answered with nothing; the difference
is a row of `book/src/gridspec/building.md` § Editing and of the fake
grid's `EditPolicy`. The same runs found that OpenSim drops a
`MultipleObjectUpdate` stating a position, a rotation and a scale at once,
which the old case also sent and would have failed on next.
