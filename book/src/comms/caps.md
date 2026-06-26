# CAPS & the Event Queue

**CAPS** — short for *capabilities* — is the HTTPS side of the protocol. Where
[LLUDP](lludp-transport.md) carries lossy real-time traffic, CAPS carries the
data that must arrive intact and in order: login follow-ups, inventory,
materials, voice provisioning, map data, and the asynchronous **event queue**.

A capability is a single idea: an **unguessable HTTPS URL that grants access to
one server feature**. Possessing the URL *is* the authorization — there is no
separate token, because the URL itself is the secret. Each capability URL is
per-session and issued by the region, so they cannot be bookmarked or shared.

## The seed capability

You do not get all the capability URLs up front. [Login](../content/login.md)
returns exactly one: the **seed capability**. The client then POSTs to the seed
URL an LLSD-XML array of the capability *names* it wants, and the region replies
with an LLSD-XML map of `name → URL` for the ones it supports:

```text
POST <seed-cap-url>
Content-Type: application/llsd+xml

<llsd><array>
  <string>EventQueueGet</string>
  <string>FetchInventoryDescendents2</string>
  <string>GetTexture</string>
  … the names the client knows how to use …
</array></llsd>

200 OK
<llsd><map>
  <key>EventQueueGet</key>            <string>https://sim.example/cap/abc…</string>
  <key>FetchInventoryDescendents2</key><string>https://sim.example/cap/def…</string>
  …
</map></llsd>
```

The client caches that map for the life of the circuit and looks up a URL by
name whenever it needs the feature. A region that does not implement a given
capability simply omits it from the reply — which is the normal way features
differ between Second Life and OpenSim, and between OpenSim configurations.

Capabilities are re-seeded per region: crossing to or teleporting into a new
region yields a new seed URL and therefore a fresh capability map.

## What capabilities exist

There are dozens. A non-exhaustive sense of the range:

- **Bulk asset access** — `GetTexture`, `GetMesh2`, `GetAsset`.
- **Inventory** — `FetchInventoryDescendents2`, `InventoryAPIv3`,
  `CreateInventoryCategory`. See [Inventory](../content/inventory.md).
- **Appearance** — `UpdateAvatarAppearance`, `UploadBakedTexture`. See
  [Appearance](../content/appearance.md).
- **Media & materials** — `ObjectMedia`, `RenderMaterials`,
  `ModifyMaterialParams`. See [Materials](../content/materials.md) and
  [Sound, Music & Media](../content/sound-media.md).
- **Voice** — `ProvisionVoiceAccountRequest`, `ParcelVoiceInfoRequest`,
  `VoiceSignalingRequest`.
- **Groups** — `GroupMemberData`.
- **Experiences** (Second Life only) — a family of experience capabilities.
- **The event queue** — `EventQueueGet`, described next.

## The event queue (`EventQueueGet`)

Some server events do not fit the lossy LLUDP model — they are infrequent, must
not be lost, and the server originates them whenever it likes (a teleport
finishing, a parcel's properties, a group chat invitation). These are delivered
through a **long-poll** over the `EventQueueGet` capability.

The pattern is the standard HTTP long-poll:

```text
client ──▶ POST EventQueueGet  { ack: <last id, or undef> }
              (server holds the request open until it has events,
               or until a timeout)
server ──▶ 200 OK { id: N, events: [ {message, body}, {message, body}, … ] }
client ──▶ POST EventQueueGet  { ack: N }   ← immediately re-poll, acking N
              …repeat forever…
```

- Each response carries an **`id`** and an array of **events**. Every event has
  a `message` name (e.g. `"TeleportFinish"`, `"ParcelProperties"`,
  `"EstablishAgentCommunication"`, `"ChatterBoxInvitation"`) and a `body` that
  is an arbitrary [LLSD](llsd.md) tree.
- The client immediately re-POSTs, passing the last `id` back as `ack` so the
  server can drop already-delivered events.
- A non-success status (or empty timeout response) just means "nothing yet" —
  the client re-polls. The loop runs for the life of the circuit.

The event queue is where a lot of *content-layer* behaviour actually surfaces,
so many chapters in the next part end with "…delivered via the event queue." A
notable example: rich parcel data (`ParcelProperties`) arrives here rather than
over UDP.

### Recognised event-queue events

Most event-queue events are the asynchronous half of a feature and are
documented in that feature's chapter (`TeleportFinish` →
[Teleport](../content/teleport.md), `ParcelProperties` →
[3D World](../content/world.md), `EstablishAgentCommunication` →
[Circuits](circuits.md), `ChatterBoxInvitation` →
[Chat](../content/chat.md), `ObjectPhysicsProperties` →
[Region](../content/region.md), …). Beyond those, the simulator pushes a
handful of standalone notifications with no UDP equivalent. They are listed here
together because they do not otherwise belong to a request/reply flow — so an
unfamiliar `message` name is easy to place:

