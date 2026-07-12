# Context — MISSING_ROADMAP.md

Non-task material from `MISSING_ROADMAP.md`: the audit method, the full inbound
/ CAPS / outbound disposition tables (HANDLE/SKIP/DEFER), skip rationale,
verification, status checklist, and server-side parity. The implemented batches
are `missing-*` task files.

## Context

The live aditi smoke test (2026-06-25) surfaced inbound LLUDP messages the
client receives but does not handle — they fell through to
`Diagnostic::UnhandledMessage` and were logged as `WARN`, dropping useful data
(`SimStats`, `SimulatorViewerTimeMessage`; the aditi issue 2, now
[`aditi-issues.md`](aditi-issues.md) / [[aditi-2]]). Investigating that revealed
a broader gap.

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
- [x] Out batch 8 — user info & sound (UserInfoRequest, UpdateUserInfo,
  SoundTrigger)
- [x] Out batch 9 — god region/estate admin (RequestGodlikePowers, EjectUser,
  FreezeUser, GodUpdateRegionInfo, SimWideDeletes)
- [x] Out batch 10 — god parcel/object/land admin (ParcelGodForceOwner,
  ParcelGodMarkAsContent, EventGodDelete, StateSave, ViewerStartAuction)

## Server-side (SimSession) parity

The batches above made the **client** (`Session`) handle every relevant message
in both directions. `SimSession` (`sl-proto/src/sim_session.rs`) is the sans-IO
**mirror** of `Session`, and the workspace's goal is a fully bidirectional
surface tested by pairing the two and passing buffers between them (see
`book/src/architecture.md`). Today `SimSession` has **no inverse for any of the
batches above**, so this part tracks closing that gap. Two rules invert the
client audit:

- A message the **client receives** (inbound batches 1–6 / EQ batches 1–3) must
  be **emittable** by `SimSession` — a typed `send_*` UDP encoder or a typed
  `enqueue_*` CAPS event-queue helper, built on the existing
  `SimSession::push` and `enqueue_caps_event` primitives and mirroring
  `send_chat_from_simulator` / `send_region_handshake`. No new `ServerEvent` is
  needed (these are server→client only).
- A message the **client sends** (outbound batches 1–10) must be **decoded** by
  `SimSession` into a typed `ServerEvent` variant (extend the `ServerEvent` enum
  and add a dispatch arm), replacing the lossy
  `ServerEvent::ClientMessage(Box<AnyMessage>)` fall-through and mirroring the
  existing `AgentUpdate` arm.

All sim-side work **reuses the client-side domain structs** already defined
(`GenericMessage`, `ServerError`, `RestoreItem`, `GodRegionUpdate`,
`EjectAction`, `SimWideDeleteFlags`, etc.) — no new wire types. Each batch is
exercised by a **round-trip** test: pair a `Session` with a `SimSession`, pass
the encoded buffer across, and assert the peer decodes the expected
`Event` / `ServerEvent`.

- **Sim inbound batches 1–6 — `send_*` encoders** for the 23 server→client
  messages, mirroring client inbound batches 1–6: batch 1 `send_sim_stats` /
  `send_simulator_time`; batch 2 `send_generic_message` /
  `send_large_generic_message` / `send_generic_streaming_message`; batch 3
  `send_error` / `send_feature_disabled` / `send_kick_user`; batch 4
  `send_object_animation` / `send_rebake_avatar_textures`; batch 5
  `send_terminate_friendship` plus the calling-card trio (`send_offer_` /
  `send_accept_` / `send_decline_calling_card`); batch 6 the inventory
  remove/move set (`send_remove_inventory_item` / `_folder` / `_objects`,
  `send_move_inventory_item`), `send_reply_task_inventory`,
  `send_user_info_reply`, `send_derez_ack`, `send_force_object_select`,
  `send_grant_godlike_powers`.
- **Sim EQ batches 1–3 — typed `enqueue_*` CAPS helpers** for the 9 push events,
  mirroring client EQ batches 1–3 (AgentStateUpdate, NavMeshStatus,
  AgentDropGroup, DisplayNameUpdate, SetDisplayNameReply, WindLightRefresh,
  SimConsoleResponse, RequiredVoiceVersion, OpenRegionInfo). Each wraps
  `enqueue_caps_event` with an LLSD builder that **inverts** the client's
  matching `*_from_llsd` conversion in `session/conversions.rs`.
- **Sim outbound batches 1–10 — decode + `ServerEvent` variants** for the 40
  client→server messages, grouped exactly as client out-batches 1–10 (calling
  cards; prim editing; rez & script perms; task inventory; land & parcel;
  inventory link & group info; teleport & agent prefs; user info & sound; god
  region/estate admin; god parcel/object/land admin). Each adds one
  `ServerEvent` variant carrying the decoded domain payload and one dispatch
  arm ahead of the `ClientMessage` fall-through.
