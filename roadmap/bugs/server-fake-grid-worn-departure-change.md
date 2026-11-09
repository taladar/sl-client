---
id: server-fake-grid-worn-departure-change
title: Fake OpenSim runs a worn script's departing changed(32768) after it arrives
topic: server
status: bugs
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
