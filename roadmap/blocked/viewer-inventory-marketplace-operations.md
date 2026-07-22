---
id: viewer-inventory-marketplace-operations
title: Inventory marketplace operations
topic: viewer
status: blocked
blocked_by: [protocol-slm-directdelivery]
origin: reference-viewer parity notes on viewer-inventory-folder-tree (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The **marketplace** operations the reference viewer exposes from inventory,
beyond the plain folder tree ([[viewer-inventory-folder-tree]], done) — the
**UI / validation half**, on top of the SLM transport
([[protocol-slm-directdelivery]], which owns the new `sl-marketplace`
crate and the DirectDelivery commands):

- a listings model keyed by listing folder (id, listed state,
  `edit_url`, version folder, count on hand) fed from the SLM events;
- the marketplace context-menu block gated to the Listings subtree
  (create listing, associate, activate / deactivate, edit on the web
  via `edit_url`);
- the client-side structure logic — nesting-depth validation, stock
  counting (`compute_stock_count`), unique-version-folder pickup, drag
  validation into the subtree — pure and unit-testable offline;
- listing-folder row decorations ("(active)" / "(unassociated)", stock
  counts) and the **Received Items** fresh-count badge (the inbox
  itself is an ordinary folder the grid fills; no protocol needed —
  the badge and decorations are doable ahead of the transport).

The special folder types are already resolved
(`FolderType::{Inbox, Outbox, MarketplaceListings, MarketplaceStock,
MarketplaceVersion}`); moving items in / out already works through the
normal move / drag paths.

Reference (Firestorm, read-only): `llmarketplacefunctions`,
`llpanelmarketplaceinbox`, `llmarketplacenotifications`.
