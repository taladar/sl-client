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

### Batch 6 — inventory sync, task inventory & misc

Server-initiated inventory mutations to keep a client mirror current:
`RemoveInventoryItem` (270), `RemoveInventoryFolder` (276),
`RemoveInventoryObjects` (284), `MoveInventoryItem` (268). Plus
`ReplyTaskInventory` (290, object contents), `UserInfoReply` (400, email/IM
prefs), `DeRezAck` (292), `ForceObjectSelect` (205), `GrantGodlikePowers` (258).

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
- **EQ batch 2 — group & display-name pushes.** `AgentDropGroup` (the sim
  dropped the agent from a group), `DisplayNameUpdate` (a cached display name
  changed), `SetDisplayNameReply` (result of the agent's own set-display-name).
- **EQ batch 3 — region/environment/voice misc.** `WindLightRefresh`
  (re-fetch environment), `SimConsoleResponse` (reply to a region
  debug-console command), `RequiredVoiceVersion` (voice protocol version),
  `OpenRegionInfo` (OpenSim extended region settings).

## Outbound gap — Phase 0 audit required

The outbound gap could **not** be auto-computed: the client builds outbound
messages through dedicated `send_*` helpers (typed structs → `circuit.send`),
not `AnyMessage::…` literals, so a name-level diff against the 219 distinct
messages the Firestorm viewer sends is unreliable. The client already exposes
312 REPL commands / many `send_*` methods, so the true gap is smaller than 219.

**Phase 0 task:** reconcile each of the 219 client-sent messages against the
client's existing `send_*` methods / `Command` variants, producing a precise
classified outbound gap (HANDLE / deprecated / sim↔sim). Then batch the
non-outdated client→server messages using the outbound pattern above. Fill this
section with the resulting table before starting outbound batches.

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
- [ ] Batch 5 — friendship & calling cards
- [ ] Batch 6 — inventory sync, task inventory & misc
- [ ] EQ batch 1 — pathfinding agent state (AgentStateUpdate — closes issue 3)
- [ ] EQ batch 2 — group & display-name pushes
- [ ] EQ batch 3 — region/environment/voice misc
- [ ] Phase 0 — outbound audit (fill the outbound gap table)
- [ ] Outbound batches (defined after Phase 0)
