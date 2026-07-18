---
id: viewer-inventory-marketplace-operations
title: Inventory marketplace operations
topic: viewer
status: ready
origin: reference-viewer parity notes on viewer-inventory-folder-tree (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The **marketplace** operations the reference viewer exposes from inventory,
beyond the plain folder tree ([[viewer-inventory-folder-tree]], done): the
**Received Items** (marketplace inbox) folder, and moving items into / managing
the **Marketplace Listings** folder (list, unlist, associate versions/stock).
The special folder types are already resolved
(`FolderType::{Inbox, Outbox, MarketplaceListings, MarketplaceStock,
MarketplaceVersion}`); this task is the operations on them.

Reference (Firestorm, read-only): `llmarketplacefunctions`,
`llpanelmarketplaceinbox`, `llmarketplacenotifications`.
