# Inventory

What each grid hands a viewer to read its inventory with, and what a change
does to the folder versions a cached tree is checked against. The messages
and the client's model are described in [Inventory](../content/inventory.md).

## Fetching

Measured on 2026-10-21 on Second Life's beta grid (aditi) and the local
OpenSim standalone by the census `inventory-fetch` (one avatar), run eight
times on aditi and as often on OpenSim while it grew, and held to the table
below on the last two runs of each. Every answer is asked of the grid **raw**:
the capability's own request, and the UDP messages sent with the circuit
probe on, so a reply's road is the datagrams that arrived. The census crawls
both trees whole (the test avatar's tree is three levels deep), and the
folders it makes are removed again; a run costs nothing.

The census holds both live grids and both fake flavours to these answers as
`Measured` constants (`sl-conformance/src/cases/inventory_fetch.rs`), except
where its `FAKE_EXEMPT` list names a gap and the task that closes it.

### The login skeleton

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| fields of an `inventory-skeleton` / `inventory-skel-lib` entry | `folder_id`, `name`, `parent_id`, `type_default`, `version` | the same | the same |
| the root | type 8, nil parent | the same | the same |
| against a crawl of the whole tree | every folder named, every version and parent the fetch states | the same | the same |
| every version positive | yes | yes | yes |
| the Library's owner (`inventory-lib-owner`) | another agent | another agent | another agent |
| inventory messages in `udp_blacklist` | none | none (no blacklist) | none |

The skeleton is the whole folder tree on both grids, so a viewer can check
every cached folder against it without a fetch.

### The capabilities

| capability | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `FetchInventoryDescendents2`, `FetchInventory2`, `CreateInventoryCategory` | granted | granted | each flavour as its grid |
| `FetchLibDescendents2`, `FetchLib2`, `InventoryAPIv3`, `LibraryAPIv3` | granted | **not** granted | each flavour as its grid |

### The folder fetch (`FetchInventoryDescendents2`)

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| answer | `200`, `{ folders: [...] }` | the same | the same |
| a folder entry | `agent_id` (a **string**), `categories`, `descendents`, `folder_id`, `items`, `owner_id` (the agent), `version` | the same, `agent_id` a UUID | each flavour as its grid |
| a sub-folder entry | `agent_id` (string), `category_id`, `name`, `parent_id`, `type_default`, `version`; ids as **strings** | no `agent_id`; ids as UUIDs | each flavour as its grid |
| an item | every id a string, `agent_id`, an empty `metadata` map, `permissions` with `last_owner_id` and no `is_owner_group` | ids as UUIDs, no `agent_id` or `metadata`, `permissions` with `is_owner_group` and no `last_owner_id` | each flavour as its grid |
| `descendents` | the sub-folders plus the items listed | the same | the same |
| a folder nobody has | **not refused**: an empty entry, version 0, descendents 0, owned by the agent (no `bad_folders`) | the same | the same |
| a Library folder through the agent's capability | that same empty entry | **listed** | each flavour as its grid |
| the Library is crawled through | `FetchLibDescendents2` | `FetchInventoryDescendents2`, the Library owner as owner | each flavour as its grid |
| nil-id sub-folders in a whole crawl | none | none | none |

The *Known already* note that OpenSim's Library leaves carry a phantom
nil-id folder did not reproduce: a raw crawl of both trees through the
capability found none on either grid.

### The item fetch (`FetchInventory2` / `FetchLib2`)

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| answer | `200`, `{ agent_id, items }` | the same | the same |
| an item against the folder fetch's entry for it | identical | identical | identical |
| an item nobody has | `200`, no items (no `bad_items`) | the same | the same |
| a Library item through `FetchInventory2` | `200`, no items | the same | the same |
| a Library item through `FetchLib2` | identical to the folder fetch's | no capability | each flavour as its grid |

### The UDP fetches

| sent raw | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `FetchInventoryDescendents`, the agent's root | **nothing** | `InventoryDescendents`, over several packets | Second Life flavour: a `FeatureDisabled` (a deliberate deviation, below); OpenSim flavour: `InventoryDescendents` |
| `FetchInventoryDescendents`, the Library's root | nothing | `InventoryDescendents` | as above |
| `FetchInventory`, one item | nothing | `FetchInventoryReply` | each flavour as its grid |

The fake grid's Second Life flavour answers the deprecated folder fetch with a
`FeatureDisabled` rather than aditi's silence on purpose
(`LegacyUdpInventory`): only an answer leaves something to assert. The census
does not hold it to the silence.

### AIS3 (`InventoryAPIv3`, Second Life only)

| behaviour | Second Life | fake grid (Second Life flavour) |
| --- | --- | --- |
| `GET /category/<id>` | `200`: the folder, with `_base_uri` (`slcap://InventoryAPIv3`), `_links` (`categories`, `children`, `items`, `links`, `parent`, `self`, each `{ href }` as a URI), `agent_id`, `category_id`, `name`, `parent_id`, `type_default`, `version`, ids as strings | the same |
| `GET /item/<id>` | `200`: the item, with `_base_uri`, `_links` (`parent`, `self` = `/item/<id>`), `agent_id`, ids as strings, no `is_owner_group` | the same |
| `GET /category/<id>/children?depth=N` | the folder at the top (`_links` adds `category` = `/category/<id>` named `self`, and `self` is `/children`), its contents under `_embedded` | the same |
| how far a listing reaches | `depth` + 1 levels, for depth 0, 1, 2, 3 and 100 (the tree is three deep, so a clamp above 2 cannot show) | the same |
| nesting | each folder's children under that folder's own `_embedded`, a level per level | the same |
| an embedded folder or item | `_links` (`parent`, `self`), `agent_id`, ids as strings | the same |
| a folder nobody has | `404` and an error map: `category_id`, `error_code` 4, `error_description` "Category not found.", `error_filename` `category.py`, `error_function` `Category._get`, `error_line_number` 427 | the same |
| `LibraryAPIv3`, the Library root | `200`, the same shape | the same |
| `LibraryAPIv3` asked for an agent folder | `404` "Category not found." | the same |

OpenSim grants no AIS3, and the fake's OpenSim flavour none either.

### What a change does to the versions

Each step read back through the folder fetch (and, on Second Life, through
AIS3 too, which agreed every time).

| change | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the skeleton's root version against a fetch at login | equal | equal | equal |
| a folder made with UDP `CreateInventoryFolder` | listed; the parent's version +1 | the same | the same |
| a folder made with `CreateInventoryCategory` | answered and listed; parent +1 | the same | the same |
| a folder made with AIS3 `POST` | listed; parent +1 | no AIS3 | Second Life flavour: the same |
| a UDP `UpdateInventoryFolder` rename | listed; the **parent** +1, the folder's own version unchanged (aditi stated −1 on two runs) | the parent +1, the folder unchanged | the parent +1, the folder unchanged |
| a UDP `MoveInventoryFolder` into the Trash | moved; the old parent and the Trash +1 each | the same | the same |
| a UDP `RemoveInventoryFolder` from the Trash | gone; the Trash **+1** | gone; the Trash **unchanged** | each flavour as its grid |
| an AIS3 `PATCH /category/<id>` of `parent_id` | **refused**: `400`, `error_code` 0, "Cannot change parent_id.  Use MOVE method.  *folder*, *parent* " (`categories.py`, `Category.mergeUntrusted`, line 306); nothing moves | no AIS3 | Second Life flavour: the same |
| an AIS3 `MOVE /category/<id>`, `Destination` absolute or relative | **refused**: `405` "Method Not Allowed", `error_code` 7, `allowed_methods` `DELETE, GET, PATCH, POST, PUT, HEAD, OPTIONS`, `method`, `url` (`access.py`, `Category._not_allowed`, line 61) | no AIS3 | Second Life flavour: the same |
| an AIS3 `DELETE /category/<id>` of a folder **outside** the Trash | removed where it stands; its parent +1 | no AIS3 | Second Life flavour: removed |
| an AIS3 `PATCH /item/<id>` of `parent_id` | refused: `400`, "Cannot change parent_id.  Use MOVE method. *item* *parent*" (`items.py`, `Item.mergeUntrusted`, line 732) | no AIS3 | not measured: the fake does not yet copy the item the leg moves |
| a UDP `MoveInventoryItem` | moved | moved | not measured (as above) |
| the next login's skeleton | states the versions the last session read, root and Trash | the same | **falls back**: each login is seeded afresh |

**AIS3 cannot move anything on Second Life**, folder or item. The reference
viewer never asks it to: its `AISAPI` has no move, and every move it makes —
`changeCategoryParent`, `changeItemParent` — ends in `updateParentOnServer`,
the UDP `MoveInventoryFolder` / `MoveInventoryItem`. Its AIS3 updates send a
`parent_id` only unchanged. Our client used to send moves to AIS3 on Second
Life, so every move and every delete-to-Trash there silently did nothing on
the grid; since this census both runtimes send the UDP messages on every
grid, the `Ais3MoveFolder` / `Ais3MoveItem` commands are gone, and
`ais3-folder-lifecycle` passes on aditi for the first time.

The fake grid keeping a session's inventory for the next login is
`server-fake-grid-inventory-persists-across-logins`; its serving the UDP
`CopyInventoryItem` (which the item legs need) is
`server-fake-grid-inventory-udp-mutations`.

### The cache across a login

`inventory-cache-skip` crawls the tree, logs out (the runtime writes its disk
cache), logs in again and checks that every root sub-folder comes back loaded
from the cache at the version the skeleton names, with nothing refetched.
Because the skeleton states the last session's versions on both grids (above),
the check is the same on both.

| case | Second Life | OpenSim |
| --- | --- | --- |
| `inventory-cache-skip` | passed (2026-10-21) | passed (2026-10-21) |
| `inventory-tree-crawl` | passed (2026-10-21) | passed (2026-10-21) |
| `library-tree-fetch` | passed (2026-10-21) | passed (2026-10-21) |

## Changing inventory

Measured on 2026-10-22 on aditi and the local OpenSim by the census
`inventory-mutations` (two avatars), run ten times on aditi and eight on
OpenSim while it grew, and held to the tables below on the last two runs of
each. Every operation is sent **raw** — the UDP message with the circuit probe
on, the AIS3 request with `reqwest` — because the client routes several of
them per grid; each is then read back through the folder fetch and the item
fetch, and what arrived in a five-second window after it is recorded by name
and road. The census makes two run folders, `F` and `G`, a notecard in `F`, and
copies of it for the removal legs, and takes them all away again; a run costs
nothing.

The census holds both live grids to these answers as `Measured` constants
(`sl-conformance/src/cases/inventory_mutations.rs`). The fake grid is not held
to it yet: its serving the item mutations, the Trash gating and the gives is
`server-fake-grid-inventory-udp-mutations`.

### Items over UDP

| operation | Second Life | OpenSim |
| --- | --- | --- |
| `CreateInventoryItem`, a notecard in `F` | answered with `UpdateCreateInventoryItem` (UDP, the callback id echoed); `F` +1; base and owner mask `0x7fffffff`; **no asset** (nil) | the same answer; `F` +1; base and owner mask `0x0009e000`; an asset |
| `CopyInventoryItem` into `F` | copied; answered with `BulkUpdateInventory` over the **event queue**; `F` +1 | copied; answered with `UpdateCreateInventoryItem` (UDP, the callback id echoed); `F` +1 |
| `UpdateInventoryItem`, a new name and description | applied; nothing pushed; `F` +1 | the same |
| `ChangeInventoryItemFlags` (flags 1) | the flags read back 0; nothing pushed; `F` **+1** | the flags read back 0; nothing pushed; `F` unchanged — OpenSim has no handler for the message |
| `MoveInventoryItem` from `F` to `G` | moved; `BulkUpdateInventory` over the event queue; `F` and `G` +1 each | the same |
| `LinkInventoryItem` to the notecard, into `G` | **refused**: an `AlertMessage` "Cannot create requested inventory." (UDP); nothing made | made, type 24 naming the notecard; `UpdateCreateInventoryItem` (UDP, no callback id); `G` +1 |
| `LinkInventoryItem` into the Current Outfit Folder | refused the same way | made, as into `G` |
| a notecard body saved with `UpdateNotecardAgentInventory` | saved | saved |

Second Life refuses the UDP link in every folder, not only the Current Outfit
Folder; the client links through AIS3 there. A notecard made on Second Life
names no asset until a body is saved, and neither road links to such an item
(AIS3 answers `400` "Cannot link to items with a NULL asset_id."), so the
census saves a body before its link legs.

### Removals and the Trash

| operation | Second Life | OpenSim |
| --- | --- | --- |
| `RemoveInventoryItem` of an item outside the Trash | **removed**; its folder +1, the Trash unchanged; nothing pushed | the same |
| `RemoveInventoryItem` of an item in the Trash | removed; the Trash +1; nothing pushed | the same |
| `RemoveInventoryFolder` of a folder outside the Trash | **refused silently**: still under the root, no version moves | the same |
| `PurgeInventoryDescendents` of a folder outside the Trash | **refused silently**: every item left, the folder's version unchanged | the same |
| `PurgeInventoryDescendents` of a folder in the Trash | emptied; the folder +1; nothing pushed | the same |
| `RemoveInventoryObjects`, a folder and an item, outside the Trash | the **item removed, the folder left** | the same |
| `RemoveInventoryObjects` in the Trash | both removed | the same |

So the Trash gates a folder's removal and an emptying on both grids, and an
item's removal on neither. No grid pushes anything after a removal: a viewer
that sent one has already changed its own model.

### AIS3 (Second Life only)

Every answer carries the item or folder itself at the top level (with
`_base_uri` and `_links`) beside the keys that say what changed, and nothing
is pushed over UDP or the event queue after any of them.

| request | answer | what its keys say |
| --- | --- | --- |
| `POST /category/<root>` with `categories` | `201` | `_created_categories` [the folder], `_created_items` [], `_embedded` the new folder, `_updated_category_versions` {root +1} |
| the same, naming a `category_id` | `201`, the folder made under **another id** | as above: AIS3 mints a folder's id and ignores one the request names |
| `PATCH /category/<id>` with `name` | `200` | `_updated_categories` [the folder], `_updated_category_versions` {**the parent** +1} |
| `PATCH /item/<id>` with `name`, `desc` | `200`, the item | `_updated_items` [the item], `_updated_category_versions` {its folder +1} |
| `PATCH /item/<id>` with the **whole item** (the reference's `asLLSD` without `asset_id`, the unchanged `parent_id` included) | `200`; name, description and next-owner mask all applied | as above |
| `POST /category/<id>` with `links` | `201` | `_created_items` [the link], `_embedded` {links}, `_updated_category_versions` {the folder +1} |
| `PUT /category/<id>/links`, a bare array of one link | `201` | `_updated_items` [the folder's existing link to the same item], `_linked_ids`, `_category_items_removed` [], `_created_items` [] — it rewrote the existing link rather than making one |
| `DELETE /item/<id>` outside the Trash | `200`, **removed** | `_category_items_removed` [the item], `_active_gestures_removed`, `_attachments_removed`, `_broken_links_removed`, `_wearables_removed`, `_updated_category_versions` {its folder +1} |
| `DELETE /category/<id>/children` outside the Trash | `200`, **emptied** | `_category_items_removed` [each item], `_categories_removed` [], `_total_items_removed`, and the four above |
| `DELETE /category/<id>` outside the Trash | `200`, removed | `_categories_removed` [the folder], `_updated_category_versions` {the parent +1}, and the rest above |
| `COPY` (`LibraryAPIv3`) of a Library folder, `Destination` `F`, `?tid=…,depth=0` | `201`; a copy of the folder and its two items in `F` | `_created_categories` [the copy], `_created_items` [two], `_moved_items` [], `_removed_items` [], `_top_category_id`, `_updated_category_versions` {`F` +1} |

AIS3 removes and empties **outside the Trash**, where the UDP messages refuse.
A slam with two links to one item is refused `400` "Duplicate links or invalid
link type."; a map `{ links: [...] }` instead of the bare array, `400` "List of
links required.".

### Gives

The no-copy give the task asked about — a notecard whose next owner may not
copy it, given by B to A and given back — was measured on OpenSim; on aditi
the first gift never arrived (below).

| step | Second Life | OpenSim |
| --- | --- | --- |
| A gives B a notecard (`give-inventory`'s shape), B accepts | filed in B's Notecards | filed in B's Notecards |
| B gives A a notecard, A accepts into its Notecards | the offer arrives; **nothing is filed and nothing pushed** in 30 s — the same for a full-permission control | filed in A's Notecards; nothing pushed to A |
| A's copy of the no-copy notecard | not received | owner may not copy |
| A gives it back: where A's copy is when the offer is made | not received | **gone** at once — OpenSim files B's copy at offer time |
| what A is told when B accepts | not received | `ImprovedInstantMessage` `InventoryAccepted` (UDP) |
| B holds it again | not received | yes |

The aditi failure is the primary test account's, not the census's:
`give-inventory` with its roles swapped (`--avatar secondary --secondary
primary`) fails the same way, while the same case as written (the primary
giving) passes. Why that account files no accepted gift is
`gridspec-inventory-offers`' to find out.
