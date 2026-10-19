---
id: viewer-inventory-marketplace-operations
title: Inventory marketplace operations
topic: viewer
status: ready
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

## Parity-audit addendum (2026-08-19)

Parity-audit extension: Received-Items (inbox) folder visibility
toggles — show the inbox folder in the regular inventory tree
(`FSShowInboxFolder`) and always show the inbox button/panel even when
empty (`FSAlwaysShowInboxButton`).

## From gridspec-marketplace (2026-10-19)

Second Life grants `DirectDelivery` to a storeless avatar and answers its
every route `404 Not found` (`text/plain`), which the client reads as "not a
merchant" and, for the listings, as an SLM error; OpenSim grants none
(`book/src/gridspec/economy.md` § Marketplace). The fake grid's Second Life
flavour serves exactly that answer, so the storeless path (the reference's
"not a merchant" listings window) can be e2e-tested offline. A merchant's
answers are unmeasured.
