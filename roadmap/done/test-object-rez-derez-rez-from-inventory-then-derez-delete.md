---
id: test-object-rez-derez
title: rez from inventory, then derez/delete
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

`object-rez-derez` — rez from inventory, then derez/delete. `1av`.
The full object lifecycle, each leg confirmed by an object-update event:
**create** a throwaway cube with [`Command::RezObject`] (`ObjectAdd`, the
build-tool new-prim path) placed a metre above a reference primitive in
the region ([`Event::ObjectAdded`] with a region-local id not seen during
the initial scene settle); **take** it into the agent's Objects folder
with [`Command::DerezObjects`] /
[`DeRezDestination::TakeIntoAgentInventory`], which removes the world
object and materialises the inventory item
([`Event::InventoryItemCreated`]); **rez that item from inventory** with
[`Command::RezObjectFromInventory`] (a second [`Event::ObjectAdded`] with
a fresh id — the operation the roadmap item names); and **delete** it to
the Trash with [`Command::DerezObjects`] / [`DeRezDestination::Trash`],
confirmed by the [`Event::ObjectRemoved`] (`KillObject`), leaving the
scene as found. This is the first case to construct a
[`RezObjectParams`] / [`RestoreItem`], so it re-exports both from the two
runtime crates (as commit `d41e378` did for `ObjectPropertiesFamily`); no
other new client code — the whole rez/derez surface already existed.
Force-deleting via `ObjectDelete` ([`Command::DeleteObjects`]) is a no-op
on stock OpenSim, so the portable delete is the derez-to-Trash; OpenSim
resolves the caller's own Trash for a `Delete` derez regardless of the
destination id and looks the source item up by id alone for the rez (the
payload's permission masks and CRC are not validated), so the round trip
is self-contained on the local grid. Reuses the `start_location` hook
(Default Region on OpenSim, where the workspace's rezzed test object is
the placement reference; a no-primitive landing region records `partial`
on SL). The take leaves the created item in the Objects folder and the
final delete a copy in Trash — bounded inventory residue of two items per
run, acceptable on a throwaway grid. Green on OpenSim: create / take /
rez / delete all confirmed against distinct created vs rezzed objects,
RTTs ≈ 15–90 ms. `[both]`; the aditi run is deferred with the batch (no
aditi record this session).