- **SKIP (no sim mirror), carried over from the client audit:** transport
  (ping/circuit — already handled in `SimSession`'s circuit management),
  sim↔sim trust, and the deprecated Xfer/AIS3/NVP families. `Error` is
  encoder-only on the sim side: the client treats it as receive-only
  (`Event::ServerError`), so the sim needs `send_error` but no client-sent
  `Error` handler.

### Server-side status

- [x] Sim inbound batch 1 — region telemetry encoders (send_sim_stats,
  send_simulator_time)
- [x] Sim inbound batch 2 — generic message family encoders
  (send_generic_message, send_large_generic_message,
  send_generic_streaming_message)
- [x] Sim inbound batch 3 — session error & disconnect encoders (send_error,
  send_feature_disabled, send_kick_user)
- [x] Sim inbound batch 4 — scene & appearance encoders
  (send_object_animation, send_rebake_avatar_textures)
- [x] Sim inbound batch 5 — friendship & calling-card encoders
  (send_terminate_friendship, send_offer_calling_card,
  send_accept_calling_card, send_decline_calling_card)
- [x] Sim inbound batch 6 — inventory sync, task inventory & misc encoders
  (send_remove_inventory_item / _folder / _objects, send_move_inventory_item,
  send_reply_task_inventory, send_user_info_reply, send_derez_ack,
  send_force_object_select, send_grant_godlike_powers)
- [x] Sim EQ batches 1–3 — typed CAPS event-queue enqueue helpers
  (enqueue_agent_state_update, enqueue_nav_mesh_status,
  enqueue_agent_drop_group, enqueue_display_name_update,
  enqueue_set_display_name_reply, enqueue_windlight_refresh,
  enqueue_sim_console_response, enqueue_required_voice_version,
  enqueue_open_region_info)
- [x] Sim out batch 1 — calling-card ServerEvents (CallingCardOffered,
  CallingCardAccepted, CallingCardDeclined)
- [x] Sim out batch 2 — object prim-editing ServerEvents (ObjectShapeSet,
  ObjectImageSet, ObjectExtraParamsSet)
- [x] Sim out batch 3 — rez & script-permission ServerEvents
  (RezObjectFromInventory, RezScript, RevokeScriptPermissions,
  DetachAttachmentIntoInventory; new `restore_item_from_inventory_block!` macro
  shared with the existing `RezRestoreToWorld` decode)
- [x] Sim out batch 4 — task-inventory ServerEvents (RequestTaskInventory,
      UpdateTaskInventory, MoveTaskInventory, RemoveTaskInventory)
- [x] Sim out batch 5 — land & parcel ServerEvents (ModifyLand -> LandEdit,
  UndoLand, RequestParcelPropertiesById -> RegionLocalParcelId + sequence_id,
  SetParcelOtherCleanTime -> RegionLocalParcelId + Duration; new LandBrushSize
  from_metres/from_index and LandBrushAction::from_code decoders)
- [x] Sim out batch 6 — inventory link & group-info ServerEvents
  (LinkInventoryItem -> NewInventoryLink + callback_id, item/folder selected by
  the AT_LINK_FOLDER (25) AssetType byte; UpdateGroupInfo ->
  UpdateGroupInfoParams with nil-insignia -> None and linden_from_wire fee;
  GroupTitleUpdate -> GroupKey + GroupRoleKey)
- [x] Sim out batch 7 — teleport & agent-prefs ServerEvents
  (TeleportLandmarkRequest -> TeleportViaLandmark with nil LandmarkID -> None
  home teleport; TeleportCancel -> CancelTeleport; SetStartLocationRequest ->
  SetStartLocation with typed StartLocationSlot::from_code and region-local
  RegionCoordinates, unrecognised slot -> ClientMessage fall-through;
  AgentDataUpdateRequest -> RequestAgentDataUpdate; AgentQuitCopy -> QuitCopy
  with typed CircuitCode; VelocityInterpolateOn/Off -> SetVelocityInterpolation
  { enabled })
- [x] Sim out batch 8 — user-info & sound ServerEvents (UserInfoRequest ->
  RequestUserInfo; UpdateUserInfo -> UpdateUserInfo { im_via_email,
  directory_visibility } via DirectoryVisibility::from_wire; SoundTrigger ->
  TriggerSound { sound, gain, region_handle, position } with the region-local
  pos as RegionCoordinates and nil owner/object/parent ids dropped)
- [x] Sim out batch 9 — god region/estate-admin ServerEvents
  (RequestGodlikePowers -> RequestGodlikePowers { godlike }, nil Token dropped;
  EjectUser -> EjectUser { target, action } via EjectAction::from_wire;
  FreezeUser -> FreezeUser { target, action } via FreezeAction::from_wire;
  SimWideDeletes -> SimWideDeletes { owner, flags } via
  SimWideDeleteFlags::from_wire; GodUpdateRegionInfo -> GodUpdateRegionInfo {
  update: GodRegionUpdate } recovering the 64-bit RegionFlagsExtended from
  RegionInfo2 with a legacy-32-bit fallback and typed RegionName /
  GridCoordinates; unrecognised eject/freeze/delete flags and an empty/invalid
  SimName fall through to ClientMessage)
- [x] Sim out batch 10 — god parcel/object/land-admin ServerEvents
  (ParcelGodForceOwner -> RegionLocalParcelId + OwnerKey::Agent, the wire
  carries no group flag so the new owner is always an agent;
  ParcelGodMarkAsContent -> RegionLocalParcelId; EventGodDelete -> EventId +
  QueryId + query_text + DirFindFlags + query_start, mirroring the events
  re-run; StateSave -> Option<String> filename with empty -> None autosave;
  ViewerStartAuction -> RegionLocalParcelId + Option<TextureKey> snapshot,
  nil id -> None. Five dispatch arms ahead of the ClientMessage fall-through,
  reusing the existing typed keys; no new wire types. Round-trip test
  client_god_parcel_admin_reaches_simulator)

## Book documentation

The mdBook (`book/`) lags the protocol surface the batches above added. The
book's convention is to fold the server-side encoders into each feature
chapter's `> **In this codebase**` note (see `content/search.md`,
`content/groups.md`, `content/object-commerce.md`), so these doc steps pair
with the SimSession parity work — document a feature's server side as its
encoders/handlers land. Follow the house style: kebab-case filenames, a single
`#` title, `##` / `###` sections, 80-column wrap (rumdl `reflow = true`),
backticked type/path references, relative-link cross-references, and the
`> **In this codebase**` blockquote with Types / Commands / Events / Server
events / REPL sub-bullets.

