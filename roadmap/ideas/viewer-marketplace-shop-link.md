---
id: viewer-marketplace-shop-link
title: Shop — open the grid's marketplace storefront
topic: viewer
status: ideas
origin: audit of menu entries still gated on UNIMPLEMENTED (2026-08-24)
refs: [viewer-inventory-marketplace-operations, viewer-web-openid-auth]
---

Context: [context/viewer.md](../context/viewer.md).

The inventory menu's **Shop...** entry, and the menu bar's future
marketplace entry (`menu_bar.rs`, noted there as such). Both are a link,
not a feature: open the grid's marketplace storefront in the in-viewer
browser.

Kept apart from [[viewer-inventory-marketplace-operations]] deliberately.
That task is a real viewer feature — the SLM listings model, the
DirectDelivery commands, the listing / version-folder validation shown in
the inventory tree. This one opens a URL, and the only reasons it is not a
two-line change are:

- the storefront URL is **grid-specific**. Second Life has one; most OpenSim
  grids have none, and the entry should be absent rather than greyed on a
  grid that cannot answer it — the same shape as the OpenID path in
  `sl-viewer-media/src/web_auth.rs`, which stays dormant off Second Life.
- it should open **already signed in**, which it will: the OpenID cookie
  minted at login ([[viewer-web-openid-auth]]) is already injected into the
  embedded browser's shared request context, and the marketplace is one of
  the hosts it was minted for.

So the work is the URL source, the per-grid presence rule, and routing the
entry at the web floater rather than the system browser.
