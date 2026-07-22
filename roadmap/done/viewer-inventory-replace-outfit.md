---
id: viewer-inventory-replace-outfit
title: Replace Current Outfit (whole-outfit swap)
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — shipped
  greyed (UNIMPLEMENTED)
refs: [viewer-inventory-context-actions, viewer-inventory-cof-maintenance]
---

Context: [context/viewer.md](../context/viewer.md).

The folder context menu's **"Replace Current Outfit"**: take everything worn
off (clothing layers and attachments — body parts stay unless the folder
supplies replacements) and wear the folder's contents instead, as one
operation. "Add To" / "Remove From Current Outfit" are live
(`inventory_actions.rs`, `outfit_add_commands` / `outfit_remove_commands`);
Replace needs the safe swap ordering the reference uses (never leave the
avatar without a body part, batch the detaches via
`RezMultipleAttachmentsFromInv`'s `DetachOrder::DetachAllFirst`).

Pairs naturally with [[viewer-inventory-cof-maintenance]] — on a COF grid the
swap should rewrite the Current Outfit Folder's links too.

Reference (Firestorm, read-only): `llappearancemgr.cpp`
(`wearInventoryCategory` / `updateCOF`).

Shipped 2026-07-22: outfit_replace_commands (pure, unit-tested)
implements the reference's append==false semantics — body parts kept
unless the folder replaces the slot, clothing swapped wholesale, worn
attachments not in the folder detached explicitly and the folder's
objects added alongside (per-point ADD-bit semantics; the modern
reference never sends FirstDetachAll either). On a COF grid the swap
also rewrites the Current Outfit Folder's links.
