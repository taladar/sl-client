---
id: viewer-ais3-inventory-mutations-and-cof-reconverge
title: Route inventory mutations through AIS3 on SL + reconverge the COF on wear
topic: viewer
status: done
origin: user report (2026-07-31, aditi — "Cannot create requested inventory")
---

Context: [context/viewer.md](../context/viewer.md).

## Problem

Wearing / adding a layer on Second Life failed with the sim alert **"Cannot
create requested inventory."** The viewer created Current-Outfit-Folder (COF)
links over the legacy UDP `LinkInventoryItem`, but modern SL rejects a UDP link
against the AIS-managed COF — the reference viewer creates it via
`AISAPI::CreateInventory` when the `InventoryAPIv3` cap is present, falling back
to UDP only on OpenSim. Our client had **no AIS3 create path at all**. Once wear
worked, the follow-on surfaced: the inventory panel (`(worn)` / bold labels, the
Current Outfit folder) did not reconverge, and a BoM-layer take-off did not
re-texture the avatar.

## Done (2026-07-31)

- **AIS3 create-link** (`sl-wire::build_ais_create_link_body` +
  `POST /category/<COF>?tid=` with a `links` array).
  `Session::link_inventory_item` is routed to AIS3 in both runtimes when
  `CAP_INVENTORY_API_V3` is present, UDP otherwise. Verified live on aditi: no
  "Cannot create requested inventory", COF version advances, avatar re-bakes.
- **Sibling routing (cap-gated, so OpenSim is untouched):** folder/item move,
  item/folder remove, and purge route through their existing `Ais3*` HTTP verbs
  when the cap is present. Deliberately kept on UDP (documented in-code): item
  *create* (upload-cap path), full item update (permissions), copy, move-with-
  rename — no clean 1:1 AIS3 equivalent.
- **COF reconvergence:** AIS3 mutation replies' `_updated_category_versions` are
  parsed (`ais_updated_category_versions`) and each advanced folder is
  `invalidate_folder`'d — flipped to `Unknown` (and its cached version
  advanced), so the background crawl re-fetches it → `InventoryDescendents` →
  the viewer re-queries → the model reconverges (a DELETE reply names no removed
  item, so nothing else drops a stale COF link). Plus an **optimistic** cache
  removal (`remove_inventory_items_local`) on the AIS3 DELETE path so a take-off
  / detach clears the `(worn)` label at once.
- **Re-bake on COF reconverge** (`appearance.rs`): a re-fetch of the COF (its
  version advanced) re-runs the server bake — the reference's
  `updateAppearanceFromCOF` after `removeCOFItemLinks`. This re-textures a
  **COF-only** BoM-layer take-off that never touches the legacy `AgentWearables`
  set. Verified live on aditi
  (`Current Outfit Folder reconverged; re-running the server appearance bake`,
  COF 100→108, all accepted, 0 errors).

## Non-obvious facts (not elsewhere in git/roadmap)

- The viewer emitted **zero** `Ais3*` commands before this — its entire
  inventory mutation surface was UDP; the `Ais3*` commands existed but only the
  REPL used them.
- Routing is cap-gated at the runtime command dispatch (both runtimes),
  mirroring the reference's `AISAPI::isAvailable()` branch — so OpenSim
  behaviour is unchanged (no `InventoryAPIv3` cap → UDP as before), and only the
  SL AIS3 path is new/untested-per-op (verify each op on aditi if extending).

## Follow-ups (filed as bugs)

- [[viewer-inventory-worn-markers-show-early]] — `(worn)` / bold appear late; an
  eager COF fetch attempt regressed to an empty Current Outfit (reverted).
- [[viewer-inventory-worn-tab-items-without-folders]]
- [[viewer-inventory-clothing-layers-shirt-icon]]
