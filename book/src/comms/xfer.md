# Xfer File Transfer

**Xfer** is the oldest bulk-transfer mechanism in the SL/OpenSim protocol: a
reliable, chunked transfer of a *named file* over the [LLUDP
transport](lludp-transport.md). It is the fallback the protocol reaches for when
it must move a small, structured blob that has no asset UUID — a prim's
inventory listing, the agent's mute list, a region's terrain — and so does not
fit the asset-download path. Modern grids have moved most bulk data to
[CAPS](caps.md) HTTP, but Xfer is still how several features work today.

This chapter covers the **download** direction and the two consumers this
client parses (the mute list and the task-inventory listing), then the
**upload** direction and where asset uploads fit. The section below first
inventories *everything* that still rides Xfer and whether it has a CAPS
alternative — because that determines what can ever be retired.

## What still uses Xfer (and whether it could move to CAPS)

Xfer is legacy, but it is **not** retirable wholesale: a shared transport can
only be dropped once *every* feature riding it has a modern alternative that
works on **both** Second Life and OpenSim. Auditing the consumers (verified
against the Firestorm viewer and the OpenSim server sources):

- **Mute list — fetch** (download): Xfer-only, with **no capability on either
  grid**. The viewer source even notes it *"ideally should be turned into a
  capability"* — it never was. This alone keeps the Xfer *download* path alive.
- **Mute list — add / remove** (mutation): **not** Xfer at all — each change is
  a per-entry UDP message (`UpdateMuteListEntry` / `RemoveMuteListEntry`), also
  with no CAPS equivalent. The simulator regenerates the list file, which the
  viewer re-fetches over Xfer.
- **Prim (task) inventory listing** (download): Xfer on OpenSim. Second Life
  also offers a `RequestTaskInventory` **capability** (HTTP `?task_id=`), but
  **OpenSim has no such cap** — it serves the listing purely over UDP + Xfer. So
  this is a *dual-path* feature (CAPS on SL, UDP + Xfer on OpenSim), not
  CAPS-only.
- **Region terrain (RAW heightmap)** (upload *and* download):
  `EstateOwnerMessage "terrain"` plus an Xfer transfer, with **no capability**
  on either grid. This keeps the Xfer *upload* path alive.
- **Estate access / ban lists (bulk)**: can transfer both ways over Xfer;
  Second Life also has an `EstateAccess` cap, and the per-entry
  `EstateOwnerMessage` `estateaccessdelta` is the common UDP path.
- **Generic named file** (download): any message that hands the client a raw
  Xfer `filename` — fetched with `Session::request_xfer`.