- **New chapter `comms/generic-messages.md` (Part 1 — Communication Layer,
  immediately after `Messages & the Template`).** The generic message
  mechanism. Sections: motivation (loosely-coupled features that don't warrant
  a dedicated message); the method-envelope shape; the three flavours
  (`GenericMessage` Low 261 — method name + invoice + variable param list;
  `LargeGenericMessage` Low 430 — same shape, larger per-param wire limit;
  `GenericStreamingMessage` High 31 — numeric method id + a single binary
  blob); worked examples (`emptymutelist`, `GrantUserRights`, the `0x4175` GLTF
  material override); the "parameter parsing is the feature handler's job"
  contract; and an `> **In this codebase**` note citing
  `sl-proto/src/types/generic.rs`, the `Event::GenericMessage` /
  `Event::LargeGenericMessage` / `Event::GenericStreamingMessage` variants, and
  the `SimSession::send_generic_*` encoders once they land. Add the
  `SUMMARY.md` entry under `[Messages & the Template](../../comms/messages.md)`.
- **New chapter `content/god-tools.md` (Part 2 — Content Layer, after
  `Region & Estate Information`).** The god/estate-admin surface: inbound
  `GrantGodlikePowers` / `ForceObjectSelect` plus out-batches 9–10
  (RequestGodlikePowers, Eject/Freeze, GodUpdateRegionInfo, SimWideDeletes,
  ParcelGodForceOwner / ParcelGodMarkAsContent, EventGodDelete, StateSave,
  ViewerStartAuction). Add the `SUMMARY.md` entry after
  `[Region & Estate Information](../../content/region.md)`.
- **Extend existing chapters** for the remaining new families, per the audit's
  per-chapter map: `comms/sessions.md` (Error / FeatureDisabled / KickUser
  error handling); `content/region.md` (SimStats / SimulatorTime telemetry +
  land & parcel out-batch 5); `content/friends.md` (calling cards +
  TerminateFriendship); `content/profiles.md` (UserInfo + display-name EQ
  events + UpdateUserInfo); `content/world.md` (ObjectAnimation + prim-edit
  out-batch 2); `content/inventory.md` + `content/scripts.md` (task inventory,
  inventory remove/move, LinkInventoryItem); `content/groups.md` (group title /
  info); `content/teleport.md` (out-batch 7); `content/sound-media.md`
  (SoundTrigger); `content/appearance.md` (RebakeAvatarTextures);
  `content/attachments.md` (DetachAttachmentIntoInv).
- **EventQueue reference** — extend `comms/caps.md` with a table of the
  recognised event-queue event names and their LLSD bodies (the 9 EQ-batch
  events), so an unrecognised event is easy to place.

### Documentation status

- [x] New chapter — `comms/generic-messages.md` (generic message mechanism)
- [x] New chapter — `content/god-tools.md` (god/estate-admin surface)
- [x] Extend `comms/sessions.md` — session errors & forced disconnect
- [x] Extend region/telemetry & land/parcel chapters
- [x] Extend friends/profiles/groups chapters (calling cards, display names,
  user info, group title/info)
- [x] Extend world/scripts/inventory/attachments chapters (animations, prim
  editing, task inventory, rez, detach)
- [x] Extend teleport & sound-media & appearance chapters
- [x] EventQueue event reference in `comms/caps.md`
