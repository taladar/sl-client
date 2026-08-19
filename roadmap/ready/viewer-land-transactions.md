---
id: viewer-land-transactions
title: Land sale & transfer actions — sell, deed, abandon, reclaim, buy pass
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-parcel-options-general, viewer-money-economy-ui,
  viewer-land-holdings, viewer-land-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

The reference About Land General tab carries the land-transaction button
row: Sell Land… (the `floater_sell_land.xml` flow — price, sell to anyone
or a specific resident, sell-objects-with-land radio, Show Objects),
Cancel Land Sale, Deed to Group with the optional "owner makes
contribution with deed" checkbox, Abandon Land, Reclaim Land, Buy For
Group, and Buy Pass, each behind a confirmation dialog.

Our nine-tab About Land floater
(`sl-client-bevy-viewer/src/about_land.rs`) has none of these:
`AboutLandAction` covers only Apply/Refresh/pickers/landing/access-add,
and the land pie's Buy This Land / Buy Pass slices are greyed
`UNIMPLEMENTED` in `sl-client-bevy-viewer/src/land_menu.rs`. Every write
path already exists unused in sl-proto (`sl-proto/src/command.rs`):
`ParcelBuy` (with the group-owned variant for Buy For Group),
`ParcelDeedToGroup`, `ParcelRelease` (abandon), `ParcelReclaim`,
`ParcelBuyPass`, and `ParcelUpdate.sale_price` / `auth_buyer_id`
(`sl-proto/src/types/parcel.rs`) for setting and cancelling the sale
state.

This task adds the General-tab button row with its confirmation dialogs,
the Sell Land floater, the deed-to-group confirmation (plus contribution
checkbox), and wires the two greyed pie slices. The Buy Land purchase
floater itself (currency estimation, covenant agree, group-contribution
removal) stays with [[viewer-money-economy-ui]].

Reference (Firestorm, read-only): `indra/newview/llfloaterland.cpp`
(LLPanelLandGeneral), `indra/newview/llfloatersellland.cpp`,
`indra/newview/skins/default/xui/en/floater_sell_land.xml`,
`indra/newview/skins/default/xui/en/floater_about_land.xml`.
