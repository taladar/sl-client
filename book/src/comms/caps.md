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

- **Bulk asset access** — `GetTexture`, `GetMesh2`, `ViewerAsset` (generic
  assets by class, e.g. `?sound_id=`/`?bodypart_id=`).
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

## The server side

The same codecs work in the grid direction. Every client-direction
`build_*_request` / `parse_*_response` pair gains (or will gain) its
server-direction `parse_*_request` / `build_*_response` inverse — the
**inverse-pairing convention** — verified by round-tripping against the
client functions in memory. For the framework itself those inverses are
`parse_seed_request` / `build_seed_response` (the seed grant) and
`parse_event_queue_request` (pairing with the long-standing
`build_event_queue_response`).

`SimCaps` is the dispatch registry a simulator hangs its capability
handlers off. It mints one unguessable `…/cap/<uuid>` URL per *served*
capability under a public base URL (token randomness is caller-supplied —
the sans-I/O crates never roll dice themselves), answers the seed POST
with the granted `name → URL` map, and routes each request on a granted
URL to its handler. Three properties mirror what real grids do:

- **The grant is idempotent.** All tokens are minted at construction, so a
  retried seed POST — the reference viewer retries up to 30 times —
  receives a byte-identical reply (the LLSD serializer's sorted map keys
  make equal grants serialize identically).
- **Unsupported names are omitted**, which is the protocol's feature
  negotiation (see above); a capability only enters the grant once a
  server-side handler exists for it.
- **The seed URL is a plain value.** `SimCaps` holds no login state; the
  login server — possibly a *different process* — just embeds
  `SimCaps::seed_url()` in its login response's `seed_capability` field.
  Nothing else crosses the login↔simulator boundary, so the login and
  CAPS HTTP servers stay independently deployable.

The `EventQueueGet` handler implements the server half of the long-poll
against `SimSession`'s event buffer:

- events queued → `200` with the next `{ id, events }` batch;
- nothing queued → a *would-block* outcome: the (future) HTTP glue holds
  the request open and, when its hold (~30 s in the reference stack)
  expires, answers `502` — the status the viewer treats as "nothing yet,
  re-poll";
- `done: true` → teardown; that poll is answered and every later one gets
  `404`, the viewer's "stop polling" signal (a closed session answers
  `404` too).

The `ack` field is parsed but deliberately fire-and-forget, exactly like
OpenSim's event-queue module: a batch is dropped when it is serialized,
so a response lost in transit loses that batch. Nothing keys on `ack`,
which also makes the batch id's eventual `i32` wrap harmless.

### The agent-communication handlers

The first served capability family beyond the framework is the
agent-communication cluster. Each handler reads and writes `SimSession`
state and validates its request the same way (wrong HTTP method → `405`,
non-LLSD or unroutable body → `400`):

- **`ChatSessionRequest`** routes on the body's `method` string. A
  `start conference` registers an ad-hoc session holding the starter and
  the body's `params` invitees, pushes `ConferenceStartRequested` for the
  driver to relay, and answers the new roster (a body naming no invitee
  is a `400`). An `invite` adds its `params` to a session that already
  exists — the modern "add participants" — answering the grown roster
  and pushing `SessionInviteRequested`; an unknown session is a `400`,
  since unlike a start it is supposed to exist. An
  `accept invitation` adds this circuit's agent to the session's roster
  (the same registry `SimSession::open_chat_session` and the IM relay
  maintain) and answers the roster as the modern `agent_info` map; an
  unknown session answers an *empty* map rather than an error, mirroring
  OpenSim's stubbed cap. A `decline invitation` removes the agent
  (dropping an emptied session) and acks with an undefined body, as does
  `decline p2p voice`. A `fetch history` answers the session's
  server-side backlog (`SimChatSession::history`, fed by
  `send_session_message` or the `record_session_history` driver API) as
  the bare array the wire carries. Relaying joins and rosters to *other*
  participants' sessions stays the driver's job — the same relay
  topology as conference start — via `send_session_participant` and the
  two event-queue pushes `enqueue_chatterbox_invitation` /
  `enqueue_chatterbox_agent_list_updates`.
