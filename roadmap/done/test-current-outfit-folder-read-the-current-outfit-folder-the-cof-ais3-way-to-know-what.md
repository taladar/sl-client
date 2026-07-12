---
id: test-current-outfit-folder
title: read the Current Outfit Folder (the COF / AIS3 way to know what the av
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 14 — Appearance, attachments & animations `[both]`
---

Context: [context/test.md](../context/test.md).

`current-outfit-folder` — read the Current Outfit Folder (the
COF / AIS3 way to know what the avatar is wearing, complementing the legacy
`wearables-request`). `1av`. Under server-side baking the authoritative
outfit is the `FT_CURRENT_OUTFIT` system folder, whose contents are inventory
*links* back to the worn wearables/attachments; the case locates that folder
and fetches every agent folder's contents (the modern CAPS
`FetchInventoryDescendents2` on SL, UDP `FetchInventoryDescendents` on
OpenSim), then dereferences each COF link to its target item (a link's
`asset_id` is the linked item's id) and asserts the outfit links resolve —
including the four mandatory body parts the legacy `AgentWearablesUpdate`
omits on modern SL. **Complete on BOTH grids** (green OpenSim: COF has 6
links, all 4 body parts; green aditi: 14 links, all 4 body parts) — so it
fully resolves on aditi *where `wearables-request` is only `partial`*, the
mirror grid-divergence the roadmap predicted, and OpenSim populates its COF
with body-part links too. Two library changes fell out of it: (1) a new
**`Command::QueryInventoryFolders` → `Event::InventoryFolders`** local query
(backed by `Session::inventory_folder_infos`) that snapshots the agent tree's
folders from the login-skeleton-seeded model — the reliable way to find the
COF by its preferred type, since SL's `FetchInventoryDescendents2` does not
echo a folder's `type_default` in a descendents reply; (2) a **latent SL
inventory-read bug fixed** — SL's `FetchInventoryDescendents2` encodes the
nested category/item ids as LLSD `string` (only the top-level folder envelope
uses `uuid`), so the strict `uuid_member` parse was silently nil-ing every id
and the placeholder filter dropped the entire contents of every CAPS-fetched
folder on SL; the descendents parser now reads those ids leniently
(regression-tested). `[both]`.
