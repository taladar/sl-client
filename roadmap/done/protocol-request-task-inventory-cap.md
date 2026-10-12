---
id: protocol-request-task-inventory-cap
title: Fetch task inventory over the RequestTaskInventory capability
topic: protocol
status: done
origin: test-phase-z-deferred-04 (2026-10-03)
refs: [test-phase-z-deferred-04, gridspec-task-inventory]
---

Context: [context/protocol.md](../context/protocol.md).

Current viewers fetch an object's contents with an HTTP GET on the region's
`RequestTaskInventory` capability (`?task_id=<object>`, plus
`&inventory_serial=<n>` when a copy is held, answered `304` when unchanged),
and keep the UDP `RequestTaskInventory` → `ReplyTaskInventory` → `Xfer` road
only as a fallback Firestorm logs as "Using old task inventory path!"
(`LLViewerObject::fetchInventoryFromServer` / `fetchInventoryFromCapCoro`,
`loadTaskInvLLSD`). We request no such capability and use only the old road.

Add the capability (seed request, the LLSD reply decoded into the same
`TaskInventoryItem`s, the serial / `304` handling), prefer it where granted
and keep the UDP road for grids without it (OpenSim) — the modern-CAPS-first,
UDP-kept rule. Serve it from `SimCaps` for the fake grid's Second Life flavour
(the per-flavour choice belongs to [[gridspec-task-inventory]]). Verify on
aditi with `task-inventory` / `script-upload`.

## Capability (triage 2026-10-04)

`RequestTaskInventory` is granted by Second Life, not by OpenSim. GET
`<cap>?task_id=<object>[&inventory_serial=<n>]` → `{inventory_serial,
contents:[items]}` (no "Contents" folder; 304 when unchanged). Firestorm
re-fetches over the capability when an unsolicited UDP `ReplyTaskInventory`
arrives. Keep the UDP + Xfer path for OpenSim.

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.

## Done together with [[gridspec-task-inventory]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.

## Done (2026-10-12)

Done inside [[gridspec-task-inventory]]. `CAP_REQUEST_TASK_INVENTORY` is
requested; `Command::FetchTaskInventory` takes it in both runtimes where the
root region grants it and the prim is in the scene-graph cache, and UDP
otherwise — and after a capability request that failed, so a lost GET cannot
leave a caller waiting. The command takes the serial of a listing the caller
holds, and a prim still at it is answered `Event::TaskInventoryUnchanged` on
either road (the capability's `304`, or a UDP reply whose file is not
fetched). The reply codec is `sl-proto/src/task_caps.rs`, both directions;
`SimCaps` hands the request to its driver (`CapsDispatch::TaskInventory`) and
the fake grid's Second-Life flavour answers it from the region's world.
Measured on aditi by `task-inventory`: `book/src/gridspec/building.md`
§ Contents.