- **`ReadOfflineMsgs`** is a GET serving the messages stored by
  `SimSession::store_offline_message` while the agent was offline —
  **deliver-once**, OpenSim's delete-on-fetch semantics: the fetch
  drains the store, so a repeated GET answers an empty array.
- **`AvatarPickerSearch`** is a GET serving the "Choose Resident" name
  search over the same display-name store: the residents whose username,
  display name or legacy name matches the `names` query parameter (capped
  at its `page_size`), as an `agents` array of the very records
  `GetDisplayNames` answers with. A query naming nobody is a `400`; one
  matching nobody is an empty — but successful — array, since a search
  answers with what it found.
- **`GetDisplayNames`** is a GET answering each `ids` query parameter
  from the session's display-name store (`SimSession::set_display_name`):
  known agents as full `agents` records, unknown ids as `bad_ids` — the
  grid's "could not resolve" form the client folds into placeholder
  records.
- **`AgentPreferences`** merges the POSTed `Some` fields into the
  session's stored preference set and echoes the **full** stored set, so
  the client's empty-body POST is the pure "get". The store starts at
  OpenSim's defaults (access ceiling `M`, language `en-us` and public,
  hover height 0, zero permission masks, god level 0); `god_level` in a
  request is ignored (reply-only).
- **`SendUserReport`** parses the abuse report and surfaces it as the
  same `ServerEvent::AbuseReportReceived` the legacy UDP `UserReport`
  path pushes; the reply is an undefined body the client discards.
  **`SendUserReportWithScreenshot`** is the two-step uploader: the
  report POST parks the report and answers
  `{ state: "upload", uploader }`, minting the uploader URL as the
  cap's own `screenshot` sub-path (path resolution tolerates sub-paths
  below a token, so the second step routes back to the same handler);
  the raw JPEG-2000 bytes POSTed there join the parked report as
  `ServerEvent::AbuseReportWithScreenshotReceived` and answer
  `{ state: "complete" }`. A bytes-POST with no parked report is a
  `400`.

The inverses added for this family follow the pairing convention:
`chat_session_request_from_llsd` / `chat_session_roster_to_llsd` /
`session_history_to_llsd` / `agent_list_voice_updates_to_llsd` next to
their client counterparts in `sl-proto/src/session/conversions.rs`, and
`build_asset_upload_response` next to `parse_asset_upload_response` in
`sl-wire/src/llsd.rs`. The display-name, agent-preferences, and
abuse-report codecs already had their server directions
(`build_display_names_response`, `build_agent_preferences_response`,
`parse_send_user_report`); this cluster wired them into the dispatch.

### The asset-delivery handlers

`GetTexture`, `GetMesh`, `GetMesh2` and `ViewerAsset` are different in
kind from every other cap: they stream **binary asset bytes** over HTTP
`GET` with byte-range requests, not LLSD, and — crucially — they need no
`SimSession` state at all. On Second Life they are served by a **content
delivery network on a different host** from the simulator (the seed grant
advertises CDN URLs); avatar *baking* is yet another separate service.
So this family lives on its own session-free surface, `AssetCaps`
(`sl-proto/src/asset_caps.rs`), which dispatches against an `AssetSource`
(a byte store keyed by asset UUID) instead of a `SimSession`:

- **The byte source.** `AssetSource` is the read-only
  UUID→bytes trait the four caps serve from, mirroring OpenSim's
  `IAssetService.Get(uuid)` — one asset's bytes are served regardless of
  which cap asked; the cap only picks the `Content-Type`. The pure
  in-memory `InMemoryAssetSource` is the fixture; a directory-backed
  source is `sl_client_tokio::load_asset_dir`, an eager loader that reads
  a flat `<uuid>[.ext]` directory into an `InMemoryAssetSource` at
  construction so the serving path itself stays sans-I/O.
