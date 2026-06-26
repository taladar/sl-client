# Missing message-coverage roadmap

## Context

The live aditi smoke test (2026-06-25) surfaced inbound LLUDP messages the
client receives but does not handle — they fell through to
`Diagnostic::UnhandledMessage` and were logged as `WARN`, dropping useful data
(`SimStats`, `SimulatorViewerTimeMessage`; issue 2 in
[`KNOWN_ISSUES_ADITI.md`](KNOWN_ISSUES_ADITI.md)). Investigating that revealed a
broader gap.

**Goal:** complete, non-outdated, *bidirectional* LLUDP message coverage for the
client `Session`:

- **Inbound** (server→client): decode and surface every non-outdated message as
  a typed [`Event`], so no data is silently dropped and no spurious `WARN` is
  logged.
- **Outbound** (client→server): expose a `send_*` method (+ `Command` + REPL
  registry entry) for every non-outdated message the client should be able to
  send.
- **CAPS EventQueue** (server→client push — a *second* inbound surface):
  decode and surface every non-outdated event the simulator pushes over
  the `EventQueueGet` long-poll, so these too become typed `Event`s instead of
  `UnknownCapsEvent` warnings (where issue 3's `AgentStateUpdate` lives).

**Explicitly out of scope** (skipped, with rationale below): messages a client
never receives on its agent circuit (sim↔sim trust, circuit/transport
handshakes already handled in `session/circuit.rs`) and clearly deprecated
subsystems (legacy UDP inventory/asset/Xfer superseded by AIS3 / HTTP CAPS, the
NameValuePair system).

This is SL-only territory: the local OpenSim grid never sends most of these, so
the gap only shows up against a real Linden Lab simulator.

## Audit method (reproducible)

All counts below come from these commands (run from the repo root unless noted):

- **Inbound universe** (messages a client receives) — the Firestorm viewer's
  handler registrations, from `~/devel/3rdparty/phoenix-firestorm`:

  ```sh
  grep -rhoE 'setHandlerFunc(Fast)?\(\s*(_PREHASH_)?"?[A-Za-z0-9]+' \
    indra/newview indra/llmessage |
    sed -E 's/.*setHandlerFunc(Fast)?\(\s*(_PREHASH_)?"?//; s/".*//' | sort -u
  ```

- **Outbound universe** (messages a client sends) — the viewer's `newMessage`
  call sites (same trees): replace `setHandlerFunc` with `newMessage` above.

- **Inbound already handled** — match arms in the client dispatch:

  ```sh
  grep -oE 'AnyMessage::[A-Za-z0-9]+[^=]*=>' sl-proto/src/session/methods.rs |
    grep -oE 'AnyMessage::[A-Za-z0-9]+' | sed 's/AnyMessage:://' | sort -u
  ```

- The **gap** is the inbound universe minus the handled set, filtered to
  messages present in the current `sl-wire/message_template.msg`.

Transport/circuit messages (ping, open/close circuit) are handled in
`sl-proto/src/session/circuit.rs`, not in `dispatch`, so they never reach the
`UnhandledMessage` arm and are out of scope.

## Established implementation pattern

Every template message is already auto-decoded into an `AnyMessage` variant by
the `sl-wire` build-time codegen — no decoder work is needed. Per message:

1. **Domain struct** in `sl-proto/src/types/<topic>.rs`, mirroring
   `RegionIdentity` (`sl-proto/src/types/region.rs`); re-export it from
   `sl-proto/src/types/mod.rs`.
2. **Event variant** in `sl-proto/src/types/event.rs` (import the struct in the
   `use super::{…}` block), with a doc comment in the house style.
3. **Dispatch arm** in `Session::dispatch` (`sl-proto/src/session/methods.rs`),
   following `AnyMessage::CoarseLocationUpdate` (~line 2342): destructure the
   blocks, map to domain types (`AgentKey::from`, scalar conversions, …), and
   `self.events.push_back(Event::…)`.
4. **Formatter arm** — add the variant to the exhaustive `event_name` match in
   `sl-repl/src/format.rs` (no `_` arm, so it fails to compile until named).
5. **Tests** — a dispatch test in `sl-proto` (feed the `AnyMessage`, assert the
   surfaced `Event`) and the `event_name` is covered by the exhaustive match.

For **outbound** messages, mirror `Session::send_instant_message`
(`session/methods.rs` ~line 3232) → `circuit.send(&AnyMessage::…, reliability)`,
add a `Command` variant, and wire a REPL token in `sl-repl/src/registry.rs`.

## Inbound gap — 39 messages

`HANDLE` = implement (batched below). `SKIP` = out of scope, rationale in the
skip list.

| Message | Id | Disposition |
| --- | --- | --- |
| SimStats | Low 140 | HANDLE — batch 1 |
| SimulatorViewerTimeMessage | Low 150 | HANDLE — batch 1 |
| GenericMessage | Low 261 | HANDLE — batch 2 |
| LargeGenericMessage | Low 430 | HANDLE — batch 2 |
| GenericStreamingMessage | High 31 | HANDLE — batch 2 |
| Error | Low 423 | HANDLE — batch 3 |
| FeatureDisabled | Low 19 | HANDLE — batch 3 |
| KickUser | Low 163 | HANDLE — batch 3 |
| ObjectAnimation | High 30 | HANDLE — batch 4 |
| RebakeAvatarTextures | Low 87 | HANDLE — batch 4 |
| TerminateFriendship | Low 300 | HANDLE — batch 5 |
| OfferCallingCard | Low 301 | HANDLE — batch 5 |
| AcceptCallingCard | Low 302 | HANDLE — batch 5 |
| DeclineCallingCard | Low 303 | HANDLE — batch 5 |
| RemoveInventoryItem | Low 270 | HANDLE — batch 6 |
| RemoveInventoryFolder | Low 276 | HANDLE — batch 6 |
| RemoveInventoryObjects | Low 284 | HANDLE — batch 6 |
| MoveInventoryItem | Low 268 | HANDLE — batch 6 |
| ReplyTaskInventory | Low 290 | HANDLE — batch 6 |
| UserInfoReply | Low 400 | HANDLE — batch 6 |
| DeRezAck | Low 292 | HANDLE — batch 6 |
| ForceObjectSelect | Low 205 | HANDLE — batch 6 |
| GrantGodlikePowers | Low 258 | HANDLE — batch 6 |
| CompletePingCheck | High 2 | SKIP — transport |
| OpenCircuit | Fixed | SKIP — transport |
| CloseCircuit | Fixed | SKIP — transport |
| UseCircuitCode | Low 3 | SKIP — transport (client-sent) |
| AddCircuitCode | Low 2 | SKIP — sim↔sim trust |
| CreateTrustedCircuit | Low 392 | SKIP — sim↔sim trust |
| DenyTrustedCircuit | Low 393 | SKIP — sim↔sim trust |
| FetchInventoryReply | Low 280 | SKIP — deprecated (AIS3) |
| SaveAssetIntoInventory | Low 272 | SKIP — deprecated (HTTP) |
| InitiateDownload | Low 403 | SKIP — deprecated (Xfer) |
| DerezContainer | Low 104 | SKIP — deprecated |
| TransferRequest | Low 153 | SKIP — deprecated (HTTP CAPS) |
| TransferAbort | Low 155 | SKIP — deprecated (Xfer) |
| AbortXfer | Low 157 | SKIP — deprecated (Xfer) |
| NameValuePair | Low 329 | SKIP — deprecated (NVP) |
| RemoveNameValuePair | Low 330 | SKIP — deprecated (NVP) |

### Skip rationale

- **Transport / circuit:** `CompletePingCheck`, `OpenCircuit`, `CloseCircuit`,
  `UseCircuitCode` are link-layer concerns handled (or sent) in
  `session/circuit.rs`; they never reach `dispatch`.
- **Sim↔sim trust:** `AddCircuitCode`, `CreateTrustedCircuit`,
  `DenyTrustedCircuit` are exchanged between simulators / services, not
  delivered to a viewer's agent circuit.
- **Deprecated subsystems:** legacy UDP inventory fetch (`FetchInventoryReply`),
  UDP asset save/download/Xfer (`SaveAssetIntoInventory`, `InitiateDownload`,
  `TransferRequest`, `TransferAbort`, `AbortXfer`, `DerezContainer`) are
  superseded by the AIS3 inventory CAPS and HTTP asset CAPS this client already
  uses; the `NameValuePair` pair is the obsolete NVP mechanism. Revisit only if
  a concrete need appears.

## Inbound batches

Each batch is a separate commit covering domain structs, Event variants,
dispatch arms, `event_name` arms, and tests, then `cargo test`/`clippy`/`fmt`.

### Batch 1 — region telemetry (closes issue 2)

- **`SimStats` (Low 140)** → `Event::SimStats(Box<RegionStats>)`:
  `RegionStats { grid_coordinates: GridCoordinates, region_flags: u32,
  object_capacity: u32, region_flags_extended: u64, stats: Vec<(SimStatId,
  f32)> }`, where `SimStatId` is an enum over the known stat ids with an
  `Unknown(u32)` fallback. (Implemented as `GridCoordinates`, not the
  originally-sketched `RegionCoordinates`: the `RegionX` / `RegionY` fields
  carry the region's map-tile indices, not a region-local position — confirmed
  against OpenSim `RegionInfo.RegionLocX = WorldLocX / RegionSize`.) Stat-id
  meanings
  (TimeDilation=0, SimFPS=1, PhysicsFPS=2, Agents=13, ActiveScripts=15, …) are
  enumerated in `~/devel/3rdparty/opensim/OpenSim/Framework/SimStats.cs`
  (`StatsID` enum) and the Firestorm `LLViewerStats` sim-stat ids.
- **`SimulatorViewerTimeMessage` (Low 150)** → `Event::SimulatorTime(Box<…>)`
  with `usec_since_start: u64, sec_per_day: u32, sec_per_year: u32,
  sun_direction: Vector, sun_phase: f32, sun_ang_velocity: Vector`.

### Batch 2 — generic message family

`GenericMessage` (Low 261), `LargeGenericMessage` (Low 430),
`GenericStreamingMessage` (High 31): a method-name + params envelope used by
many features. Surfaced as `Event::GenericMessage(GenericMessage)` /
`Event::LargeGenericMessage(GenericMessage)` (the large variant shares the
`GenericMessage { method: String, invoice: InvoiceId, params: Vec<Vec<u8>> }`
domain struct — identical shape, larger per-param wire limit) and
`Event::GenericStreamingMessage(GenericStreamingMessage { method: u16, data:
Vec<u8> })`, leaving feature-specific parsing to consumers. The `Invoice`
correlation id is the new `InvoiceId` newtype (in `bookkeeping_ids.rs`);
parameter blobs stay raw bytes (lossless — they are usually but not always
UTF-8 strings). The feature-specific `emptymutelist` `GenericMessage` and the
GLTF-material-override `GenericStreamingMessage` (method `0x4175`) keep their
existing dedicated arms, matched ahead of the generic fallback.

### Batch 3 — session errors & forced disconnect

`Error` (Low 423), `FeatureDisabled` (Low 19): surface as typed error events.
`KickUser` (Low 163): surface as a kick event and drive the session toward
`Event::Disconnected`/`LoggedOut`.

Implemented in `types/server_error.rs` as `Event::ServerError(Box<ServerError>)`
(HTTP-like `code`, originating `system` path, human-readable `message`, plus the
deliberately-polymorphic `id` correlation field kept as a raw `Uuid` and the
binary LLSD `data` blob kept verbatim), `Event::FeatureDisabled(FeatureDisabled
{ message, agent: AgentKey, transaction: TransactionId })`, and
`Event::Kicked(Kick { agent: AgentKey, reason: String })`. The `KickUser` arm
also calls `self.close(DisconnectReason::Kicked { message })` — a new
`DisconnectReason` variant — so the session reaches its terminal
`Event::Disconnected` state; the `KickUser` routing fields (target sim address,
echoed session id) carry nothing the client needs and are dropped.

### Batch 4 — scene & appearance

`ObjectAnimation` (High 30): per-object animation start/stop (animesh).
`RebakeAvatarTextures` (Low 87): server request to rebake and re-upload
appearance.

Implemented as `Event::ObjectAnimation { object_id: ObjectKey, animations:
Vec<ObjectPlayingAnimation> }` (the object analogue of `Event::AvatarAnimation`;
the simulator sends the full authoritative set of animations signalled on an
animesh object's control avatar, not a delta). `ObjectPlayingAnimation {
anim_id: AnimationKey, sequence_id: i32 }` lives in `types/object.rs`; it omits
the `source_id` of `PlayingAnimation` because an animesh object is its own
animation source. `RebakeAvatarTextures` surfaces as
`Event::RebakeAvatarTextures {
texture_id: TextureKey }` — the baked texture the simulator could not find and
wants re-uploaded.

### Batch 5 — friendship & calling cards

`TerminateFriendship` (Low 300), `OfferCallingCard` (Low 301),
`AcceptCallingCard` (Low 302), `DeclineCallingCard` (Low 303).

Implemented as four inline `Event` variants (all payloads are key +
transaction-id newtype combos, so no dedicated domain struct was warranted):
`Event::FriendshipTerminated { other: FriendKey }` (the `AgentData` echo of this
agent's own id is dropped — only `ExBlock.OtherID`, the former friend, matters);
`Event::CallingCardOffered { offering_agent: AgentKey, transaction:
TransactionId }` (`AgentBlock.DestID`, this agent itself, is dropped — note a
calling card is a reference card to an avatar filed in the Calling Cards folder,
*not* a friendship request); `Event::CallingCardAccepted { agent: AgentKey,
transaction: TransactionId }` (the accepter's `FolderData` destination folder is
their inventory, not this agent's, so it is dropped); and
`Event::CallingCardDeclined { agent: AgentKey, transaction: TransactionId }`.
The `transaction` is the existing `TransactionId` newtype, correlating an
accept/decline back to the original offer.

### Batch 6 — inventory sync, task inventory & misc

Server-initiated inventory mutations to keep a client mirror current:
`RemoveInventoryItem` (270), `RemoveInventoryFolder` (276),
`RemoveInventoryObjects` (284), `MoveInventoryItem` (268). Plus
`ReplyTaskInventory` (290, object contents), `UserInfoReply` (400, email/IM
prefs), `DeRezAck` (292), `ForceObjectSelect` (205), `GrantGodlikePowers` (258).

Implemented as nine `Event` variants (the echoed `AgentData.AgentID` is dropped
on every one — it is just this agent's own id):

- `RemoveInventoryItem` → `Event::InventoryItemsRemoved { items:
  Vec<InventoryKey> }`; `RemoveInventoryFolder` →
  `Event::InventoryFoldersRemoved { folders: Vec<InventoryFolderKey> }`;
  `RemoveInventoryObjects` → `Event::InventoryObjectsRemoved { folders, items }`
  (mixed folders + items in one message).
- `MoveInventoryItem` → `Event::InventoryItemsMoved { stamp: bool, moves:
  Vec<InventoryItemMove> }`, where `InventoryItemMove { item: InventoryKey,
  folder: InventoryFolderKey, new_name: Option<String> }` (in
  `types/inventory.rs`; an empty wire `NewName` maps to `None`) and `stamp`
  echoes the re-timestamp flag.
- `ReplyTaskInventory` → `Event::TaskInventoryReply(TaskInventoryReply { task:
  ObjectKey, serial: i16, filename: String })` (in `types/object.rs`); the
  filename is the temporary Xfer file the full contents listing is downloaded
  from.
- `UserInfoReply` → `Event::UserInfo(UserInfo { im_via_email: bool,
  directory_visibility: String, email: String })` (in
  `types/avatar_profile.rs`).
- `DeRezAck` → `Event::DeRezAck { transaction: TransactionId, success: bool }`.
- `ForceObjectSelect` → `Event::ForceObjectSelect { reset_list: bool, objects:
  Vec<ScopedObjectId> }`; the region-local ids are scoped to the originating
  circuit (skipped if the circuit is unknown).
- `GrantGodlikePowers` → `Event::GodlikePowersGranted { god_level: u8 }`; the
  wire `Token` is checked on the sim and ignored by the viewer, so it is
  dropped.

## CAPS EventQueue gap

A second inbound surface: events the simulator pushes over the `EventQueueGet`
long-poll capability (not LLUDP, not HTTP cap GET/POST replies). They are
dispatched by name in `Session::handle_caps_event`
(`sl-proto/src/session/methods.rs` ~line 263), with an `UnknownCapsEvent`
diagnostic fallback — the same shape of gap as the inbound UDP one. Surfaced by
issue 3 in `KNOWN_ISSUES_ADITI.md` (`AgentStateUpdate` logged unhandled on
aditi).

**Audit method:** the viewer's EventQueue dispatch (Firestorm
`indra/newview/lleventpoll.cpp` → `LLMessageSystem::dispatch(msg_name, …)`, plus
the per-event `LLHTTPNode` registrations) and OpenSim's senders
(`OpenSim/Region/ClientStack/Linden/Caps/EventQueue/EventQueueGetHandlers.cs`)
give the universe; our handled set is the string-literal arms in
`handle_caps_event`.

**Already handled (push):** `EnableSimulator`, `EstablishAgentCommunication`,
`TeleportFinish`, `CrossedRegion`, `ParcelProperties`, `AgentGroupDataUpdate`,
`BulkUpdateInventory`, `ObjectPhysicsProperties`, `UpdateAvatarAppearance`,
`ExtEnvironment`, `ChatterBoxInvitation` (+ the HTTP cap GET/POST replies routed
through the same match).

**Implementation pattern:** add a match arm to `handle_caps_event`, parse the
LLSD `body` with a `*_from_llsd` helper (mirror
`chatterbox_invitation_from_llsd` / the parsers in
`sl-proto/src/session/conversions.rs`), and push a typed
`Event` — then the same Event variant + `event_name` arm + tests as the UDP
pattern.

### Unhandled EventQueue events

| Event | Disposition |
| --- | --- |
| `AgentStateUpdate` | HANDLE — EQ batch 1 (closes issue 3) |
| `NavMeshStatusUpdate` | HANDLE — EQ batch 1 |
| `AgentDropGroup` | HANDLE — EQ batch 2 |
| `DisplayNameUpdate` | HANDLE — EQ batch 2 |
| `SetDisplayNameReply` | HANDLE — EQ batch 2 |
| `WindLightRefresh` | HANDLE — EQ batch 3 |
| `SimConsoleResponse` | HANDLE — EQ batch 3 |
| `RequiredVoiceVersion` | HANDLE — EQ batch 3 |
| `OpenRegionInfo` | HANDLE — EQ batch 3 (OpenSim-specific) |
| `ChatterBoxSessionAgentListUpdates` | DEFER — `CHAT_ROADMAP.md` |
| `ChatterBoxSessionStartReply` | DEFER — `CHAT_ROADMAP.md` |
| `ChatterBoxSessionUpdate` | DEFER — `CHAT_ROADMAP.md` |
| `ChatterBoxSessionEventReply` | DEFER — `CHAT_ROADMAP.md` |
| `ForceCloseChatterBoxSession` | DEFER — `CHAT_ROADMAP.md` |

The `ChatterBox*` session-lifecycle events are the stateful group/conference
chat machinery already designed in `CHAT_ROADMAP.md` (which owns
`ChatterBoxInvitation` → `Event::ConferenceInvited`); they are handled there,
not here, to avoid a half-built chat surface.

### EventQueue batches

- **EQ batch 1 — pathfinding agent state (closes issue 3).**
  `AgentStateUpdate` (body `{ "can_modify_navmesh": bool }` — whether the agent
  may rebake this region's navmesh; Firestorm `llpathfindingmanager.cpp`) and
  `NavMeshStatusUpdate` (navmesh dirty/baking status). SL-only — OpenSim emits
  neither, so this only ever shows up against a real grid.

  Implemented as `Event::AgentStateUpdate { can_modify_navmesh: bool }` (an
  inline variant — a single flag warrants no domain struct) and
  `Event::NavMeshStatus(NavMeshStatus)`, where `NavMeshStatus { region_id:
  Uuid, version: u32, status: NavMeshBuildStatus }` lives in
  `types/pathfinding.rs`. `NavMeshBuildStatus` is an enum over the four wire
  tokens (`Pending`/`Building`/`Complete`/`Repending`) with a `from_wire`
  parser that maps any unrecognised or missing value to `Complete`, mirroring
  the reference viewer's `LLPathfindingNavMeshStatus`. `region_id` stays a raw
  `Uuid` — this crate has no dedicated region-key newtype and represents region
  ids as `Uuid` everywhere (see `RegionIdentity`, `EnvironmentSettings`).
- **EQ batch 2 — group & display-name pushes.** `AgentDropGroup` (the sim
  dropped the agent from a group), `DisplayNameUpdate` (a cached display name
  changed), `SetDisplayNameReply` (result of the agent's own set-display-name).

  Implemented as `Event::AgentDroppedFromGroup { group: GroupKey }` (an inline
  variant — the echoed `AgentID` is this agent itself and is dropped, leaving
  only the `GroupID` the sim removed the agent from), and two boxed variants
  carrying domain structs in the new `types/display_name.rs`:
  `Event::DisplayNameUpdate(Box<DisplayNameUpdate>)` where `DisplayNameUpdate {
  old_display_name: String, name: DisplayName }` reuses the existing
  `sl_wire::DisplayName` record (the push's `agent` block is
  `LLAvatarName::asLLSD`, the same People API fields as a `GetDisplayNames`
  entry but with no embedded `id` — so the id is taken from the body's top-level
  `agent_id`); and `Event::SetDisplayNameReply(Box<SetDisplayNameReply>)` where
  `SetDisplayNameReply { status: i32, reason: String, new_display_name:
  Option<String>, error_tag: Option<String> }` extracts the meaningful fields of
  the polymorphic `content` blob (the new name on success, the error tag on
  failure) and exposes a `succeeded()` helper (`status == 200`). All three are
  SL-only — OpenSim never pushes them. Decoded by `agent_drop_group_from_llsd` /
  `display_name_update_from_llsd` / `set_display_name_reply_from_llsd` in
  `session/conversions.rs` and dispatched by name in
  `Session::handle_caps_event`.
- **EQ batch 3 — region/environment/voice misc.** `WindLightRefresh` (re-fetch
  environment), `SimConsoleResponse` (reply to a region debug-console command),
  `RequiredVoiceVersion` (voice protocol version), `OpenRegionInfo` (OpenSim
  extended region settings).

  Implemented as two inline `Event` variants and two struct-carrying ones:
  `Event::WindLightRefresh { interpolate: bool }` (the body's single
  `Interpolate` int flag — the sim asks the client to re-fetch the region
  environment, interpolating the transition when set) and
  `Event::SimConsoleResponse { output: String }` (the body is a *bare* LLSD
  string — the console command's raw output — not a map);
  `Event::RequiredVoiceVersion(RequiredVoiceVersion)` where
  `RequiredVoiceVersion { major_version: i32, region_name: String,
  voice_server_type: Option<String> }` lives in the new `types/voice.rs` (the
  voice backend is `None` on older grids, which the reference viewer treats as
  the `"vivox"` default); and
  `Event::OpenRegionInfo(Box<OpenRegionInfo>)` where `OpenRegionInfo` (new
  `types/open_region.rs`) is a 27-field all-`Option` bag of OpenSim per-region
  limits/overrides — only the keys the sim sends are present, matching the
  reference viewer's independent `has()` checks. The `Max`/`Min` position bounds
  group their `*PosX`/`*PosY`/`*PosZ` keys into a `RegionCoordinates` (present
  only when all three components are); other fields stay primitive (no domain
  newtype fits these OpenSim-specific limits).
  `WindLightRefresh`/`SimConsoleResponse` are OpenSim-emitted; `OpenRegionInfo`
  is OpenSim-only; `RequiredVoiceVersion` is SL/grid-specific. Decoded by
  `windlight_refresh_from_llsd` / `sim_console_response_from_llsd` /
  `required_voice_version_from_llsd` / `open_region_info_from_llsd` in
  `session/conversions.rs` and dispatched by name in
  `Session::handle_caps_event`.

## Outbound gap — Phase 0 audit (complete)

The outbound gap could **not** be auto-computed by name alone at first: the
client builds outbound messages through dedicated `send_*` helpers (typed
structs → `circuit.send`), not bare `AnyMessage::…` literals. Phase 0
reconciled the two universes by scanning where the client *constructs* an
`AnyMessage` to send.

**Audit method (reproducible).** The outbound universe is the Firestorm
`newMessage`/`newMessageFast` call sites (the command in *Audit method* with
`setHandlerFunc` replaced by `newMessage`): 216 distinct real messages (after
dropping C identifier fragments the regex caught — `add`, `char`, `const`,
`info`, `message`, `msg`, `name`, `mMessageReader`, `LLMessageStringTable`).
The client's *sent* set is every `AnyMessage::…` variant the client
constructs (not match-arm dispatches) in `session/circuit.rs` and
`session/methods.rs`:

```sh
grep -rhnE 'AnyMessage::[A-Za-z0-9]+' \
  sl-proto/src/session/circuit.rs sl-proto/src/session/methods.rs |
  grep -vE '=>' | grep -oE 'AnyMessage::[A-Za-z0-9]+' |
  sed 's/AnyMessage:://' | sort -u
```

That yields **182** messages the client already sends. The raw gap is 65
names; **9** are the C-fragment false positives above and **4**
(`AckAddCircuitCode`, `EstateOwnerRequest`, `GetScriptExports`, `RedoLand`) are
absent from the current `sl-wire/message_template.msg`, so — per the same
template filter the inbound audit used — they fall out of scope. That leaves
**55** real, in-template outbound gap messages: **41 HANDLE**, **14 SKIP**.

### Outbound gap — 55 messages

`HANDLE` = implement (batched below). `SKIP` = out of scope, rationale in the
skip list. Direction was confirmed against the Firestorm send sites
(`indra/newview/*` = viewer-sent; `indra/llmessage/*` infrastructure was
checked individually).

| Message | Id | Disposition |
| --- | --- | --- |
| OfferCallingCard | Low 301 | HANDLE — out batch 1 |
| AcceptCallingCard | Low 302 | HANDLE — out batch 1 |
| DeclineCallingCard | Low 303 | HANDLE — out batch 1 |
| ObjectExtraParams | Low 99 | HANDLE — out batch 2 |
| ObjectImage | Low 96 | HANDLE — out batch 2 |
| ObjectShape | Low 98 | HANDLE — out batch 2 |
| RezObject | Low 293 | HANDLE — out batch 3 |
| RezScript | Low 304 | HANDLE — out batch 3 |
| RevokePermissions | Low 193 | HANDLE — out batch 3 |
| DetachAttachmentIntoInv | Low 397 | HANDLE — out batch 3 |
| RequestTaskInventory | Low 289 | HANDLE — out batch 4 |
| UpdateTaskInventory | Low 286 | HANDLE — out batch 4 |
| MoveTaskInventory | Low 288 | HANDLE — out batch 4 |
| RemoveTaskInventory | Low 287 | HANDLE — out batch 4 |
| ModifyLand | Low 124 | HANDLE — out batch 5 |
| UndoLand | Low 77 | HANDLE — out batch 5 |
| ParcelPropertiesRequestByID | Low 197 | HANDLE — out batch 5 |
| ParcelSetOtherCleanTime | Low 200 | HANDLE — out batch 5 |
| LinkInventoryItem | Low 426 | HANDLE — out batch 6 |
| GroupTitleUpdate | Low 377 | HANDLE — out batch 6 |
| UpdateGroupInfo | Low 341 | HANDLE — out batch 6 |
| TeleportCancel | Low 72 | HANDLE — out batch 7 |
| TeleportLandmarkRequest | Low 65 | HANDLE — out batch 7 |
| SetStartLocationRequest | Low 324 | HANDLE — out batch 7 |
| AgentDataUpdateRequest | Low 386 | HANDLE — out batch 7 |
| AgentQuitCopy | Low 85 | HANDLE — out batch 7 |
| VelocityInterpolateOn | Low 125 | HANDLE — out batch 7 |
| VelocityInterpolateOff | Low 126 | HANDLE — out batch 7 |
| UserInfoRequest | Low 399 | HANDLE — out batch 8 |
| UpdateUserInfo | Low 401 | HANDLE — out batch 8 |
| SoundTrigger | High 29 | HANDLE — out batch 8 |
| RequestGodlikePowers | Low 257 | HANDLE — out batch 9 |
| EjectUser | Low 167 | HANDLE — out batch 9 |
| FreezeUser | Low 168 | HANDLE — out batch 9 |
| GodUpdateRegionInfo | Low 143 | HANDLE — out batch 9 |
| SimWideDeletes | Low 129 | HANDLE — out batch 9 |
| ParcelGodForceOwner | Low 214 | HANDLE — out batch 10 |
| ParcelGodMarkAsContent | Low 227 | HANDLE — out batch 10 |
| EventGodDelete | Low 183 | HANDLE — out batch 10 |
| StateSave | Low 127 | HANDLE — out batch 10 |
| ViewerStartAuction | Low 228 | HANDLE — out batch 10 |
| StartPingCheck | High 1 | SKIP — transport |
| CreateTrustedCircuit | Low 392 | SKIP — sim↔sim trust |
| DenyTrustedCircuit | Low 393 | SKIP — sim↔sim trust |
| RequestTrustedCircuit | Low 394 | SKIP — sim↔sim trust |
| UUIDNameReply | Low 236 | SKIP — sim/service-sent |
| UUIDGroupNameReply | Low 238 | SKIP — sim/service-sent |
| Error | Low 423 | SKIP — symmetric error primitive |
| AbortXfer | Low 157 | SKIP — deprecated (Xfer) |
| TransferAbort | Low 155 | SKIP — deprecated (transfer/HTTP CAPS) |
| TransferInfo | Low 154 | SKIP — deprecated (transfer/HTTP CAPS) |
| TransferPacket | High 17 | SKIP — deprecated (transfer/HTTP CAPS) |
| AssetUploadComplete | Low 334 | SKIP — deprecated (HTTP asset) |
| FetchInventory | Low 279 | SKIP — deprecated (AIS3) |
| RemoveNameValuePair | Low 330 | SKIP — deprecated (NVP) |

### Outbound skip rationale

- **Transport:** `StartPingCheck` is the link-layer circuit ping, paired with
  `CompletePingCheck` in `session/circuit.rs`; it never goes through a feature
  `send_*` method.
- **Sim↔sim trust:** `CreateTrustedCircuit`, `DenyTrustedCircuit`,
  `RequestTrustedCircuit` negotiate the trusted inter-simulator circuit; a
  viewer's agent circuit never sends them (the Firestorm sites are in
  `indra/llmessage`, used by the sim/service build).
- **Sim/service-sent:** `UUIDNameReply` / `UUIDGroupNameReply` are *replies* to
  `UUIDNameRequest` / `UUIDGroupNameRequest` (both of which the client already
  sends); the Firestorm `newMessage` sites are the legacy `llcachename.cpp`
  peer-name-cache acting as a responder, not a viewer→sim send.
- **Symmetric error primitive:** `Error` is `LLMessageSystem::sendError`, an
  infrastructure error-report used by both ends. The viewer's role is to
  *receive* it — already surfaced inbound as `Event::ServerError` (batch 3);
  there is no client feature that needs to send one.
- **Deprecated subsystems:** `AbortXfer` and the `Transfer*` family
  (`TransferAbort`, `TransferInfo`, `TransferPacket`) plus `AssetUploadComplete`
  are the legacy UDP Xfer/asset-transfer mechanisms superseded by the HTTP asset
  CAPS; `FetchInventory` is legacy UDP inventory fetch superseded by AIS3;
  `RemoveNameValuePair` is the obsolete NVP system. Consistent with the inbound
  skip list — the client retains a few pre-existing legacy Xfer *requests*
  (`RequestXfer`, `SendXferPacket`, `ConfirmXferPacket`, `TransferRequest`,
  `AssetUploadRequest`), but extending these deprecated subsystems is out of
  scope. Revisit only if a concrete need appears.

### Outbound batches

Each batch is a separate commit covering any new domain structs, the `send_*`
method(s) on `Session` (mirroring `Session::send_instant_message`), the
`Command` variant(s), the REPL token(s) in `sl-repl/src/registry.rs`, and tests,
then `cargo fmt`/`clippy`/`test`. Several god/estate and object-editing messages
are partially OpenSim-testable (terraform, object edit) while others are SL- or
god-only; live-verify what the local grid supports and exercise the rest against
aditi.

- **Out batch 1 — calling cards.** `OfferCallingCard`,
  `AcceptCallingCard`, `DeclineCallingCard`: the viewer→sim counterparts of the
  inbound batch-5 events. Offer a calling card for an avatar; accept/decline an
  incoming offer, echoing its `TransactionId`.

  Implemented as `Session::offer_calling_card(to_agent_id: AgentKey,
  transaction_id: TransactionId)`, `Session::accept_calling_card(transaction_id:
  TransactionId, calling_card_folder: InventoryFolderKey)` and
  `Session::decline_calling_card(transaction_id: TransactionId)` (mirroring the
  existing `send_friendship_offer` / `accept_friendship` / `decline_friendship`
  trio — `AcceptCallingCard` carries the same `FolderData` destination-folder
  block as `AcceptFriendship`), backed by `send_offer_calling_card` /
  `send_accept_calling_card` / `send_decline_calling_card` on the circuit. Wired
  as `Command::{OfferCallingCard, AcceptCallingCard, DeclineCallingCard}`
  through the tokio runtime, the `command_name` formatter, and the
  `offer_calling_card` / `accept_calling_card` / `decline_calling_card` REPL
  tokens. SL-only round-trip (OpenSim does not surface calling-card offers);
  exercised by a pack-the-wire test asserting each message carries the right
  dest/transaction/folder ids.
- **Out batch 2 — object prim editing.** `ObjectShape` (prim geometry),
  `ObjectExtraParams` (sculpt/flexi/light/mesh extra params), `ObjectImage`
  (per-face textures / TE) — the edit-tool prim-update messages keyed by the
  region-local object id.

  Implemented as
  `Session::set_object_shape(local_id: ScopedObjectId, shape: &PrimShapeParams)`,
  `Session::set_object_image(local_id, media_url: Option<&str>, texture_entry: &TextureEntry)`,
  and `Session::set_object_extra_params(local_id, params: &ObjectExtraParams)`
  (mirroring the existing `set_object_*` edit methods, all
  `ScopedObjectId`-keyed via `circuit_for_scope`). Each reuses an existing
  domain struct rather than raw wire fields: `ObjectShape` carries the inbound
  `PrimShapeParams` (the same quantized path/profile values an `ObjectUpdate`
  decodes to); `ObjectImage` carries a `TextureEntry` packed with the existing
  `encode_texture_entry` (a new `TextureFace::new` builds a neutral face — one
  face retextures every face, since the wire run-length default applies to all);
  `ObjectExtraParams` carries the inbound `ObjectExtraParams` bag and is
  serialised by a new `extra_param_message_blocks` helper (factored out of
  `encode_extra_params`'s entry builder) that emits
  **one block per known subtype** with `ParamInUse` reflecting presence —
  mirroring the reference viewer's `sendExtraParameters`, so a subtype absent
  from `params` is *cleared* on the object and `ObjectExtraParams::default`
  clears them all. Wired as
  `Command::{SetObjectShape, SetObjectImage, SetObjectExtraParams}` through the
  tokio and bevy runtimes, the `command_name` formatter, and the
  `set_object_shape` / `set_object_image` / `set_object_extra_params` REPL
  tokens (the extra-params token covers the flexi/light/sculpt subtypes — the
  OpenSim-handled ones; the rarer light-image/extended-mesh/render-material/
  reflection-probe subtypes remain settable through the typed API). Covered by
  three pack-the-wire tests; object edit (shape/texture) is OpenSim-testable.
- **Out batch 3 — rez & script permissions.** `RezObject` / `RezScript` (rez an
  inventory object/script into the world), `RevokePermissions` (revoke
  previously-granted script permissions), `DetachAttachmentIntoInv` (detach a
  worn attachment back to inventory).

  Implemented as `Session::rez_object_from_inventory(params: &RezObjectParams)`,
  `Session::rez_script(target: ScopedObjectId, params: &RezScriptParams)`,
  `Session::revoke_script_permissions(object_id: ObjectKey, permissions: ScriptPermissions)`,
  and `Session::detach_attachment_into_inventory(item_id: InventoryKey)`. The
  `RezObject` wire message rezzes an inventory item into the world (distinct
  from the existing `Session::rez_object`, which builds a *new* prim via
  `ObjectAdd` — hence the `_from_inventory` suffix to avoid the collision). New
  domain structs `RezObjectParams` (ray placement + the per-object permission
  masks the rez applies) and `RezScriptParams` (running flag + active group)
  both carry the message's full inventory-item block as a reused `RestoreItem` —
  the same per-item payload `RezRestoreToWorld` takes — rather than 20 raw wire
  fields. `revoke_script_permissions` reuses the typed `ScriptPermissions`
  bitfield (the inverse of `answer_script_permissions`);
  `detach_attachment_into_inventory` keys off the worn item's `InventoryKey`,
  unlike `detach_objects` which needs the rezzed object's region-local id. Wired
  as
  `Command::{RezObjectFromInventory, RezScript, RevokeScriptPermissions, DetachAttachmentIntoInventory}`
  through the tokio and bevy runtimes, the `command_name` formatter, and the
  matching REPL tokens (`rez_object_from_inventory` / `rez_script` reuse a new
  `restore_item_from_args` helper, which also de-duplicates the
  `rez_restore_to_world` token's 20-field item builder). Covered by one
  pack-the-wire lifecycle test and four REPL parse tests. Live-testable on
  OpenSim (rez/detach/script-drop all work against the local grid).
- **Out batch 4 — task (object) inventory.** `RequestTaskInventory`,
  `UpdateTaskInventory`, `MoveTaskInventory`, `RemoveTaskInventory`: read and
  mutate the inventory contents of an in-world object (the outbound side of the
  inbound batch-6 `ReplyTaskInventory`).

  Implemented as `Session::request_task_inventory`,
  `Session::update_task_inventory` (taking a `TaskInventoryKey` and a
  `&RestoreItem`), `Session::move_task_inventory` (taking a destination
  `InventoryFolderKey` and the item's `InventoryKey`), and
  `Session::remove_task_inventory`. All four name the target prim by its
  [`ScopedObjectId`] (the region-local `LocalID` the wire blocks carry, scoped
  to its circuit) rather than a bare `u32`, matching the `rez_script` pattern;
  `request_task_inventory`'s reply arrives as the already-handled
  `Event::TaskInventoryReply`. `UpdateTaskInventory` reuses the same full-item
  [`RestoreItem`] payload as `RezScript`/`RezObject` (its `InventoryData` block
  is field-for-field identical) instead of 20 raw wire fields, and a new typed
  [`TaskInventoryKey`] enum (`Item`/`Asset`, LL's `TASK_INVENTORY_*_KEY`)
  replaces the raw `Key` byte. Wired as four new `Command` variants through the
  tokio and bevy runtimes, the `command_name` formatter, and the matching REPL
  tokens (`update_task_inventory` reuses the existing `restore_item_from_args`
  helper and a new `parse_task_inventory_key`). Covered by one pack-the-wire
  lifecycle test and four REPL parse tests. Live-testable on OpenSim (task
  inventory read/edit/move/remove all work against the local grid).
- **Out batch 5 — land & parcel.** `ModifyLand` (terraform), `UndoLand` (undo
  terraform; `RedoLand` is absent from the template),
  `ParcelPropertiesRequestByID` (fetch a parcel by local id),
  `ParcelSetOtherCleanTime` (parcel object
  auto-return time).

  Implemented as `Session::modify_land` (taking a typed [`LandEdit`]),
  `Session::undo_land`, `Session::request_parcel_properties_by_id`, and
  `Session::set_parcel_other_clean_time`. `ModifyLand` uses a new typed
  `land` module instead of raw wire fields: [`LandBrushAction`] (the `E_LAND_*`
  action enum — level/raise/lower/smooth/noise/revert), [`LandBrushSize`]
  (small/medium/large, carrying both the LL metre radius sent in
  `ModifyBlockExtended` and the deprecated legacy index byte), and
  [`TerraformArea`] (the region-local ground rectangle); the optional target
  parcel is a [`RegionLocalParcelId`] (`-1` when absent, as the viewer sends for
  free brushing). The two parcel ops key off a [`ScopedParcelId`] like the rest
  of the parcel API, and `set_parcel_other_clean_time` takes a
  [`std::time::Duration`] rounded down to whole minutes (the wire `S32`).
  `request_parcel_properties_by_id` fetches by local id where the existing
  `request_parcel_properties` fetches by metre rectangle; both surface the reply
  as `Event::ParcelProperties`. Wired as `Command::{ModifyLand, UndoLand,
  RequestParcelPropertiesById, SetParcelOtherCleanTime}` through the tokio and
  bevy runtimes, the `command_name` formatter, and the matching REPL tokens
  (`modify_land` / `undo_land` / `request_parcel_properties_by_id` /
  `set_parcel_other_clean_time`, with new `parse_land_brush_action` /
  `parse_land_brush_size` helpers). Covered by one pack-the-wire lifecycle test,
  three `land`-module unit tests, and four REPL parse tests. Live-testable on
  OpenSim (terraform/undo and parcel auto-return all work against the local
  grid).
- **Out batch 6 — inventory link & group info.** `LinkInventoryItem` (create an
  inventory link), `GroupTitleUpdate` (set the agent's active group title),
  `UpdateGroupInfo` (edit a group's charter/settings).

  Implemented as `Session::link_inventory_item(new: &NewInventoryLink)` →
  `InventoryCallbackId` (mirroring `create_inventory_item`; the reply arrives as
  the already-handled `Event::InventoryItemCreated`),
  `Session::update_group_info(params: &UpdateGroupInfoParams)`, and
  `Session::update_group_title(group_id: GroupKey, title_role_id: GroupRoleKey)`.
  `NewInventoryLink` (in `types/inventory.rs`) keys the link target by the
  polymorphic `InventoryItemOrFolderKey` (an item *or* folder link) and keeps
  the asset/inv type codes as raw `i8`, consistent with the sibling
  `NewInventoryItem`; the wire `TransactionID` is always nil for a link.
  `UpdateGroupInfoParams` (in `types/group.rs`) mirrors `CreateGroupParams` but
  targets an existing `GroupKey` and carries no name (a group cannot be
  renamed). `GroupTitleUpdate` needs no domain struct — it is a `GroupKey` +
  `GroupRoleKey` pair (the role carrying the desired title); the message's
  routing is otherwise just the echoed agent/session ids. Wired as
  `Command::{LinkInventoryItem, UpdateGroupInfo, UpdateGroupTitle}` through the
  tokio and bevy runtimes, the `command_name` formatter, and the
  `link_inventory_item` / `update_group_info` / `update_group_title` REPL tokens
  (with `build_new_inventory_link` / `build_update_group_info_params` helpers,
  the link builder choosing item vs folder via a `folder_link` flag). Covered by
  two pack-the-wire lifecycle tests and four REPL parse tests. Group ops are
  SL-testable against aditi; `LinkInventoryItem` works on OpenSim.
- **Out batch 7 — teleport & agent prefs.** `TeleportLandmarkRequest` (teleport
  to a landmark), `TeleportCancel` (cancel an in-progress teleport),
  `SetStartLocationRequest` (set home), `AgentDataUpdateRequest`,
  `AgentQuitCopy` (crash-quit leaving objects), `VelocityInterpolateOn` /
  `VelocityInterpolateOff`.

  Implemented as `Session::teleport_via_landmark(landmark: Option<AssetKey>)`
  (the `LandmarkID` is the landmark inventory item's *asset* id, `None` =
  home; mirrors `teleport_to`'s state machine — arms the teleport timeout and
  enters [`TeleportPhase::Requested`] with no destination hint, since a
  landmark teleport resolves sim-side and the authoritative handle arrives with
  the `TeleportFinish`), `Session::cancel_teleport` (sends `TeleportCancel` and,
  if teleporting, returns to the active state and disarms the timeout),
  `Session::set_start_location(slot: StartLocationSlot, position:
  RegionCoordinates, look_at: Vector)`, `Session::request_agent_data_update`,
  `Session::quit_copy`, and `Session::set_velocity_interpolation(enabled:
  bool)` (one method dispatching to the On/Off messages). The only new domain
  type is a typed [`StartLocationSlot`] enum (`Last`/`Home`/`Direct`/`Parcel`/
  `Telehub`/`Url`, the reference viewer's `EStartLocation` ordinal) replacing
  the raw `LocationID` `u32` — kept distinct from the existing login
  [`StartLocation`] (the SLURL-style `start=` parameter, a different shape that
  bundles region+position and has no wire ordinal); the `SimName` is sent empty
  (the simulator fills the region name, as the reference viewer does) and
  `AgentQuitCopy`'s `ViewerCircuitCode` reuses the circuit's own code. Wired as
  `Command::{TeleportViaLandmark, CancelTeleport, SetStartLocation,
  RequestAgentDataUpdate, QuitCopy, SetVelocityInterpolation}` through the tokio
  and bevy runtimes, the `command_name` formatter, and the matching REPL tokens
  (with a `parse_start_location_slot` helper). Covered by three pack-the-wire
  lifecycle tests, one `StartLocationSlot` round-trip unit test, and three REPL
  parse tests. Teleport-to-landmark/home, cancel, set-home, agent-data poll,
  and velocity interpolation are OpenSim-testable; `AgentQuitCopy` is an
  inter-sim quit best exercised against SL.
- **Out batch 8 — user info & sound.** `UserInfoRequest` / `UpdateUserInfo`
  (read/write the email & IM-forwarding prefs — the outbound side of the
  inbound batch-6 `UserInfoReply`), `SoundTrigger` (trigger a sound at the
  agent's position).
- **Out batch 9 — god region/estate admin.** `RequestGodlikePowers`,
  `EjectUser`, `FreezeUser`, `GodUpdateRegionInfo`, `SimWideDeletes`. All
  `NotTrusted` and viewer-sent with the god bit set; gated on the agent holding
  god/estate powers.
- **Out batch 10 — god parcel/object/land admin.** `ParcelGodForceOwner`,
  `ParcelGodMarkAsContent`, `EventGodDelete`, `StateSave` (god object state
  save), `ViewerStartAuction`.

## Verification

- Per batch: `cargo fmt --all`, `cargo clippy`, `cargo test -p sl-proto -p
  sl-repl`.
- Live check against aditi with the existing smoke script (see
  `scripts/aditi-smoke.repl`): confirm the message's `WARN UnhandledMessage`
  line is gone and the new `Event` is surfaced. Batch 1 specifically should
  eliminate the `SimStats` / `SimulatorViewerTimeMessage` warning flood called
  out in `KNOWN_ISSUES_ADITI.md` issue 2.

## Status

- [x] Batch 1 — region telemetry (SimStats, SimulatorViewerTimeMessage)
- [x] Batch 2 — generic message family (GenericMessage, LargeGenericMessage,
  GenericStreamingMessage)
- [x] Batch 3 — session errors & forced disconnect (Error, FeatureDisabled,
  KickUser)
- [x] Batch 4 — scene & appearance (ObjectAnimation, RebakeAvatarTextures)
- [x] Batch 5 — friendship & calling cards
- [x] Batch 6 — inventory sync, task inventory & misc
- [x] EQ batch 1 — pathfinding agent state (AgentStateUpdate — closes issue 3)
- [x] EQ batch 2 — group & display-name pushes (AgentDropGroup,
  DisplayNameUpdate, SetDisplayNameReply)
- [x] EQ batch 3 — region/environment/voice misc (WindLightRefresh,
  SimConsoleResponse, RequiredVoiceVersion, OpenRegionInfo)
- [x] Phase 0 — outbound audit (55 in-template gap messages: 41 HANDLE /
  14 SKIP; 10 outbound batches defined)
- [x] Out batch 1 — calling cards (OfferCallingCard, AcceptCallingCard,
  DeclineCallingCard)
- [x] Out batch 2 — object prim editing (ObjectShape, ObjectExtraParams,
  ObjectImage)
- [x] Out batch 3 — rez & script permissions (RezObject, RezScript,
  RevokePermissions, DetachAttachmentIntoInv)
- [x] Out batch 4 — task inventory (RequestTaskInventory, UpdateTaskInventory,
  MoveTaskInventory, RemoveTaskInventory)
- [x] Out batch 5 — land & parcel (ModifyLand, UndoLand,
  ParcelPropertiesRequestByID, ParcelSetOtherCleanTime)
- [x] Out batch 6 — inventory link & group info (LinkInventoryItem,
  GroupTitleUpdate, UpdateGroupInfo)
- [x] Out batch 7 — teleport & agent prefs (TeleportLandmarkRequest,
  TeleportCancel, SetStartLocationRequest, AgentDataUpdateRequest,
  AgentQuitCopy, VelocityInterpolateOn, VelocityInterpolateOff)
- [ ] Out batch 8 — user info & sound (UserInfoRequest, UpdateUserInfo,
  SoundTrigger)
- [ ] Out batch 9 — god region/estate admin (RequestGodlikePowers, EjectUser,
  FreezeUser, GodUpdateRegionInfo, SimWideDeletes)
- [ ] Out batch 10 — god parcel/object/land admin (ParcelGodForceOwner,
  ParcelGodMarkAsContent, EventGodDelete, StateSave, ViewerStartAuction)
