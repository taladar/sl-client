---
id: server-fake-grid-worn-departure-change
title: Fake OpenSim runs a worn script's departing changed(32768) after it arrives
topic: server
status: done
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`events/changed-worn` on the local OpenSim: leaving a region, a worn object's
script hears `changed(32768)` while still in the region it leaves (the handler
reads the old region), then `CHANGED_REGION | CHANGED_TELEPORT` in the new one.
The fake grid raises the 32768 in `attachments::pack` and saves the script in
the same breath (`sl-fake-grid/src/attachments.rs`), so the event runs from the
saved queue after arrival.

After the round trip the local OpenSim's drop raised no `changed(32768)` before
`attach(NULL_KEY)`, though without a trip (`events/attach`) it raised one. The
fake grid raises one. The probe ends at the `attach`, so a later change is
unseen; measure which it is.

## Fix

Run the departing event, or let the script have its turn, before the save.

## Done (2026-11-10)

`attachments::carry` now raises YEngine's departing `changed(32768)` itself
(`raise_departing`, no longer inside `pack`) and waits for the region it
leaves to give its scripts their next tick (`await_turn`, on the region's
heartbeat; a stepped region's comes when its test advances it) before it
packs and saves them, so the handler runs in the old region. The new
`a_worn_script_hears_its_departure_where_it_leaves_*` tests in
`sl-fake-grid/tests/scripts.rs` hold the order on both flavours.

The drop after the trip was measured: `events/changed-worn` now listens five
seconds past the drop's `attach(NULL_KEY)` (a `detached` record), re-run on
aditi and the local OpenSim under both engines. OpenSim raises no
`changed(32768)` for that drop at all, before or after, where a drop of an
object put on in the region raises one (`events/attach`). `AttachedTo` now
carries `travelled`, set when a worn object comes in with its wearer, and a
drop of such an object raises no 32768 on OpenSim. A take-off into inventory
after a trip is not measured and still raises one. Both fake flavours re-run
agree with their grids, and the accepted entry is gone.