- **The request.** A `GET` on the cap URL with a `?<class>_id=<uuid>`
  selector — `texture_id` for `GetTexture`, `mesh_id` for
  `GetMesh`/`GetMesh2`, and any `AssetType::from_asset_query_key` key for
  `ViewerAsset` (the inverse of the client's `get_asset_query_key`
  fetch-URL builder) — plus an optional `Range: bytes=start-end` header
  (inclusive `end`). `CapsRequest` grew a `range` field to carry it.
- **The response.** No `Range` → `200` with the whole asset and its
  content type (`image/x-j2c`, `application/vnd.ll.mesh`, or
  `application/octet-stream`). A satisfiable range → `206` with the byte
  slice and a `Content-Range: bytes start-last/total` header
  (`CapsResponse` grew a `content_range` field). A start past the end of
  an **existing** asset → `416` with `Content-Range: bytes */total`; a
  missing asset → `404`; a non-`GET` → `405`. The `416`-on-overrun is
  HTTP-correct and the client turns it into an empty chunk and stops;
  OpenSim instead serves the whole asset there to dodge a reference-viewer
  416 bug, but our client handles `416` cleanly so we stay spec-correct.

`SimCaps` composes one `AssetCaps` purely so a single seed grant
advertises the asset caps alongside the sim caps
(`SimCaps::new` co-locates them, `SimCaps::new_split` mints them under a
separate CDN base URL); the asset dispatch path is reached through
`SimCaps::assets()` and stays independent. A CDN process rebuilds the
surface with `AssetCaps::from_tokens` from the token map the simulator
advertised — the only value that crosses that boundary.

### The content upload & media handlers

The largest cluster is content creation and editing: the asset
**upload/update** caps, the **materials** caps, and **media-on-a-prim**
(MOAP). These write world state that a real grid's world authority owns —
which is out of scope here — so their read side serves from small
driver-populated stores on `SimSession` (the same pattern as the
display-name store), and every mutation surfaces to the driver as a
`ServerEvent` rather than mutating a prim database.

**The two-stage uploader.** `NewFileAgentInventory`, `UploadBakedTexture` and
the whole
`Update{Gesture,Notecard,Script,Settings,Material}{Agent,Task}Inventory` family
share one server-side state machine — the generalisation of the
`SendUserReportWithScreenshot` uploader. All route to one
`CapHandler::AssetUpload`; the cap name only picks which step-1 metadata parser
runs:

- **Step 1** (a `POST` to the cap URL) parses the cap's metadata body into a
  `CapsUploadMetadata`, parks it under the cap name
  (`SimSession::park_caps_upload`), and answers `{ state: "upload", uploader }`
  — the uploader URL is the cap's own `upload` sub-path (path resolution
  tolerates sub-paths below a token, so step 2 routes back to the same
  handler, exactly like the screenshot uploader's `screenshot` sub-path).
- **Step 2** (a `POST` of the raw asset bytes to that sub-path) takes the
  parked metadata, has the session mint the stored ids and push
  `ServerEvent::CapsAssetUploaded { metadata, new_asset, new_inventory_item,
  data }`, and answers `{ state: "complete", new_asset, new_inventory_item? }`
  — plus `{ compiled: true, errors: [] }` for the two `Update*Script*` caps
  (their completion carries the compile result). `UploadBakedTexture`
  produces a temporary asset with **no** inventory item; a bytes-POST with no
  parked upload is a `400`.

The minted `new_asset` / `new_inventory_item` ids come from a monotonic
per-session serial (`SimSession::next_sim_serial`) — a deliberate
simplification: a real grid mints random asset ids, but the client stores
whatever id it is handed, so the value's structure is immaterial and a
deterministic counter keeps `SimSession` pure (no clock, no RNG). The same
serial mints the `x-mv:<serial>/<uuid>` MOAP version strings.

