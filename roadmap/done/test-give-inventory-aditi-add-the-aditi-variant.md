---
id: test-give-inventory-aditi
title: Give inventory — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `give-inventory` case
(`[[test-give-inventory]]`, already green `[opensim]`): **green on aditi
live** (2026-08-12, Phase Z) after two case changes for Second Life:

- The notecard-creation wait captures the first `UpdateCreateInventoryItem`
  and then asserts the item name, so a grid that echoes the item back
  differently fails with a named mismatch instead of a silent timeout.
- **Second Life relays no giver-facing acceptance IM.** OpenSim's
  `InventoryTransferModule` sends the giver an `IM_INVENTORY_ACCEPTED` IM
  when the recipient accepts; SL sends none (the modern viewer confirms
  the give via inventory/AIS state, verified live — zero IMs in the accept
  window). The `InventoryAccepted` wait is therefore OpenSim-only (full
  `REPLY_TIMEOUT` + hard assert); on aditi it is a short best-effort probe
  recorded as `giver_ack_seen` (observed `false`). The **authoritative
  confirmation on both grids** is the offered item's copy landing in the
  recipient's Notecards folder, now matched by the unique per-run **name**
  (SL assigns the recipient's copy id on accept, so the id is not knowable
  from the offer bucket the way it is on OpenSim).
