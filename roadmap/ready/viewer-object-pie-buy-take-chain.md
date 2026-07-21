---
id: viewer-object-pie-buy-take-chain
title: Object pie Buy slices + the reference Buy/Take autohide chain
topic: viewer
status: ready
origin: follow-up from viewer-object-context-menu (2026-07-21)
refs: [viewer-object-context-menu, viewer-ui-radial-menu, viewer-object-menu-reorder-when-implemented, api-g6]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-object-context-menu]] left the object pie's two `Buy` slices greyed
(`UNIMPLEMENTED`) and — deliberately — did **not** reproduce the reference's
west-slot autohide chain, where a `Buy` slice hides the `Take >` sub-pie when
the picked object is for sale. Two reasons, both to resolve here:

- **Buying itself is unwired in the viewer.** The wire path is done
  ([[api-g6]]: `ObjectBuy` with sale-type / price echo — the
  purchase/pay/spin families), and the object pie already fires
  `RequestObjectPropertiesFamily` on open, whose reply carries `sale_type` and
  `sale_price` — so the open-time conditions can know "for sale". What is
  missing is the purchase UI: a confirmation surface showing name, price and
  sale type (contents / copy / original) before any money moves. A pie slice
  must never buy silently on a flick.
- **A chain member that is a sub-pie is not expressible.** `pie_menu`'s
  `PieContent::Chain` holds only `PieAction`s; the reference chain is
  `Buy` (action) / `Take >` (sub-pie). Extend the widget so a chain link can
  be a sub-pie. Navigation (`sub_pie_at` / `follow`) resolves sub-pies
  condition-independently today, which is safe as long as **at most one link
  of a chain is a sub-pie** — enforce that by construction or by test.

Scope:

- Wire both `Buy` addresses (west chain, and the More > south-east slice) to
  the purchase flow, enabled on a new for-sale condition fed from the
  properties-family reply.
- Extend `PieContent::Chain` (or a successor) to admit one sub-pie link;
  re-declare the west slot as the reference chain `Buy` / `Take >`.
- Update the committed `object_pie_keeps_every_address` table in the same
  commit — the addresses themselves must not move (both chain members keep
  the west address, as chains already report).