| `message` | LLSD body | Decodes to | Grid |
|-----------|-----------|------------|------|
| `AgentStateUpdate` | `{ can_modify_navmesh: bool }` | `Event::AgentStateUpdate` | SL |
| `NavMeshStatusUpdate` | `{ region_id: uuid, version: int, status: string }` | `Event::NavMeshStatus` | SL |
| `AgentDropGroup` | `{ AgentData: [ { AgentID, GroupID } ] }` | `Event::AgentDroppedFromGroup` | both |
| `DisplayNameUpdate` | `{ agent_id: uuid, old_display_name: string, agent: <name record> }` | `Event::DisplayNameUpdate` | SL |
| `SetDisplayNameReply` | `{ status: int, reason: string, content: { display_name \| error_tag } }` | `Event::SetDisplayNameReply` | SL |
| `WindLightRefresh` | `{ Interpolate: int(0/1) }` | `Event::WindLightRefresh` | OpenSim |
| `SimConsoleResponse` | a bare LLSD **string** (the command output) | `Event::SimConsoleResponse` | OpenSim |
| `RequiredVoiceVersion` | `{ major_version: int, region_name: string, voice_server_type?: string }` | `Event::RequiredVoiceVersion` | SL |
| `OpenRegionInfo` | a map of optional OpenSim per-region settings (only overridden keys present) | `Event::OpenRegionInfo` | OpenSim |

Two `message` names differ from their event: the wire `NavMeshStatusUpdate`
becomes `Event::NavMeshStatus`, and `AgentDropGroup` becomes
`Event::AgentDroppedFromGroup`. `SimConsoleResponse` is the lone event whose
body is a bare LLSD scalar rather than a map. Each decoder lives in
`sl-proto/src/session/conversions.rs` (`*_from_llsd`), and the simulator side
has a matching `SimSession::enqueue_*` helper that builds the same body.

Because the grid can deliver an event the client does not recognise, or a body
that does not parse the way the client expects, the event-queue path also
produces [diagnostics](sessions.md#diagnostics): an event whose `message` name
the client has no handler for is an `UnknownCapsEvent`, and one whose body fails
to decode (or whose `from_llsd` returns nothing) is a `CapsDecodeFailed`. As
with the other diagnostics these are off by default and surface only when
enabled.

---

> **In this codebase**
>
> - The capability **name** constants are in `sl-proto/src/session.rs`, exported
>   as `CAP_GET_TEXTURE`, `CAP_FETCH_INVENTORY`, `CAP_PROVISION_VOICE_ACCOUNT`,
>   etc.; `REQUESTED_CAPABILITIES` is the list the client asks the seed for.
> - The seed round-trip is built/parsed by `build_seed_request` /
>   `parse_seed_response`, and the long-poll by `build_event_queue_request` /
>   `parse_event_queue_response` (all in `sl-wire/src/llsd.rs`, re-exported from
>   `sl-proto`). A parsed batch is `EventQueueResponse` { `id`, `events` } with
>   `EventQueueEvent` { `message`, `body` }.
> - The Tokio driver runs the loop in `sl-client-tokio/src/caps.rs`:
>   `fetch_capabilities` does the seed POST, `spawn_event_queue` /
>   `run_event_queue` drive the long-poll and forward each `(message, body)`
>   over an mpsc channel. The Bevy driver mirrors this in
>   `sl-client-bevy/src/caps.rs`.
> - HTTP plumbing shared by the CAPS features is in
>   `sl-client-tokio/src/http.rs` (and `fetch.rs` / `upload.rs`). A failed CAPS
>   request is reported (rather than swallowed into an `Option`) when
>   diagnostics are on, via the `caps::report_caps_failure` sentinel that the
>   run loop turns into an `ExpectedReplyMissing`
>   [diagnostic](sessions.md#diagnostics).
> - The unknown-event and decode-failure [diagnostics](sessions.md#diagnostics)
>   (`UnknownCapsEvent`, `CapsDecodeFailed`) are emitted from the event-queue
>   handling in `sl-proto/src/session.rs` (`handle_caps_event`), which
>   dispatches each recognised `message` name to its typed `Event`.
> - The standalone-notification decoders (`agent_state_update_from_llsd`,
>   `nav_mesh_status_from_llsd`, `agent_drop_group_from_llsd`,
>   `display_name_update_from_llsd`, `set_display_name_reply_from_llsd`,
>   `windlight_refresh_from_llsd`, `sim_console_response_from_llsd`,
>   `required_voice_version_from_llsd`, `open_region_info_from_llsd`) are in
>   `sl-proto/src/session/conversions.rs`; the simulator-side inverses are the
>   matching `SimSession::enqueue_*` helpers (`sl-proto/src/sim_session.rs`),
>   each building the same LLSD body via `enqueue_caps_event`.