- **Legacy asset upload** (upload): a large `AssetUploadRequest` streams the
  asset over Xfer. For **new-item** uploads this is retired — the
  `NewFileAgentInventory` capability exists on both grids and is the only
  path this client uses to mint items. But the **in-place save** of an
  existing wearable/body-part asset has **no capability on either grid**
  (the reference viewer's `saveNewAsset` cap code is commented out; OpenSim
  registers no wearable update cap), so that save still rides the UDP
  transaction upload — inline when small, pulled over Xfer when large. See
  [Uploads and transport choice](#uploads-and-transport-choice).

Net: the Xfer transport itself **stays** (the mute list pins the download half;
terrain RAW and the in-place wearable save pin the upload half). Only the
*new-item* asset upload has migrated to CAPS.

## The transfer

An Xfer moves one named file, correlated end to end by a 64-bit **xfer id**. To
download, the viewer sends a `RequestXfer` naming the file; the simulator
streams it back one `SendXferPacket` at a time, and the viewer acknowledges each
with a `ConfirmXferPacket`. The transfer is reliable and ordered — the next
chunk is only sent once the previous one is confirmed.

```text
viewer                                     simulator
  │  RequestXfer { id, filename }  ───────────▶
  │                                            │
  │  ◀─────  SendXferPacket { id, packet=0 }   │   packet 0's data is
  │          [ u32 LE length | file bytes… ]   │   prefixed with the total
  │  ConfirmXferPacket { id, packet=0 } ──────▶│   file length
  │                                            │
  │  ◀─────  SendXferPacket { id, packet=1 }   │
  │  ConfirmXferPacket { id, packet=1 } ──────▶│
  │                          …                 │
  │  ◀──  SendXferPacket { id, packet|0x80000000 }   the high bit of the
  │          [ final file bytes… ]             │     packet number marks EOF
  │  ConfirmXferPacket ───────────────────────▶│
```

Two wire details matter to a decoder:

- **The first packet is length-prefixed.** Packet 0's data begins with a 4-byte
  little-endian total file length before the file bytes; later packets are raw.
- **EOF is a flag, not a separate message.** The top bit (`0x80000000`) of the
  packet number marks the last packet; the low 31 bits are the sequence number.

## A shared download registry

Because every download is the same handshake — accumulate chunks, strip the
prefix on packet 0, confirm each packet, finish on the EOF bit — this client
drives them all through one registry rather than a bespoke path per consumer.
Each in-flight download is keyed by its xfer id and carries a *purpose* tag that
says what the assembled bytes should become. The single `SendXferPacket` handler
accumulates and confirms, and on the final packet routes the completed file by
its purpose:

- a **mute list** is parsed into `Event::MuteList`;
- a **task-inventory listing** is parsed into `Event::TaskInventoryContents`;
- a **generic** request surfaces the raw bytes as `Event::XferDownloaded`;
- a **server-initiated** offer (an `InitiateDownload`, today only the region
  terrain RAW) surfaces the raw bytes as `Event::ServerFileDownloaded`, tagged
  with the viewer filename it echoed back.

The generic path is the public building block: `Session::request_xfer(filename)`
starts a download and returns the `XferId` that tags its completion event, so a
caller handed a raw Xfer `filename` by some other message can fetch the bytes
directly.

## The server-initiated (terrain RAW) consumer

Some downloads are pushed the *other* way round: instead of the viewer naming a
file it already knows, the simulator hands the viewer a file it just produced.
The estate **terrain RAW download** is the one live example. The viewer sends an
`EstateOwnerMessage "terrain"` with `["download filename", <viewer name>]`
(`Session::request_region_terrain_download`); the simulator serialises the
region heightmap to an LL RAW file, stashes it under a random Xfer name, and
sends an `InitiateDownload` naming that server-side file and echoing back the
viewer name. The client follows the offer automatically — an Xfer download for
the named file, exactly as the reference viewer's `process_initiate_download` —
and surfaces the assembled bytes as `Event::ServerFileDownloaded`. This is
region-owner/god gated and has no capability on either grid, so it is the
`Xfer`-download path's non-mute-list, non-task-inventory consumer.

## The task-inventory consumer

A prim's [task inventory](../content/scripts.md#task-inventory) is the headline
example. `RequestTaskInventory` does **not** return the item list; its
`ReplyTaskInventory` returns only the contents `serial` and an Xfer `filename`.
The listing itself is downloaded over Xfer, and it is not LLSD — it is LL's
plain-text `inv_item { … }` format:

```text
	inv_item	0
	{
		item_id	<uuid>
		parent_id	<prim-uuid>
		permissions 0
		{
			base_mask	7fffffff
			…
			owner_id	<uuid>
			group_owned	0
		}
		asset_id	<uuid>          ← nil unless you may edit the prim
		type	lsltext
		inv_type	script
		flags	00000000
		sale_info	0 { sale_type not  sale_price 0 }
		name	Hello World|
		desc	|
		creation_date	1700000000
	}
```

`Command::FetchTaskInventory` ties the two steps together: it sends the request,
follows the reply to its Xfer file, downloads and parses it, and surfaces the
items as `Event::TaskInventoryContents` (the lower-level `TaskInventoryReply` is
still emitted first, for a caller that only wants the serial). Note the
simulator redacts `asset_id` to nil unless the requester may edit the prim's
inventory, so a parsed item's asset id is optional.

## Uploads and transport choice

Legacy asset **uploads** run over the same messages in the other direction: a
small asset is inlined in the `AssetUploadRequest`, while a large one is
answered with a `RequestXfer` and the client streams it back in
`SendXferPacket`s (driven by the simulator's `ConfirmXferPacket`s). The
simulator derives the stored asset id as `combine(transaction_id,
secure_session_id)` — the same combine the uploading client predicts — and
closes the exchange with an `AssetUploadComplete`.

Which upload path a client should take depends on what exists on **both**
grids:

- **New-item uploads** go over the CAPS `NewFileAgentInventory` uploader: a
  two-step HTTP exchange (POST the metadata, then POST the bytes to the
  returned uploader URL), no Xfer involved. Both grids advertise it, so the
  runtimes' `UploadAsset` command is CAPS-only — there is no UDP fallback for
  minting a new item, and the `asset-upload` conformance case is CAPS-only.
- **In-place saves of an existing item's asset** use the *update* caps where
  they exist (`UpdateNotecardAgentInventory`, `UpdateScriptAgent`,
  `UpdateGestureAgentInventory`, …, on both grids). But plain
  **wearables/body parts have no update capability on either grid** — the
  reference viewer's wearable save still calls the legacy `storeAssetData`
  path — so `Session::save_inventory_asset` speaks the UDP transaction upload
  above: `AssetUploadRequest` (+ the `UpdateInventoryItem` binding the
  transaction to the item), the simulator's Xfer pull for an oversized asset,
  and `Event::InventoryAssetSaved` on the `AssetUploadComplete`.

So the Xfer upload machinery stays for terrain RAW and the in-place wearable
save — the two riders with no CAPS equivalent on either grid — while
everything with a both-grids modern path uses it.

## The server side

The simulator half of every exchange above lives in the sans-I/O
`SimSession`, proven by in-memory `Session` ↔ `SimSession` loopback tests:

- **File serving** is a registry: `register_xfer_file(filename, bytes)` makes
  a file downloadable; the client's `RequestXfer` consumes the entry (the
  requests ask for delete-on-completion) and starts the one-packet-in-flight
  `SendXferPacket` stream, each next chunk released by the client's
  `ConfirmXferPacket`. An unregistered name is refused with an `AbortXfer`
  so the requester is not left hanging.
- **Task-inventory serving** composes the pieces: `serve_task_inventory`
  writes the contents listing with the mirror *writer* of the client's
  parser (`build_task_inventory`, byte-compatible with OpenSim's
  `RequestInventoryFile` text format), registers it under a deterministic
  `inventory_<task>.tmp` name, and sends the `ReplyTaskInventory` — the
  full server half of `Command::FetchTaskInventory`.
- **Terrain RAW** is the pair above composed: the client's
  `EstateOwnerMessage`/`terrain` requests surface as
  `ServerEvent::TerrainDownloadRequested` / `TerrainUploadRequested`; the
  driver answers a download with `send_initiate_download(sim_name,
  viewer_name, bytes)` (registers the file and sends the `InitiateDownload`
  the client auto-follows) and an upload with
  `request_xfer_upload(viewer_name)` — a named `RequestXfer` pull whose
  reassembled bytes arrive as `ServerEvent::XferReceived`. Every other
  estate method surfaces raw as `ServerEvent::EstateOwnerRequest`.
- **Upload receive** mirrors the wearable in-place save: an inline
  `AssetUploadRequest` completes immediately; an oversized one makes the
  sim issue the `RequestXfer` keyed by the predicted `VFileID`
  (`combine(transaction, secure_session)` — give the sim the secure session
  id via `set_secure_session_id`), reassemble the client's packet stream,
  and reply `AssetUploadComplete`. The assembled bytes surface as
  `ServerEvent::AssetUploaded`.

The sequencing rules are exactly the client's, mirrored: seq-0 length
prefix, high-bit EOF marker, strictly one packet in flight, `AbortXfer`
honoured in both directions. Both halves frame their packets through the
one sans-I/O codec in `sl-wire` (`XferPacketId`, `next_xfer_chunk`,
`decode_xfer_chunk`), so neither side masks the EOF bit or writes the
length prefix by hand. Aborting or pacing an xfer id that is not in
flight is an `Error::UnknownXfer`, not a silent no-op.

---

> **In this codebase**
>
> - The shared download registry is on the `Session` in
>   `sl-proto/src/session.rs` (`xfer_downloads`, keyed by `XferId`, each an
>   `XferDownload` carrying an `XferPurpose`). The single `SendXferPacket`
>   handler and the completion
>   routing are in `sl-proto/src/session/methods.rs`.
> - Low-level sends (`RequestXfer` / `ConfirmXferPacket` / `SendXferPacket` /
>   `AssetUploadRequest`) are in `sl-proto/src/session/circuit.rs`; `XferId`
>   is in `sl-proto/src/bookkeeping_ids.rs`.
> - The packet framing (`XFER_CHUNK_SIZE`, `XFER_EOF_FLAG`, `XferPacketId`,
>   `encode_xfer_chunk` / `decode_xfer_chunk` / `next_xfer_chunk`) is the
>   byte-pinned codec in `sl-wire/src/xfer.rs`, shared by the client's
>   download handler and upload sender and the server's send and receive.
> - Public API: `Session::request_xfer` (→ `Event::XferDownloaded`),
>   `Session::request_mute_list` (→ `Event::MuteList`),
>   `Session::fetch_task_inventory` (→ `Event::TaskInventoryContents`),
>   `Session::request_region_terrain_download` (→ `Event::ServerFileDownloaded`,
>   after the simulator's `InitiateDownload` the handler follows automatically),
>   `Session::request_region_terrain_upload` (→ `Event::XferUploaded`), and
>   `Session::save_inventory_asset` (→ `Event::InventoryAssetSaved`).
>   The runtime commands are `Command::RequestXfer` /
>   `Command::FetchTaskInventory` / `Command::RequestRegionTerrainDownload`,
>   wired identically in `sl-client-tokio` and `sl-client-bevy`.
> - The task-inventory text parser is `parse_task_inventory` in
>   `sl-proto/src/session/conversions.rs` (alongside `parse_mute_list`),
>   producing `TaskInventoryItem` (`sl-proto/src/types/object.rs`); its mirror
>   writer is `build_task_inventory` next to it. The asset/inventory type-name
>   maps they share are `AssetType::from_type_name`/`to_type_name` and
>   `InventoryType::from_type_name`/`to_type_name` in
>   `sl-proto/src/types/asset.rs`.
> - Server side: `SimSession::{register_xfer_file, serve_task_inventory,
>   send_initiate_download, request_xfer_upload, abort_xfer,
>   set_secure_session_id}` and the
>   `ServerEvent::{XferRequested, XferServed, XferReceived, XferAborted,
>   AssetUploadRequested, AssetUploaded, TerrainDownloadRequested,
>   TerrainUploadRequested, TerrainBakeRequested, EstateOwnerRequest}`
>   events in
>   `sl-proto/src/sim_session.rs`; loopback tests in
>   `sl-proto/tests/sim_session.rs`.
