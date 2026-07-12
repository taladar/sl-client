---
id: test-task-inventory
title: request/update a prim's task inventory
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

`task-inventory` — request/update a prim's task inventory. `1av`.
A prim carries its own inventory (the scripts / notecards / sounds / objects
a build drops into it); a viewer learns its contents by requesting them
([`Command::RequestTaskInventory`]) and receives an
[`Event::TaskInventoryReply`] carrying the current *contents serial* (bumped
on every change, so a client can tell whether a cached listing is stale) plus
the temporary Xfer filename to download the full listing from — the serial
alone is enough to observe a write landing. Rather than depend on a
pre-populated prim the case manufactures a self-contained fixture like
`object-rez-derez`: it rezs a throwaway **container** cube
([`Command::RezObject`]) and a second **donor** cube which it takes into the
Objects folder ([`Command::DerezObjects`] /
[`DeRezDestination::TakeIntoAgentInventory`]) to materialise an agent
inventory item; requests the container's task inventory (serial `0`, empty
filename — a fresh cube is empty); drops the taken item in with
[`Command::UpdateTaskInventory`] / [`TaskInventoryKey::Item`] (OpenSim
resolves the item by id from the agent's own inventory and copies it in);
requests again and asserts the serial advanced (`0` → `1`) and the filename is
now non-empty; then derezes the container to Trash ([`Event::ObjectRemoved`])
to leave the scene as found. The two replies are correlated to the container
by their task id so a stray reply is skipped. No new client code — the
`RequestTaskInventory`/`UpdateTaskInventory` surface all existed; this case
only re-exports [`TaskInventoryKey`] / [`TaskInventoryReply`] from the two
runtime crates (as commit `d41e378` did for `ObjectPropertiesFamily`), plus
`ObjectKey` which was missing from both runtime re-export lists. Reuses the
`start_location` hook and the settle/reference machinery of `object-rez-derez`
(Default Region on OpenSim, where the workspace's rezzed test object is the
placement reference; a no-primitive landing region records `partial` on SL).
Green on OpenSim: serial `0` → `1`, request and update RTTs ≈ 15 ms. The take
leaves the donor item in the Objects folder and the container's copy of it
goes to Trash with the container — bounded inventory residue, acceptable on a
throwaway grid. `[both]`; the aditi run is deferred with the batch (no aditi
record this session).
