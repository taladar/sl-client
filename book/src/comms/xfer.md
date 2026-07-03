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
  asset back over Xfer. This is the **one** Xfer consumer with a both-grids CAPS
  replacement — `NewFileAgentInventory` — so it (and only it) is being retired
  in favour of CAPS; see
  [Uploads and transport choice](#uploads-and-transport-choice).

Net: the Xfer transport itself **stays** (the mute list pins the download half,
terrain RAW pins the upload half). Only the legacy *asset* upload migrates to
CAPS, because it is the only rider with a modern path on both grids.

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
- a **generic** request surfaces the raw bytes as `Event::XferDownloaded`.

The generic path is the public building block: `Session::request_xfer(filename)`
starts a download and returns the `XferId` that tags its completion event, so a
caller handed a raw Xfer `filename` by some other message can fetch the bytes
directly.

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
`SendXferPacket`s (driven by the simulator's `ConfirmXferPacket`s). This is the
Xfer *upload* path's asset consumer.

The modern alternative is the CAPS `NewFileAgentInventory` uploader — a two-step
HTTP exchange (POST the metadata, then PUT the bytes to the returned uploader
URL), no Xfer involved. The runtimes' single `UploadAsset` command auto-selects:
it uses the CAPS uploader when the region advertises the capability, and falls
back to the UDP asset-upload plus a `CreateInventoryItem` otherwise — either way
surfacing the same `Event::AssetUploaded`.

Crucially, though, **both** Second Life and OpenSim advertise
`NewFileAgentInventory` (OpenSim registers it in its capability seed), and the
modern viewer uploads exclusively over it. So — unlike terrain RAW or the mute
list, which have no CAPS path on either grid — the legacy UDP asset upload is
the one Xfer rider with a both-grids modern replacement, and is therefore
slated for removal: the planned `asset-upload` conformance case is CAPS-only.

Retiring it will **not** remove the Xfer transport: the `RequestXfer` →
`SendXferPacket` machinery stays for terrain RAW (and any future bulk-file
upload), which have no CAPS equivalent on either grid. Only the asset-specific
layer (`AssetUploadRequest` and its bookkeeping) would go.

---

> **In this codebase**
>
> - The shared download registry is on the `Session` in
>   `sl-proto/src/session.rs` (`xfer_downloads`, keyed by `XferId`, each an
>   `XferDownload` carrying an `XferPurpose`). The single `SendXferPacket`
>   handler and the completion
>   routing are in `sl-proto/src/session/methods.rs`.
> - Low-level sends (`RequestXfer` / `ConfirmXferPacket` / `SendXferPacket`) are
>   in `sl-proto/src/session/circuit.rs`; `XferId` is in
>   `sl-proto/src/bookkeeping_ids.rs`.
> - Public API: `Session::request_xfer` (→ `Event::XferDownloaded`),
>   `Session::request_mute_list` (→ `Event::MuteList`), and
>   `Session::fetch_task_inventory` (→ `Event::TaskInventoryContents`). The
>   runtime commands are `Command::RequestXfer` / `Command::FetchTaskInventory`,
>   wired identically in `sl-client-tokio` and `sl-client-bevy`.
> - The task-inventory text parser is `parse_task_inventory` in
>   `sl-proto/src/session/conversions.rs` (alongside `parse_mute_list`),
>   producing `TaskInventoryItem` (`sl-proto/src/types/object.rs`). The
>   asset/inventory type-name maps it needs are `AssetType::from_type_name` /
>   `InventoryType::from_type_name` in `sl-proto/src/types/asset.rs`.
