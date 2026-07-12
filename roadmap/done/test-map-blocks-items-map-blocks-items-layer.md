---
id: test-map-blocks-items
title: map blocks/items/layer
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`map-blocks-items` — map blocks/items/layer. `1av`. **Green on OpenSim.**
    Drives all three world-map UDP round-trips against the current region:
    [`Command::RequestMapBlocks`] over a small grid-coordinate rectangle
    around the agent's own region (a one-cell margin on each side, so the
    multi-cell rectangle path is exercised too) → drains the
    [`Event::MapBlock`] entries to a quiet gap and asserts the agent's own
    region is among them; [`Command::RequestMapItems`] for
    [`MapItemType::AgentLocations`] targeting the current region
    (`RegionHandle(0)`) → asserts one [`Event::MapItems`] arrives echoing the
    requested type with at least one item (OpenSim always sends a placeholder
    green dot for a lightly-populated region); and
    [`Command::RequestMapLayer`] → asserts one [`Event::MapLayers`] with at
    least one image-tile layer (OpenSim's `RequestMapLayer` always answers
    with a single built-in whole-grid tile). Records the three round-trip
    latencies, the block/item/layer counts, the agent's grid coordinates, and
    the resolved region name. On the local grid the block rectangle returns
    **4** regions (the multi-region teleport-test set), the item reply the
    single green-dot placeholder, and the layer reply OpenSim's one built-in
    tile; region `Default Region` at grid `1000,1000`. **No new client code**
    — the whole command/event surface (`request_map_blocks`,
    `request_map_items`, `request_map_layer`, `MapRegionInfo`, `MapItem`,
    `MapItemType`) already existed and `sl-survey` uses the same
    `RequestMapBlocks` path to enumerate regions. `[both]`; the aditi run is
    deferred with the batch (SL answers the same UDP requests; it may
    additionally serve the layer tile over a CAPS path, but the UDP replies
    still arrive).
