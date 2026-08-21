# Asset Transfer (UDP)

The **Transfer** channel (`TransferRequest` → `TransferInfo` +
`TransferPacket` stream) is the old generic asset download path — the
UDP predecessor of the `ViewerAsset` capability. Unlike
[Xfer](xfer.md), which moves *named files*, Transfer moves *assets*
resolved by a **source**: the requester describes where the asset lives,
and the simulator streams the bytes back.

Most of it is retired. Plain asset-by-id downloads (`LLTST_ASSET`,
source 2) are superseded by the `ViewerAsset` HTTP capability on **both**
Second Life and OpenSim, so this client never speaks that source (and
its server side refuses it). Two sources, however, remain UDP-only on
both grids — neither has any HTTP capability — and they are why the
channel is still implemented:

- **`SimInvItem` (source 3): a task-inventory item's asset.** This is
  how a viewer reads a script or notecard body out of a prim's contents
  — the asset ids come from the [task-inventory
  listing](xfer.md#the-task-inventory-consumer), and the permission
  check is the simulator's. The reference viewer routes every
  script/notecard-in-prim read through this path unconditionally.
- **`SimEstate` (source 4): an estate asset**, in practice the estate
  **covenant** notecard (`ET_Covenant`). The `EstateCovenantReply` gives
  the covenant's asset id; the body itself is fetched here.

## The exchange

The requester mints a `TransferID` (a UUID), names the source type, and
packs a source-specific opaque `Params` blob. The serving side answers
with a `TransferInfo` header — status, declared size, and the params
echoed back — then streams `TransferPacket`s. There are no per-packet
acknowledgements (everything rides the reliable channel); the packet
whose status is `Done` (1) is the last, and packets may arrive out of
order.

```text
viewer                                        simulator
  │ TransferRequest { id, source, params } ──────▶
  │                                               │
  │ ◀── TransferInfo { id, status=Ok, size,       │  status ≠ Ok is the
  │                    params (echo) }            │  refusal: UnknownSource,
  │ ◀── TransferPacket { id, packet=0, Ok, data } │  InsufficientPermissions…
  │ ◀── TransferPacket { id, packet=1, Ok, data } │
  │                     …                         │
  │ ◀── TransferPacket { id, packet=n, Done, data }
```

The `Params` blobs are `LLDataPackerBinaryBuffer` layouts — raw 16-byte
UUIDs plus little-endian `S32`s:

- `SimInvItem` (100 bytes): `AgentID`, `SessionID`, `OwnerID`, `TaskID`,
  `ItemID`, `AssetID` at offsets 0/16/32/48/64/80, then the `S32`
  asset-type code at 96.
- `SimEstate` (36 bytes): `AgentID`, `SessionID`, then the `S32`
  estate-asset-type code (0 = covenant). No asset id — the simulator
  resolves the estate's current covenant itself.

A requester cancels with `TransferAbort`; a server refuses with a
non-`Ok` status in the `TransferInfo` (size 0), most commonly
`UnknownSource` (−2, the transfer 404) or `InsufficientPermissions`
(−3).

## Client and server views

The client mints monotonic transfer ids (a sans-I/O session has no
randomness; the id only correlates replies on the circuit), buffers
packets by index so out-of-order arrival still reassembles, and routes
the assembled bytes by the purpose it recorded at request time. A
non-`Ok` `TransferInfo` surfaces as a typed failure instead of silence.

The server side stashes the request params (to echo, as real simulators
do), surfaces a typed request event, and leaves the *decision* to the
driver: answer with the asset bytes, or refuse with a status. A request
for the legacy plain-asset source is auto-refused as `UnknownSource` per
the legacy-skip rule, but surfaced as a typed refusal (with the decoded
asset id and type) so a driver can log a client still trying the old
path; any other source type is refused silently.

---

> **In this codebase**
>
> - The params sub-codecs are `TransferSourceParamsInvItem` /
>   `TransferSourceParamsEstate` in `sl-wire/src/transfer.rs`, with the
>   channel/source/estate-asset constants and byte-exact layout tests.
> - Client: `Session::fetch_task_item_asset` (→
>   `Event::TaskItemAssetReceived`), `Session::fetch_estate_covenant_asset`
>   (→ `Event::EstateCovenantAssetReceived`), `Session::abort_transfer`;
>   failures surface as `Event::TransferFailed { status }`
>   (`TransferStatus`, `sl-proto/src/types/asset.rs`). The reassembly
>   machine is `TransferDownload` in `sl-proto/src/session.rs`; the
>   runtime commands are `Command::FetchTaskItemAsset` /
>   `Command::FetchEstateCovenantAsset`, wired identically in
>   `sl-client-tokio` and `sl-client-bevy` (REPL tokens
>   `fetch_task_item_asset`, `fetch_estate_covenant_asset`).
> - Server: `ServerEvent::TransferRequested { source:
>   TransferRequestSource, .. }`, `ServerEvent::TransferAborted`, and
>   `ServerEvent::LegacyAssetTransferRefused` (params decoded by
>   `sl_wire::TransferSourceParamsAsset`),
>   answered by `SimSession::send_transfer_asset` /
>   `SimSession::send_transfer_fail` (`sl-proto/src/sim_session.rs`);
>   loopback tests in `sl-proto/tests/sim_session.rs`.
> - `TransferId` is in `sl-proto/src/bookkeeping_ids.rs`.
