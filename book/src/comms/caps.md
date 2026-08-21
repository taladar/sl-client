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
- **Inventory** — `FetchInventoryDescendents2`, the per-item
  `FetchInventory2`/`FetchLib2`, `InventoryAPIv3`,
  `CreateInventoryCategory`. See [Inventory](../content/inventory.md).
- **Appearance** — `UpdateAvatarAppearance`, `UploadBakedTexture`. See
  [Appearance](../content/appearance.md).
- **Media & materials** — `ObjectMedia`, `RenderMaterials`,
  `ModifyMaterialParams`. See [Materials](../content/materials.md) and
  [Sound, Music & Media](../content/sound-media.md).
- **Region & object info** — `SimulatorFeatures`, `LSLSyntax`,
  `ExtEnvironment` (EEP), `RemoteParcelRequest`, the object cost/physics
  reports (`GetObjectCost`, `GetObjectPhysicsData`, `ResourceCostSelected`)
  and the script-resource reports (`AttachmentResources`, `LandResources`).
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

### The inventory handlers

The inventory cluster serves the modern AIS3 REST surface and the legacy
fetch caps from an **in-memory inventory tree** (`SimInventoryTree`), two of
which live on `SimSession` as driver-populated serving stores: the agent's
tree and the read-only shared Library. Unlike the purely-read stores, the
AIS3 mutations *apply* to the agent tree — it is fixture state, not world
authority — so a follow-up fetch observes the create/rename/move/delete a
test (or the fake grid) just performed, and every affected folder's
`version` bumps exactly as the real service's `_updated_category_versions`
reports it. Each mutation also surfaces a `ServerEvent`
(`InventoryCategoryCreated`, `InventoryLinksCreated`, `…Renamed`, `…Moved`,
`InventoryItemUpdated`/`Moved`/`Removed`, `…Removed`, `…Purged`) for a
driver persisting inventory.

- **`FetchInventoryDescendents2` / `FetchLibDescendents2`** parse the
  `folders` batch (`parse_fetch_inventory_request`, the inverse of
  `build_fetch_inventory_request`) and answer one entry per **known**
  folder — direct children only, honouring `fetch_folders` /
  `fetch_items` / `sort_order` — via the existing
  `inventory_descendents_to_llsd`. Unknown folders are skipped tolerantly,
  matching OpenSim.
- **`FetchInventory2` / `FetchLib2`** are the per-item legacy fetches the
  reference viewer falls back to for items referenced before their folder
  was listed. Both directions were net-new: the request
  (`build_fetch_inventory_items_request` /
  `parse_fetch_inventory_items_request`, body
  `{ agent_id, items: [ { owner_id, item_id } ] }`) and the reply envelope
  (`fetch_inventory_items_to_llsd` / `fetch_inventory_items_from_llsd`,
  `{ agent_id, items }` over the same flat item shape the descendents
  caps use). Unknown ids are omitted (OpenSim-identical) and the reply
  never carries an `error` member — the viewer treats one as a failed
  fetch. The client now requests both caps and folds their replies into
  `Event::InventoryBulkUpdate`. Note the real cap name is `FetchLib2`,
  not "FetchLibrary2".
- **`InventoryAPIv3` / `LibraryAPIv3`** route on HTTP verb × URL sub-path
  exactly as the client builders lay the URLs out: `POST
  /category/<parent>?tid=` creates a folder — or **links**, when the body
  carries a `links` array (the Current Outfit Folder wear path; the link
  items' `asset_id` is the linked object's id); `PATCH /category/<id>`
  renames or (with `{ parent_id }`) moves; `DELETE /category/<id>` removes
  a subtree, `DELETE /category/<id>/children` empties a folder; `GET
  /category/<id>/children?depth=` lists a subtree; `GET`/`PATCH`/`DELETE
  /item/<id>` fetch / update-or-move / remove an item. Mutation replies
  carry the `AisUpdate` change-set meta (`_created_categories`,
  `_updated_category_versions`, …) with the affected objects under
  `_embedded` (`ais_mutation_reply_to_llsd`); deletes answer meta only.
  `LibraryAPIv3` is read-only: its `GET`s serve the Library tree and every
  mutating verb answers `405`. One deliberate divergence: the real AIS
  nests `_embedded` recursively per depth level, but our client parser
  reads only the top level, so a children fetch serves the subtree
  **flattened** into the top-level `_embedded` — information-equivalent
  (uuid-keyed maps, every entry carries its `parent_id`).
