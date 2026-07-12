---
id: protocol-5
title: Inventory
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**5. Inventory — login skeleton + UDP and HTTP-CAPS fetch · 8 pts. ✅ Done (UDP

- CAPS).** Fetch the folder/item tree. Implemented: the login request asks for
`inventory-root` + `inventory-skeleton`, and the response parser extracts the
root folder id and the full folder skeleton (every folder's
id/parent/name/type/version), surfaced as `Event::InventorySkeleton` +
`Session::inventory_root()`. Folder *contents* are available over **both**
transports, both surfaced as `Event::InventoryDescendents` with
`InventoryFolder`
- `InventoryItem` value types (full permissions, asset id, types, sale info):
- **UDP** — `Session::request_folder_contents` (`FetchInventoryDescendents` →
  `InventoryDescendents`), wired as `Command::RequestFolderContents`. Simple,
  one folder per call; OpenSim splits the reply across packets.
- **HTTP CAPS** — `Command::FetchInventoryFolders` (batch), the modern path used
  on Second Life. The capability map is now a first-class runtime concept: each
  runtime fetches the seed once per region (requesting
  `REQUESTED_CAPABILITIES`), caches the `cap → URL` map, drives the
  `EventQueueGet` long-poll off it, and POSTs `FetchInventoryDescendents2` for
  inventory; the LLSD response is decoded by `Session::handle_caps_event` into
  the same event. The capability-map caching refactor also sets up future CAPS
  calls (textures, mesh, AIS3, …).

Verified live against the local OpenSim on both paths (20-folder skeleton; root
fetch returning all 17 system sub-folders — three UDP packets vs one CAPS
response). Deferred: AIS3 (`InventoryAPIv3`) REST semantics, and inventory
*mutation* (`BulkUpdateInventory`/`UpdateInventoryItem` watching,
move/copy/delete/create). Prerequisite for appearance (Current Outfit Folder,

## 20) and giving items over IM (#2). *Test: local OpenSim (both paths

`Cap_FetchInventoryDescendents2` is enabled by default).*
