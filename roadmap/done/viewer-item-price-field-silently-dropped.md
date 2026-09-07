---
id: viewer-item-price-field-silently-dropped
title: The item-properties price field took a value it never saved
topic: viewer
status: done
origin: found live-checking the keyed properties floater on the local grid
  (2026-09-07)
refs: [viewer-inventory-item-properties, viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

Typing a **sale price** into an item's properties and pressing Enter did
nothing when **For Sale** was not ticked: the value vanished on the next open,
and nothing said why. Permission changes committed fine, and so did a price on
an item that *was* for sale.

## Why, and why it is not a save bug

`commit_text_edits` applies a typed price only to an item with a sale:

```text
if item.sale.is_some()
    && let Some(price) = read(ui.price_field)…
```

That gate is correct. `ItemInfo::sale` is `Option<(SaleType, LindenAmount)>` —
"not for sale" has no price slot to hold the number, and the wire says the same
thing (`SaleType::NotForSale` with no price). Verified against the grid: with
For Sale unticked, the item's `salePrice` row never changed, so nothing was
lost in transit.

The bug was the **affordance**: the window showed a live, focusable price field
for an item that was not for sale, and quietly ignored what was typed into it.

## The fix

The price field is spawned with
[`InteractionDisabled`](bevy::ui::InteractionDisabled) while `item.sale` is
`None` — the text widget greys it and refuses focus — so the value cannot be
typed in the first place. Ticking For Sale re-opens the window (that is how
every checkbox repaints) and the field comes back live carrying the value it
was showing. This is what the reference does with its price spinner, which is
enabled only while its For Sale box is checked.

`the_price_field_is_dead_until_the_item_is_for_sale` pins both halves.

Reference (Firestorm, read-only): `llfloaterproperties.cpp` (`refresh` /
`updateSaleInfo`, which enables `EditPrice` from the sale checkbox).
