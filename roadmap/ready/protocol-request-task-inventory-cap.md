---
id: protocol-request-task-inventory-cap
title: Fetch task inventory over the RequestTaskInventory capability
topic: protocol
status: ready
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
