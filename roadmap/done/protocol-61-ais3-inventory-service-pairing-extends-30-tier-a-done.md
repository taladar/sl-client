---
id: protocol-61
title: AIS3 inventory service pairing (extends #30, Tier A). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**61. AIS3 inventory service pairing (extends #30, Tier A). ✅ Done.**
`sl-wire/src/inventory.rs` had the AIS3 URL + request-body builders. Added the
inverse server-side surface: URL-suffix parsers
(`parse_ais_create_category_url`,
`parse_ais_category_url`, `parse_ais_category_children_url`,
`parse_ais_category_children_fetch_url`, `parse_ais_item_url` — distinguishing
the `/children` sub-path and re-clamping `depth`), request-body parsers
(`parse_ais_create_category_body` → `AisCategoryCreate`,
`parse_ais_rename_category_body`, `parse_ais_move_body`,
`parse_ais_update_item_body` → `AisItemUpdate`,
`parse_create_inventory_category_request` → `CreateInventoryCategoryRequest`),
and the response builders (`build_ais_update_response` over a new `AisUpdate`
change-set struct mirroring `AISUpdate::parseMeta`'s `_`-prefixed wire keys —
`_created_categories`/`_created_items`/`_updated_categories`/
`_updated_category_versions`/`_categories_removed`/`_category_items_removed`/
`_removed_items`/`_broken_links_removed`, omitting empty change-sets; and
`build_create_inventory_category_response`). Built on #52's `Llsd::to_llsd_xml`
and `parse_llsd_xml`, re-exported through `sl-proto`, same
private-intra-doc-link
gotcha as #54–#60. *Test: unit round-trip (URL/body builders → parsers, and
response builders re-parsed via `parse_llsd_xml`).* **Next = #62/F11.**