**Single-POST inventory caps.** `UpdateAvatarAppearance` parses the requested
Current Outfit Folder version, surfaces
`ServerEvent::ServerAppearanceRequested`, and answers the accept reply
`{ success: true }` (the baked-texture ids arrive separately over UDP
`AvatarAppearance`). `CopyInventoryFromNotecard` is a one-way POST — it surfaces
`ServerEvent::CopyInventoryFromNotecardRequested` and acks with an undefined
body (the copied item is delivered over the normal inventory-update stream).

**Materials.** `RenderMaterials` routes on HTTP method: a `POST` (the zipped
id list) or `GET` (all) queries the session's driver-populated
`region_materials` store and answers the matching legacy materials; a `PUT`
sets legacy materials on faces, surfacing `ServerEvent::RenderMaterialsSet`.
`ModifyMaterialParams` parses the per-face GLTF assignments, surfaces
`ServerEvent::MaterialParamsModified`, and answers `{ success: true,
message: "" }`. `UpdateMaterialAgentInventory` is just another two-stage
uploader cap.

**MOAP.** `ObjectMedia` routes on the body's `verb`: a `GET` answers the
object's stored per-face media as an `ObjectMediaResponse` (an unknown object
gets an empty, tolerant media list); an `UPDATE` records the new media under a
freshly advanced version and surfaces `ServerEvent::ObjectMediaSet`.
`ObjectMediaNavigate` advances the object's media version and surfaces
`ServerEvent::ObjectMediaNavigated`. `ObjectAnimation` stays unserved: it is
never POSTed — listing it in the seed only opts a viewer into the UDP
`ObjectAnimation` stream.

The inverses added for this cluster follow the pairing convention:
`parse_new_file_agent_inventory_request`, `parse_update_item_asset_request`,
`parse_update_task_item_asset_request`,
`parse_update_script_{agent,task}_request`,
`parse_update_avatar_appearance_request`, the MOAP `parse_object_media_request`
/ `parse_object_media_navigate_request` / `ObjectMediaResponse::to_llsd`, and
the materials `parse_render_materials_request` /
`parse_render_materials_put_request` / `build_modify_material_params_response` —
beside their client-direction partners in `sl-wire`, plus
`parse_copy_inventory_from_notecard` next to its builder in `sl-proto`. The
shared `build_asset_upload_response`, `server_appearance_update_to_llsd`,
`build_render_materials_response` and `parse_modify_material_params_request`
already existed; this cluster wired them into the dispatch. Loopback coverage is
in `sl-proto/tests/sim_caps.rs`.

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
> - The server-direction inverses (`parse_seed_request` /
>   `build_seed_response`, `parse_event_queue_request` with its
>   `EventQueueRequest` type, and `build_event_queue_response`) sit next to
>   their client pairs in `sl-wire/src/llsd.rs`. `SimCaps` — with the
>   transport-agnostic `CapsRequest` / `CapsResponse` / `CapsDispatch`
>   types — is `sl-proto/src/sim_caps.rs`; its pinned coverage table
>   (`caps_coverage_table_is_pinned`, one row per `REQUESTED_CAPABILITIES`
>   entry) is the ledger the `protocol-sim-caps-*` cluster tasks tick off,
>   and the in-memory loopback tests driving the client's own builders
>   against `SimCaps::dispatch` are `sl-proto/tests/sim_caps.rs`.
> - The session-free **asset-delivery** surface is
>   `sl-proto/src/asset_caps.rs` (`AssetCaps`, `AssetCapHandler`), served
>   from the `AssetSource` / `InMemoryAssetSource` byte store in
>   `sl-proto/src/asset_source.rs`; the directory-backed loader
>   `load_asset_dir` and the real-client round-trip test are in
>   `sl-client-tokio` (`src/assets.rs`, `tests/asset_caps_roundtrip.rs`).
>   The coverage table's predicate consults both `SimCaps::handler_for`
>   and `AssetCaps::handler_for`.
