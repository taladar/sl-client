---
id: viewer-unticking-for-sale-erased-the-price
title: Unticking For Sale threw the item's price away — on screen and on the grid
topic: viewer
status: done
origin: found live-checking the keyed properties floater on the local grid
  (2026-09-07)
refs: [viewer-inventory-item-properties, viewer-keyed-floater-audit,
  viewer-item-price-field-silently-dropped]
---

Context: [context/viewer.md](../context/viewer.md).

Unticking **For Sale** in an item's properties made the price field show `10`
instead of the price the item had. Re-ticking then offered a number the owner
never chose — and, worse, the save wrote it back.

## The model could not hold the fact

`ItemInfo::sale` was `Option<(SaleType, LindenAmount)>`: "not for sale" was
`None`, so an unoffered item had **nowhere to keep its price**. That was carried
through the whole stack:

- **Decoding** dropped it — `linden_price_from_wire` returned `None` for a
  not-for-sale item, discarding the `SalePrice` the grid had sent.
- **Encoding** wrote a zero — `linden_price_to_wire(None) == 0` — so unticking
  For Sale and saving **erased the grid's stored price**. That is data loss, not
  a display quirk.
- The properties floater papered over the gap with a hardcoded `10` fallback,
  which is where the number on screen came from.

The wire has never worked that way: `SaleType` and `SalePrice` are two
independent fields, and the reference keeps both in one `LLSaleInfo` regardless
of whether the item is offered.

## The fix

`SaleInfo { sale_type, price }` replaces the `Option` pair in `ItemInfo`.
Whether an item is offered is `sale_type != NotForSale`
(`SaleInfo::is_for_sale`), the price rides along either way, and both halves
always reach the wire. The properties floater reads the item's own price
instead of a fallback, and the For Sale toggle keeps it
(`SaleInfo::not_for_sale(price)`).

`unticking_for_sale_keeps_the_price` pins both ends: the toggle keeps the
number, and `to_wire_item` still carries it with `NotForSale`.

## Related

The same session's [[viewer-item-price-field-silently-dropped]] is the other
half of this row's behaviour: the price field is disabled while the item is not
offered, so a value that cannot be committed cannot be typed either.