- **`CreateInventoryCategory`** (served by OpenSim too, unlike AIS3)
  applies the client-chosen folder id and echoes the request fields via
  `build_create_inventory_category_response`.

The status contract adds one deliberate exception to the tolerant-empty
stance: an unknown **AIS3 target id** answers `404` (the REST convention),
and an invalid move — an unknown new parent, or a folder moved under
itself/its own descendant — answers `400`; the batch fetch caps keep
skipping unknown ids silently. A fixture value the wire shape cannot carry
(an out-of-range L$ sale price) answers `500` rather than disguising a
server-data fault as a client error.

The inverses added for this cluster: `parse_fetch_inventory_request`,
`build_fetch_inventory_items_request` / `parse_fetch_inventory_items_request`
(`sl-wire/src/llsd.rs`), `parse_ais_create_link_body` with its
`AisLinkCreate` records and the `ais_update_to_llsd` tree form of
`build_ais_update_response` (`sl-wire/src/inventory.rs`), and
`fetch_inventory_items_to_llsd` / `ais_mutation_reply_to_llsd` /
`ais_category_children_reply_to_llsd` / `ais_item_reply_to_llsd`
(`sl-proto/src/session/conversions.rs`). The AIS3 URL/body codec both
directions already existed (Tier-F #61); this cluster wired it into the
dispatch, over the new tree.

### The region-information handlers

The region/object-information cluster serves nine caps from small
driver-populated stores on `SimSession` (the `display_names` stance): a
`SimulatorFeatures` document, an `LslSyntax` document, a per-parcel
environment map, three per-object maps (cost, physics, selection cost), a
region id + parcel-cover rectangle list, and the attachment/land resource
reports. One cap mutates: an `ExtEnvironment` PUT applies to the environment
store and surfaces `ServerEvent::EnvironmentUpdated` for a driver persisting
environments (which can then push `enqueue_windlight_refresh` so other
clients re-fetch).

- **`SimulatorFeatures`** (bodyless GET) serves the stored feature document
  via `build_simulator_features_response`. Its `lsl_syntax_id` is owned by
  `SimSession::set_lsl_syntax`, which stores the syntax document **and**
  advertises its id — the two caps can never drift apart (the client keys
  its `LSLSyntax` re-fetch on that id).
- **`LSLSyntax`** (bodyless GET) serves the stored language document via
  `build_lsl_syntax_document`, stamped with the schema version the client's
  parser insists on.
- **`ExtEnvironment`** routes on method. GET answers the stored
  `EnvironmentSettings` for the `?parcelid=` query (absent or `-1` = the
  region entry, seeded at construction; a parcel without its own entry
  inherits the region's) via `environment_to_llsd`. PUT parses the
  reference viewer's `coroUpdateEnvironment` body
  (`environment_update_from_llsd`, optional `?trackno=` scope), merges the
  `Some` fields wholesale (per-track splicing is deferred), bumps
  `env_version`, and echoes the stored result — the same
  `{ environment, success: true }` envelope the GET serves, which the
  client folds into `Event::Environment` unchanged. A `day_asset`-only
  update answers the reference's graceful failure,
  `200 { success: false, message }` — the fixture has no settings-asset
  store to resolve the id against. The reference's DELETE reset is out of
  scope (the task covers get/put) and answers `405`.
- **`RemoteParcelRequest`** (POST) resolves the requested region +
  location against the parcel-cover store (`SimParcel` rectangles,
  first-hit-wins): the request targets this region iff its non-nil region
  id or non-zero region handle matches the session's. A hit answers the
  parcel id (`build_remote_parcel_response`); a miss answers a `200`
  empty map — the "could not resolve" signal the client's `Ok(None)` fold
  reports as a decode failure rather than a typed event. (OpenSim instead
  synthesizes an id from handle + location so every lookup "succeeds";
  the store-driven miss is a deliberate divergence.)
- **`GetObjectCost` / `GetObjectPhysicsData`** (POST `{ object_ids }`)
  serve the stored per-object records in id order; unknown ids are
  omitted — the "no such object" signal, matching the batch-fetch
  tolerance stance.
- **`ResourceCostSelected`** (POST `selected_roots`/`selected_prims`)
  answers the component-wise **sum** of the stored per-object selection
  costs; unknown ids contribute zero, and the roots/prims kind validates
  the body without changing the arithmetic.
- **`AttachmentResources`** (bodyless GET) serves the stored
  agent-scoped report via `build_attachment_resources_response`.
- **`LandResources`** is the cluster's only two-stage cap: the
  `{ parcel_id }` POST (validated, but the stored reports are served
  as-is — their scope is the driver's choice) answers the
  `ScriptResourceSummary` / `ScriptResourceDetails` follow-up URLs,
  minted as the cap's own `summary` / `detail` sub-paths (the
  screenshot-uploader pattern — `resolve` routes on the token and the
  handler on the sub-path). The follow-up GETs serve the stored reports,
  which the client runtimes fold under the `LAND_RESOURCE_SUMMARY_TAG` /
  `LAND_RESOURCE_DETAIL_TAG` pseudo-cap names.

The status contract is the house standard — wrong method `405`, malformed
body or query (a non-integer `?parcelid=`/`?trackno=`) `400`, unknown
`LandResources` sub-path `404` — plus two deliberate soft failures: the EEP
`day_asset` case and the unresolved `RemoteParcelRequest` both answer `200`
with a body the client reads as "no result", never an HTTP error, because
that is what the reference stacks do.

The inverses added for this cluster: `parse_get_object_cost_request`
(`sl-wire/src/object_cost.rs`) and the net-new `ExtEnvironment` PUT pair —
`build_environment_update_request` / `environment_update_from_llsd` over the
new `EnvironmentUpdate` type (`sl-proto/src/session/conversions.rs`,
`sl-proto/src/types/environment.rs`) — driven client-side by the new
`Command::SetEnvironment` (both runtimes, repl command `set_environment`).
Everything else already existed client-paired
(`build_simulator_features_response`, `build_lsl_syntax_document`, the
`remote_parcel` / `object_cost` / `object_physics` / `resource_report`
codecs); this cluster wired them into dispatch over the new stores.

### The experience handlers

The experience cluster serves the twelve experience caps from one new
driver-populated fixture set, `SimExperiences`
(`sl-proto/src/sim_experiences.rs`, held as `SimSession::experiences[_mut]`):
metadata records keyed by public id, the agent's allowed/blocked
preference lists, the agent's owned/admin/creator id lists, per-group id
lists, and the region's allowed/blocked/trusted triple. Three caps
mutate — `ExperiencePreferences`, `UpdateExperience` and the
`RegionExperiences` POST — and their edits apply to the fixture (so
follow-up reads observe them), each surfacing a `ServerEvent`
(`ExperiencePermissionSet`, `ExperienceUpdated`, `RegionExperiencesSet`).

- **`GetExperienceInfo`** (GET, the `/id/?public_id=…` sub-path + query
  form) serves the stored record per requested id via
  `build_experience_infos_response`; unknown ids come back as `error_ids`
  entries, which the client folds into `missing` placeholders.
- **`FindExperienceByName`** (GET `?page=…&query=…`) answers a 1-based
  `SEARCH_PAGE_SIZE` page of records whose name contains the
  percent-decoded text case-insensitively, hiding invalid and
  `PROPERTY_PRIVATE` records (the grid's search lists public experiences
  only), sorted by name with an id tie-break.
- **`GetExperiences`** (bodyless GET) serves the agent's
  allowed/blocked lists via `build_experience_permissions_response`.
- **`ExperiencePreferences`** routes on method: PUT parses the
  `{ "<id>": { permission } }` body and applies `Allow`/`Block` (moving
  the id between the two lists); DELETE parses the `?<id>` query and
  forgets the preference. Both echo the full post-mutation lists — the
  same shape as `GetExperiences`, which is how the client folds them.
  Any id is accepted without a record lookup: a preference is the
  agent's own keyed entry (viewers block ids they never resolved).
- **`AgentExperiences` / `GetAdminExperiences` /
  `GetCreatorExperiences` / `GroupExperiences`** (bodyless GETs; the
  group form takes a bare `?<group_id>` query) are name-routed through
  one handler to the owned / admin / creator / per-group lists, each
  answered via `build_experience_ids_response`. An unknown group answers
  an empty list. The reply carries no group id, so the client runtimes
  parse it out-of-band and echo the queried id themselves.
- **`IsExperienceAdmin` / `IsExperienceContributor`** (GET
  `?experience_id=…`) answer `{ status }` from admin- / creator-list
  membership ("contributor" is the reference viewer's name for the
  creator list — it files `GetCreatorExperiences` under its Contributor
  tab). An unknown id answers `{ status: false }`, never an error; these
  replies are also parsed out-of-band by the runtimes.
- **`UpdateExperience`** (POST) applies the editable fields (name,
  description, maturity, properties, SLURL, extended metadata) to the
  stored record — owner, quota and expiration are server-controlled and
  untouched, matching the fields the reference viewer strips from the
  POST — and echoes the updated record in the wrapped
  `{ experience_keys }` form, whose first record the client folds into
  `Event::ExperienceUpdated`.
- **`RegionExperiences`** routes on method: GET serves the stored
  triple, POST parses the same-shaped `{ allowed, blocked, trusted }`
  body, replaces the lists wholesale and echoes the stored result, both
  via `build_region_experiences_response`.

The status contract is the house standard — wrong method `405`,
malformed body or query `400`, an `UpdateExperience` targeting an
unknown record `404` — plus two deliberate exceptions:
`GetExperienceInfo` with an empty or absent query answers `200` with no
records (the parser is lenient by design), and `ExperiencePreferences`
never 404s on an unknown id (see above).

No new codecs: the whole server-direction surface
(`parse_experience_info_query` … `parse_region_experiences_request`,
`build_experience_infos_response` … `build_experience_status_response`
in `sl-wire/src/experience/server.rs`) already existed inverse-paired
from the experience service-pairing task; this cluster wired it into
dispatch over the new fixture. The AIS-style `ais_suffix` helper
reconstructs the URL suffix those parsers consume (the `/id/` sub-path
and the bare-query forms both round-trip through it).

### The voice handlers

The voice cluster serves the three voice caps from a signalling **stub**,
`SimVoice` (`sl-proto/src/sim_voice.rs`, held as `SimSession::voice[_mut]`).
Its fixtures say which backends the region speaks — a `WebRtcStub` (the
ICE/DTLS identity every JSEP answer advertises; `Default` is a
deterministic loopback identity) and/or a Vivox `VoiceAccountInfo` — plus
a per-parcel `ParcelVoiceInfo` table with the agent's current parcel and
optional per-channel credentials for chat-session channels. Its live state
is the WebRTC connections the client provisioned (`VoiceConnection`:
channel, offer, minted answer, trickled ICE candidates, end-of-gathering
flag), keyed by the `viewer_session` the stub mints serially. There is
**no media plane**: nothing listens on the advertised candidate, so the
stub drives a client's signalling state machine end to end without ever
carrying audio.

- **`ProvisionVoiceAccountRequest`** (POST) routes on the request:
  `voice_server_type: "webrtc"` with a JSEP offer opens a connection —
  `channel_type: "local"` binds the spatial channel (with the optional
  `parcel_local_id`), `"multiagent"` binds a chat session's `channel`
  and must present its registered `credentials` — and answers
  `{ viewer_session, jsep: { type: "answer", sdp } }`, the SDP derived
  from the offer by `WebRtcStub::answer_for` (media sections mirrored,
  `a=setup:passive`, the offer's ICE/DTLS lines replaced by ours, our
  candidates inline plus `a=end-of-candidates`). `logout: true` tears the
  `viewer_session` down. `"vivox"` (or no server type) hands out the
  Vivox fixture. Every request surfaces `ServerEvent::VoiceProvisionRequested`
  with its `VoiceProvisionOutcome`.
- **`ParcelVoiceInfoRequest`** (POST, body ignored — the viewer sends
  `undef`) answers the agent's recorded parcel: its stored entry, or the
  empty-`channel_uri` "no voice here" form. A Second Life WebRTC
  `channel_uri` is a bare UUID (the region id for the estate-wide
  channel), which is why `ParcelVoiceInfo` / `VoiceChannelInfo` carry a
  `VoiceChannelUri` (`Uri(sip:…)` | `Id(uuid)`) rather than a URL.
  Surfaces `ServerEvent::ParcelVoiceInfoRequested`.
- **`VoiceSignalingRequest`** (POST) records the ICE trickle
  (`candidates` batch or `candidate.completed`) on its connection and
  acks with an undefined body — the viewer only looks at the status (a
  non-2xx restarts its voice session). Surfaces
  `ServerEvent::VoiceSignalingReceived` (with `known: false` for a
  session the stub never provisioned).

The status contract: wrong method `405`, malformed body `400`, an
unavailable backend / missing offer / unknown channel type `400`, a
logout or trickle for an unknown `viewer_session` `404`, and mismatched
channel credentials `401` — the code the reference viewer reports as
"channel locked" (`409` would be "channel full"; the stub has no capacity
limit).

Two protocol facts worth knowing: the server's own ICE candidates ride
**inside the JSEP answer** — the viewer has no inbound ICE-trickle path,
so there is no server→client signalling event to serve; and the backend
is advertised three ways, all of which the fake grid derives from the
stub (`SimVoice::advertised_server_type`): the login response's
`voice-config`, `SimulatorFeatures.VoiceServerType` (the field the viewer
picks its spatial voice module from), and the `RequiredVoiceVersion`
event-queue push on region entry.

No new wire codecs beyond two gaps the cluster closed:
`SimulatorFeatures.voice_server_type`, and the multi-agent form's
`channel` / `credentials` on `VoiceProvisionRequest`
(`VoiceProvisionRequest::webrtc_channel`). The parsers and builders
(`parse_provision_voice_account_request`, `parse_voice_signaling_request`,
`build_provision_voice_account_response`,
`build_parcel_voice_info_response` in `sl-wire/src/voice.rs`) already
existed inverse-paired from the voice service-pairing task.

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
> - The inventory serving fixture (`SimInventoryTree`) is
>   `sl-proto/src/sim_inventory.rs`, held twice on `SimSession`
>   (`agent_inventory[_mut]` / `library_inventory[_mut]`). The per-item
>   fetch caps' constants are `CAP_FETCH_INVENTORY_ITEM`
>   (`"FetchInventory2"`) and `CAP_FETCH_LIBRARY_ITEM` (`"FetchLib2"`) in
>   `sl-proto/src/session.rs`, requested alongside the descendents caps.
> - The region/object-information serving stores are inline `SimSession`
>   fields with `set_*` driver setters (`set_simulator_features`,
>   `set_lsl_syntax`, `set_environment`, `set_object_cost`,
>   `set_object_physics`, `set_selection_cost`, `set_region_id` +
>   `add_parcel` (`SimParcel`), `set_attachment_resources`,
>   `set_land_resource_summary`, `set_land_resource_details`) in
>   `sl-proto/src/sim_session.rs`. The client-side EEP **write** path is
>   `Command::SetEnvironment` → `build_environment_update_request` →
>   an `ExtEnvironment` PUT (`put_caps_llsd` / `run_put_caps_llsd`),
>   folded back through the ordinary `Event::Environment`.
> - The experience serving fixture (`SimExperiences`) is
>   `sl-proto/src/sim_experiences.rs`, held as
>   `SimSession::experiences[_mut]`; the three mutating caps go through
>   the session wrappers `set_experience_preference`,
>   `apply_experience_update` and `apply_region_experiences`, which push
>   the `ExperiencePermissionSet` / `ExperienceUpdated` /
>   `RegionExperiencesSet` server events.
> - The voice signalling stub (`SimVoice`, `WebRtcStub`,
>   `VoiceConnection`, `VoiceChannel`, `VoiceProvisionOutcome` /
>   `VoiceProvisionRefusal`) is `sl-proto/src/sim_voice.rs`, held as
>   `SimSession::voice[_mut]`; the caps go through the session wrappers
>   `provision_voice`, `record_voice_signaling` and `parcel_voice_info`,
>   which push the `VoiceProvisionRequested` / `VoiceSignalingReceived` /
>   `ParcelVoiceInfoRequested` server events.
