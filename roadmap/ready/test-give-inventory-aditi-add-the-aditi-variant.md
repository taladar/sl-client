---
id: test-give-inventory-aditi
title: Give inventory — [aditi] variant
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Add the `[aditi]` variant of the `give-inventory` case
(`[[test-give-inventory]]`, green `[opensim]`). The case's `grids()` now
includes `Grid::Aditi`, but the live aditi run **does not yet pass** — it
needs one more iteration.

Groundwork already committed: the notecard-creation wait captures the
first `UpdateCreateInventoryItem` and then asserts the item name (so a
grid that echoes the item back differently fails with a named mismatch
instead of a silent timeout).

**Open problem (2026-08-12 live):** creation now succeeds on SL (the
`UpdateCreateInventoryItem` and the follow-on offer
`ImprovedInstantMessage` are both observed), so the case reaches the
give/accept round-trip and times out *later* — at step 5, the primary
(giver) observing the recipient's acceptance
([`Event::InstantMessageReceived`] with `ImDialog::InventoryAccepted`).
Second Life very likely does not relay an `IM_INVENTORY_ACCEPTED` IM back
to the giver the way OpenSim's `InventoryTransferModule` does (the modern
viewer confirms the give via inventory/AIS state, not a giver-facing IM).
Next step: trace which of steps 3-5 stalls (add a per-step marker), and
if SL genuinely sends no giver-facing acceptance IM, confirm the transfer
by the recipient-side signal instead (the accepted item appearing in the
secondary's Objects/target folder, already re-fetched in step 4) and mark
the giver-facing-IM assertion OpenSim-only.
