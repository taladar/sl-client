# sl-client roadmap — missing Second Life protocol features

This is a gap analysis of what a *full* Second Life / OpenSim client needs
versus what `sl-client` implements today. Today the workspace does login,
circuit setup, region handshake, keepalive, teleport, logout, and read-only
region/parcel/map *survey* data. Everything below is unimplemented at the
handler level.

**Ordering — incremental standalone value.** Items are ordered so that each one
delivers a client capability *usable on its own* given only that feature plus
everything above it. A feature that is only useful in combination with several
others (notably the world-rendering set: object scene graph, terrain, textures,
avatar appearance, PBR) is ranked by when that *combination* becomes viable, not
by its raw importance. So narrow but self-sufficient clients come first — a
text/chat client, a profile checker, an inventory/product-update bot, an IM or
group bot — and the world-rendering cluster, where value only compounds once its
members coexist, comes later.

All 483 LLUDP messages already exist as generated codec types
(`sl-wire/build.rs`), so most estimates cover
**session state + handler logic + wiring through both runtimes**, not wire
encoding. CAPS-delivered features additionally need new HTTP capability calls
(only `EventQueueGet` is used today).

**Story points** (relative effort, single number for core + both runtimes): 1
trivial · 2 small · 3 small-medium · 5 medium · 8 large · 13 very large · 21
epic. **Test** says whether the local `opensim.service` is enough.

| # | Feature | Pts | Standalone client it unlocks | Test |
|---|---------|-----|------------------------------|------|
| 1 ✅ | Local chat **(done)** | 3 | Text-only chat client / chat bot | Local OpenSim |
| 2 ✅ | Instant messaging **(done)** | 5 | IM bot, notifier, offer-handler | Local OpenSim |
| 3 ✅ | Agent movement & control **(done)** | 5 | Walking/flying/follow bot, autopilot | Local OpenSim (real physics engine) |
| 4 ✅ | Avatar profiles **(done)** | 3 | Profile / picks checker | Local OpenSim (profiles enabled) |
| 5 ✅ | Inventory **(done, UDP + CAPS)** | 8 | Inventory manager, product-update bot | Local OpenSim |
| 6 ✅ | Friends & presence **(done)** | 5 | Presence/online monitor | Local OpenSim (2 accounts) |
| 7 ✅ | Group support **(done)** | 8 | Group chat bot, roster tool | Local OpenSim (Groups V2 module) |
| 8 ✅ | Script dialogs & permissions **(done)** | 3 | Vendor/scripted-object interaction bot | Local OpenSim (scripted object) |
| 9 ✅ | Mute list **(done)** | 2 | Moderation helper | Local OpenSim (MuteList module) |
| 10 ✅ | Seamless teleport (child circuits) **(done)** | 8 | Roaming bot that keeps its session | Local OpenSim (multi-region) |
| 11 ✅ | Money / economy **(done)** | 5 | Balance monitor, tip/vendor bot | **Money module or SL grid** |
| 12 ✅ | Full world map **(done)** | 5 | Live map: agents, POIs, land-for-sale | Local OpenSim |
| 13 ✅ | Parcel management **(done)** | 5 | Land-management tool | Local OpenSim |
| 14 ✅ | Estate/region management **(done)** | 5 | Region admin/restart/ban bot | Local OpenSim (owner account) |
| 15 ✅ | Bandwidth throttle (`AgentThrottle`) | 2 | *(enabler for 16–25)* | Local OpenSim |
| 16 ✅ | Object/scene graph | 13 | Scene auditor, proximity bot | Local OpenSim |
| 17 ✅ | Object interaction & editing | 8 | Builder/rezzer, object mover | Local OpenSim |
| 18 ✅ | Terrain heightmaps (`LayerData`) | 8 | Ground geometry for a renderer | Local OpenSim |
| 19 ✅ | Asset & texture pipeline **(fetch)** | 13 | Asset fetch + textured rendering | Local OpenSim |
| 20 ✅ | Avatar appearance & wearables | 13 | Render avatars; outfit control | Local OpenSim |
| 21 ✅ | Animations | 5 | Dance/gesture bot; animate scene | Local OpenSim |
| 22 ✅ | Sound | 3 | Spatial audio playback | Local OpenSim |
| 23 ✅ | Asset/texture/mesh upload | 5 | Content uploader | Local OpenSim |
| 24 ✅ | Media-on-a-prim / parcel audio | 5 | Media surfaces, streaming audio | Local OpenSim (external stream) |
| 25 ✅ | PBR materials / GLTF | 8 | Modern materials in a renderer | **Recent SL grid; OpenSim varies** |
| 26 ✅ | Voice chat **(signalling)** | 13 | Voice-enabled client | **SL Vivox/WebRTC or FreeSWITCH** |
| 27 ✅ | Experiences | 5 | Experience-permission client | **SL grid only** |
| 28 ✅ | Complete the IM surface | 8 | Offer-handler / IM bot (full) | Local OpenSim (2 accts) |
| 29 ✅ | Profile & pick/classified editing | 5 | Profile editor | Local OpenSim (profiles) |
| 30 ✅ | Inventory mutation & AIS3 | 8 | Inventory manager, product bot | Local OpenSim |
| 31 ✅ | Group management edits | 5 | Group admin bot | Local OpenSim (Groups V2) |
| 32 ✅ | Camera & interest control | 3 | Look-aware roaming bot | Local OpenSim |
| 33 ✅ | World-stream decode & LOD fetch | 5 | *(faithfulness for 16/19)* | Local OpenSim |
| 34 ⛔ | Experience key-value store | 3 | *(out of scope — no client cap)* | **n/a** |

**Every client protocol feature in this roadmap is now implemented (#1–#33).**
The deferred follow-ups #28–#33 — the bits items #1–#27 knowingly left for later
(the "Deferred:" / "follow-up" / "waits on #…" / "unit-tested only" notes in
those entries), promoted to first-class roadmap items so the gap analysis stays
complete — are done. Each was grouped under and extends an earlier item; their
full prose is in **"Planned — deferred follow-ups"** below the done items.

**Item #34 turned out not to be a client protocol feature and is reclassified
out of scope (⛔).** The experience key-value store has **no viewer/client
capability**: the reference viewer (Firestorm) requests all 13 experience caps
(`GetExperienceInfo`, `GetExperiences`, `RegionExperiences`, …) — every one of
which #27 already implements — but no KV cap; the SL wiki's authoritative
"Current Sim Capabilities" list contains no `ExperienceKeyValue` (or any
`KeyValue`) capability; and `llReadKeyValue`/`llCreateKeyValue`/… appear in the
viewer source only as *script-editor keywords*, never as cap calls. The KV store
is reached **only by in-world LSL scripts** running server-side; the simulator
talks to an internal Linden datastore over a service-to-service path never
exposed to clients. There is therefore nothing for a client to wire — it joins
the other out-of-scope items (J2C/glTF/mesh decode, rendering, the voice audio
transport). See the closing "Out of scope" note.

**Decode-fidelity audit (2026-06-18) — #35–#51, Tier E.** A pass over the
inbound decode path (the struct→`Event` translation in `sl-proto`'s
`session.rs`, the hand-written binary-blob decoders, and the CAPS/LLSD decoders)
found *information loss*: fields that are present on the wire and fully decoded,
but then dropped before reaching the library user — usually because the
user-facing type has no field to hold them. These are **not** missing features;
each is a faithfulness gap in an already-shipped item (#1–#33). They are
collected as a new **Tier E** below, ordered by severity, and do not change the
"client protocol surface is complete" claim above — they make the *data* that
surface already carries reach the caller. (Wire encoding is unaffected; the
generated codec already decodes every field — the loss is purely in what
`session.rs` forwards.)

## Tier A — self-sufficient interactive clients (text & bot viewers)

Each works as a complete, useful client on top of today's connection layer.

**1. Local chat — `ChatFromViewer` (send), `ChatFromSimulator` (receive) · 3
pts. ✅ Done.** Smallest step to a genuinely interactive client: a text-only
viewer or chat bot. Implemented: `Session::say` (whisper/normal/shout, any
channel) and `Session::set_typing`, with `Event::ChatReceived` (speaker, ids,
type, audibility, region-local position, text) and a distinct
`Event::ChatTyping` for typing start/stop. Wired as
`Command::Chat`/`Command::Typing` through both the tokio and bevy runtimes;
verified live against the local OpenSim (full send→rebroadcast→receive
round-trip). *Test: local OpenSim.*

**2. Instant messaging — `ImprovedInstantMessage` · 5 pts. ✅ Done.**
Implemented: `Session::send_instant_message` (1:1, with the canonical
`agent_id XOR target` session id) and `Session::send_im_typing`, plus an
`ImDialog` enum classifying the dialog sub-types multiplexed over this message
(inventory offers, teleport offers/lures, group invites, friendship offers,
object IMs, group/conference messages, …). Incoming IMs surface as
`Event::InstantMessageReceived` (a full `InstantMessage`: sender, dialog, ids,
region/position, offline flag, message, binary bucket) with typing split out as
a distinct `Event::ImTyping`. Wired as
`Command::InstantMessage`/`Command::ImTyping` through both runtimes; verified
live against the local OpenSim (self-IM round-trip for both message and typing).
Incoming offline IMs that the sim pushes as `ImprovedInstantMessage` are already
surfaced (with `offline = true`). Deferred follow-ups: (a) offer accept/decline
reply flows (inventory/teleport/friendship); (b) offline-IM *history retrieval*
— the legacy `RetrieveInstantMessages` UDP trigger and the modern SL
`ReadOfflineMsgs` CAPS path (needs the grid's offline module enabled, and an
offline-then-relogin test); (c) sending into group/conference sessions and the
session start/invite/leave dialogs (`IM_SESSION_*`), which belong with #7 (group
support). *Test: local OpenSim (single account suffices via a self-IM;
cross-avatar needs two).*

**3. Agent movement & control · 5 pts. ✅ Done.** Promoted the stubbed
`AgentUpdate` into a real control surface. Implemented: a `ControlFlags`
bitfield (walk/run/fly/turn/jump/up/down/…); `Session::set_controls` and
`Session::set_rotation` (persisted and re-sent on every keep-alive, so the sim
keeps moving the agent); one-shot `Session::stand` and `Session::sit_on_ground`;
`Session::sit_on` (the `AgentRequestSit` → `AvatarSitResponse` → `AgentSit`
handshake, surfaced as `Event::SitResult`); and `Session::autopilot_to`
(server-side walk-to-coordinates via a `GenericMessage` `autopilot`, so a bot
can navigate without any scene knowledge). Wired as
`Command::{SetControls, SetRotation, Stand, SitOnGround, Sit, Autopilot}`
through both runtimes; verified live (the avatar walked +14.5 m forward under
`AT_POS`). Camera stays at region centre — true camera control waits on position
tracking from the object/scene graph (#16). *Test: local OpenSim — needs a real
physics engine (ubODE/BulletSim); the default BasicPhysics does not move
avatars.*

**4. Avatar profiles — `AvatarPropertiesRequest`/`Reply` + `GenericMessage`
picks/notes · 3 pts. ✅ Done.** A standalone profile/picks checker. Implemented:
`Session::request_avatar_properties` (UDP `AvatarPropertiesRequest`, answered by
`AvatarPropertiesReply` + `AvatarInterestsReply` + `AvatarGroupsReply`) plus
`request_avatar_picks` and `request_avatar_notes` (the `GenericMessage`
`avatarpicksrequest` / `avatarnotesrequest` calls OpenSim expects). Surfaced as
`Event::{AvatarProperties, AvatarInterests, AvatarGroups, AvatarPicks, AvatarNotes}`
with value types (`AvatarProperties`, `AvatarInterests`,
`AvatarGroupMembership`, `AvatarPick`). Wired as
`Command::RequestAvatar{Properties, Picks,Notes}` through both runtimes;
verified live (own-profile round-trip returned born date, flags, about text, and
interests). Profile *editing* (`AvatarPropertiesUpdate`, pick/classified
create-update-delete) and pick/classified *detail* fetches are follow-ups.
*Test: local OpenSim — needs the profile module enabled (set `[UserProfiles]
ProfileServiceURL`); otherwise no reply is sent.*

**5. Inventory — login skeleton + UDP and HTTP-CAPS fetch · 8 pts. ✅ Done (UDP

- CAPS).** Fetch the folder/item tree. Implemented: the login request asks for
`inventory-root` + `inventory-skeleton`, and the response parser extracts the
root folder id and the full folder skeleton (every folder's
id/parent/name/type/version), surfaced as `Event::InventorySkeleton` +
`Session::inventory_root()`. Folder *contents* are available over **both**
transports, both surfaced as `Event::InventoryDescendents` with
`InventoryFolder`
- `InventoryItem` value types (full permissions, asset id, types, sale info):
- **UDP** — `Session::request_folder_contents` (`FetchInventoryDescendents` →
  `InventoryDescendents`), wired as `Command::RequestFolderContents`. Simple,
  one folder per call; OpenSim splits the reply across packets.
- **HTTP CAPS** — `Command::FetchInventoryFolders` (batch), the modern path used
  on Second Life. The capability map is now a first-class runtime concept: each
  runtime fetches the seed once per region (requesting
  `REQUESTED_CAPABILITIES`), caches the `cap → URL` map, drives the
  `EventQueueGet` long-poll off it, and POSTs `FetchInventoryDescendents2` for
  inventory; the LLSD response is decoded by `Session::handle_caps_event` into
  the same event. The capability-map caching refactor also sets up future CAPS
  calls (textures, mesh, AIS3, …).

Verified live against the local OpenSim on both paths (20-folder skeleton; root
fetch returning all 17 system sub-folders — three UDP packets vs one CAPS
response). Deferred: AIS3 (`InventoryAPIv3`) REST semantics, and inventory
*mutation* (`BulkUpdateInventory`/`UpdateInventoryItem` watching,
move/copy/delete/create). Prerequisite for appearance (Current Outfit Folder,

## 20) and giving items over IM (#2). *Test: local OpenSim (both paths

`Cap_FetchInventoryDescendents2` is enabled by default).*

**6. Friends & presence · 5 pts. ✅ Done.** A standalone presence/online
monitor. Implemented: the **friend list arrives at login** — the request now
asks for `buddy-list`, and the response parser extracts each friend's id plus
the two rights bitfields (`BuddyListEntry`), surfaced once as
`Event::FriendList(Vec<Friend>)` right after `CircuitEstablished`. **Presence**
is sim-pushed: `OnlineNotification` /`OfflineNotification` surface as
`Event::FriendsOnline`/`FriendsOffline` (`Vec<Uuid>`). **Rights**:
`Session::grant_user_rights` (`GrantUserRights`) sets the rights granted to a
friend, and incoming `ChangeUserRights` surfaces as
`Event::FriendRightsChanged { friend_id, rights, granted_to_us }` — the
`granted_to_us` flag distinguishes a friend changing their grant to us from the
sim echoing our own change (OpenSim's `AgentData.AgentID == self` hack).
**Friendship offer/accept via IM**: `Session::send_friendship_offer`
(`ImprovedInstantMessage` `IM_FRIENDSHIP_OFFERED`), plus
`accept_friendship`/`decline_friendship`
(`AcceptFriendship`/`DeclineFriendship`, echoing the offer IM's `id` as the
transaction id) and `terminate_friendship` (`TerminateFriendship`). A
`FriendRights` bitfield value type wraps the rights flags
(`CAN_SEE_ONLINE`/`CAN_SEE_ON_MAP`/`CAN_MODIFY_OBJECTS`). Wired as
`Command::{OfferFriendship, GrantUserRights, TerminateFriendship,
AcceptFriendship, DeclineFriendship}` through both runtimes; verified live
against the local OpenSim with two accounts
(offer→accept round-trip, friend list at re-login, and online/offline
notifications). *Test: local OpenSim with two accounts.*

**7. Group support · 8 pts. ✅ Done.** A group chat relay / roster tool.
Implemented across the full UDP surface: **membership & active group** —
`AgentDataUpdate` → `Event::ActiveGroupChanged` (active group, title, powers)
and `AgentGroupDataUpdate` → `Event::GroupMemberships`, with
`Session::activate_group` (`ActivateGroup`). **Roster/roles/profile** —
`request_group_members`, `request_group_roles`, `request_group_role_members`,
`request_group_titles`, `request_group_profile`,
`request_group_notices`/`request_group_notice` (the
`GroupMembersReply`/`GroupRoleDataReply`/`GroupRoleMembersReply`/
`GroupTitlesReply`/`GroupProfileReply`/`GroupNoticesListReply` round-trips →
`Event::Group{Members,RoleData,RoleMembers,Titles,ProfileReceived,Notices}`).
**Group IM sessions** — `start_group_session`/`send_group_message`/
`leave_group_session` over `ImprovedInstantMessage` (session id = group id;
`IM_SESSION_GROUP_START`/`SEND`/`LEAVE`), with incoming group chat surfaced as
`Event::GroupSessionMessage` and join/leave as `Event::GroupSessionParticipant`
(new `ImDialog` session variants 13–18). **Group management** — `create_group`
(`CreateGroupParams`), `join_group`, `leave_group`, `invite_to_group`,
`set_group_accept_notices`, `set_group_contribution`, plus
`Event::{CreateGroupResult, JoinGroupResult, LeaveGroupResult, DroppedFromGroup}`.
All wired as `Command`/`SlCommand` variants through both runtimes. Built on #2's
IM multiplexing. Verified live against the local OpenSim (Groups V2) with two
accounts: create group → fetch profile/roster → second avatar joins (open
enrollment) → roster shows both → group-chat message round-trips between them.
Also implemented the **CAPS group APIs** (the modern Second Life path): the
event-queue `AgentGroupDataUpdate` (memberships; the UDP one is `UDPDeprecated`)
→ `Event::GroupMemberships`, and the `GroupMemberData` capability POST
(`Command::FetchGroupMembers`, hex-powers / titles-by-index LLSD) →
`Event::GroupMembers` — both decoded by `Session::handle_caps_event`, wired
through both runtimes' cap-POST machinery, and covered by `parse_llsd_xml` →
`handle_caps_event` tests (SL-only, so not live-verified on OpenSim, whose UDP
path is the testable one). Deferred follow-ups: group-notice *creation*, role
create/delete and member-role assignment edits, and ejecting members. *Test:
local OpenSim with the Groups V2 module enabled (needs a MySQL/MariaDB ≤10.x
backend; OpenSim's bundled connector can't talk to MariaDB 12).*

**8. Script dialogs & permissions · 3 pts. ✅ Done.** A vendor / scripted-object
interaction bot. Incoming scripted prompts surface as events:
`Event::ScriptDialog` (`llDialog`/`llTextBox` — object, owner, message, buttons,
hidden chat channel; `ScriptDialog::is_text_box` detects the `llTextBox` magic
button), `Event::ScriptPermissionRequest` (`llRequestPermissions`, with a
`ScriptPermissions` bitfield mirroring the LSL `PERMISSION_*` constants),
`Event::LoadUrl` (`llLoadURL`), and `Event::ScriptTeleport`
(`ScriptTeleportRequest`/`llMapDestination`). Replies:
`Session::reply_script_dialog` (`ScriptDialogReply` — chosen button on the
dialog's channel, also used to return `llTextBox` text) and
`Session::answer_script_permissions` (`ScriptAnswerYes` — grant a subset, or
`ScriptPermissions::default` to deny). Wired as
`Command::{ReplyScriptDialog, AnswerScriptPermissions}` through both runtimes.
Verified live against the local OpenSim (XEngine/YEngine enabled): an OAR-loaded
scripted prim fired `llDialog`, `llRequestPermissions(PERMISSION_DEBIT)` and
`llLoadURL` at the test avatar, which received all three events and replied to
the dialog. *Test: local OpenSim with the script engine enabled and a scripted
object (no headless rez path — a scripted prim must be loaded via an OAR or a
viewer).*

**9. Mute list · 2 pts. ✅ Done.** A small moderation helper that fetches and
edits the mute/block list. Implemented: `Session::request_mute_list`
(`MuteListRequest`, zero CRC), `mute` (`UpdateMuteListEntry`) and `unmute`
(`RemoveMuteListEntry`), with `MuteType` (by-name/agent/object/group/external)
and a `MuteFlags` exception bitfield. The **fetch** is the real thing: the sim
replies with `UseCachedMuteList` (→ `Event::MuteListUnchanged`), a
`GenericMessage` `emptymutelist` (→ `Event::MuteList([])`), or a
`MuteListUpdate` naming a file the client then
**downloads over the legacy `Xfer` file-transfer path** (`RequestXfer` →
`SendXferPacket`/`ConfirmXferPacket`, stripping packet-0's 4-byte length prefix
and detecting the `0x80000000` last-packet flag), parsing the
`<type> <uuid> <name>|<flags>` lines into `MuteEntry` values
(`Event::MuteList`). The `Xfer` machinery (session state for in-flight
transfers) is reusable for #19's legacy asset path. Wired as
`Command::{RequestMuteList, Mute, Unmute}` through both runtimes. Verified live
against the local OpenSim (MuteList module + SQLite MuteListService enabled):
muting an agent then fetching returned the parsed entry over Xfer, and unmuting
then fetching returned an empty list. *Test: local OpenSim with `[Messaging]
MuteListModule = MuteListModule` and a `MuteListService` (SQLite) configured.*

**10. ✅ Seamless teleport via child-agent circuits — `EnableSimulator` → child
`UseCircuitCode`, `EstablishAgentCommunication` (CAPS), `CrossedRegion`,
`TeleportFinish` handover · 8 pts. (done)** Not a new surface but a quality
upgrade that *adds value to the Tier-A clients*: replaced the re-login
workaround with real child→root handover so a roaming bot keeps one continuous
session (open IMs, group sessions, agent state) across teleports and region
crossings. `Session` now holds a root circuit plus a `BTreeMap` of child-agent
circuits keyed by simulator address; neighbours are opened with a child
`UseCircuitCode` (no `CompleteAgentMovement`) so they hold the agent's presence
*before* a crossing, and a crossing promotes the pre-opened child to root
(swapping the old root back down to a child — shared neighbours are **not**
dropped, so the general any-side topology keeps its circuits). Datagrams are
routed per-circuit by source address; both runtimes already multiplex circuits
over one socket via `Transmit.destination` / `recv_from`, so neither needed
changes. **Key live finding:** OpenSim (and SL) deliver `EnableSimulator`,
`EstablishAgentCommunication` **and** `CrossedRegion` over the **CAPS event
queue**, not UDP — and the CAPS `Port`/`SimPort` is a plain integer (no
byte-swap, unlike the UDP `IPPORT`). Both UDP and CAPS paths are handled.
*Live-verified: a bot flew east across the Default→East border on one
continuous login (3 neighbours enabled, `RegionChanged` to the East sim, no
re-login) against the local 2×2 multi-region OpenSim.*

### Tier B — extensions of the existing survey/map strengths

These build directly on the read-only data the client already collects, each a
usable standalone tool.

**11. Money / economy (done) ✅ — `MoneyBalanceRequest`/`Reply`,
`MoneyTransferRequest`, `EconomyData`/`Request` · 5 pts.** L$ balance and
transfers — a balance monitor or tip/vendor bot (stronger combined with #2/#8,
but a balance/transfer tool stands alone). `Session::request_money_balance` /
`request_economy_data` / `send_money_transfer` (the latter taking a
`MoneyTransactionType` — `Gift`, `PayObject`, `ObjectSale`, or `Other(i32)`);
replies surface as `Event::MoneyBalance` (balance as `sl_types::LindenAmount`,
plus the optional `TransactionInfo` as `MoneyTransaction` when the reply
describes a real payment) and `Event::EconomyData` (upload/claim/group prices,
region object capacity). Wired through both runtimes
(`Command::RequestMoneyBalance` / `RequestEconomyData` / `SendMoneyTransfer`).
*Live-verified against local OpenSim with `economymodule =
BetaGridLikeMoneyModule`: a `MoneyBalanceReply` (balance 0 L$, success) and
`EconomyData` (the configured upload/group-create prices) both round-tripped on
one login. Stock OpenSim's module hardcodes a 0 balance and does not route real
transfers, so the transfer path is unit-tested only; full transfers need a money
backend (Gloebit/DTL) or the real SL grid.*

**12. Full world map (done) ✅ — `MapItemRequest`
(agents/telehubs/events/land-for-sale), `MapNameRequest`, `MapBlockRequest` by
name · 5 pts.** Extends the existing `MapBlockRequest`/`MapBlockReply` to a
complete map: avatar dots, POIs, search-by-name — a live map tool on its own.
`Session::request_map_by_name` (search regions by name → `Event::MapBlock`, the
same reply as a block request) and
`request_map_items(MapItemType, region_handle)` →
`Event::MapItems { item_type, items }`, where `MapItemType` covers
`Telehub`/`AgentLocations`/`LandForSale`/the event types/`Other(u32)` and each
`MapItem` carries global coordinates (with `region_handle()`/`local_x()`/
`local_y()` helpers), an id, the type-specific `extra`/`extra2`, and a name.
Both map requests send the viewer's map-layer flag (`LAYER_FLAG = 2`). Wired
through both runtimes (`Command::RequestMapByName` / `RequestMapItems`).
*Live-verified against local OpenSim with two avatars: `MapNameRequest("East
Region")` resolved the neighbour by name, and an `AgentLocations` request
returned the second avatar's map dot at the right global coordinates. Stock
OpenSim answers agent-locations, telehubs and land-for-sale locally; events and
classifieds are not implemented server-side.*

**13. Parcel management (done) ✅ — `ParcelPropertiesUpdate`,
`ParcelAccessListRequest`/`Reply`/`Update`, `ParcelDwellRequest`/`Reply`,
`ParcelBuy`, `ParcelReturnObjects`, `ParcelSelectObjects`, plus
`ParcelDeedToGroup`/`Reclaim`/`Release` · 5 pts.** Turns the existing parcel
read path into a land-management tool. `Session` gains
`update_parcel(&ParcelUpdate)` (a builder-style struct — flags, name/desc,
category, sale price, group, media, landing point),
`request_parcel_access_list`/`update_parcel_access_list` (allow/ban lists via
`ParcelAccessScope`, surfaced as `Event::ParcelAccessList`),
`request_parcel_dwell` (→ `Event::ParcelDwell`), `buy_parcel`,
`return_parcel_objects`/`select_parcel_objects` (`ParcelReturnType` bitfield),
`deed_parcel_to_group`, `reclaim_parcel`, `release_parcel`. Wired through both
runtimes. Added a `ParcelFlags::union` helper for combining flags. **Fixed a
pre-existing CAPS read bug:** OpenSim encodes the `uint` `ParcelFlags` as a
4-byte binary LLSD element, which the old `as_i32` parse dropped to `0` — now
read via a tolerant `llsd_u32` (binary/integer/string). **Read-side
stream/media URLs (follow-up):** `ParcelInfo` now also surfaces the parcel's
streaming-audio URL (`music_url`), media URL (`media_url`), `media_id` and
`media_auto_scale` — the `ParcelProperties` message and CAPS event already
carried them, but the old `parcel_info`/`parcel_info_from_llsd` builders dropped
them, so a client could *set* a parcel's stream URL (via `ParcelUpdate`) but not
*read* the current one. Both decode paths covered by tests (the UDP wire form
incl. NUL-trimming, and the CAPS LLSD form incl. OpenSim's boolean
`MediaAutoScale`); the CAPS LLSD keys (`MusicURL`/`MediaURL`/`MediaID`/
`MediaAutoScale`) were cross-checked against OpenSim's
`LLClientView.cs` encoder. (The *per-face* media-on-a-prim system and parcel
media *control* — `ObjectMedia`/`ObjectMediaNavigate`,
`ParcelMediaCommandMessage` — remain roadmap #24.) *Live-verified against
local OpenSim logged in as the estate owner: dwell read, access-list read +
write + re-read round-trip, and a `ParcelPropertiesUpdate` that changed the
parcel name and flags (confirmed via the console and across logins; OpenSim
serves an explicit in-session ParcelProperties re-request from a cached
snapshot, so flag edits show on the next fetch). Most write ops need parcel
ownership / estate powers — see the estate-owner login.*

**14. Estate/region management (done) ✅ — `EstateOwnerMessage`
(kick/ban/restart/teleport-home/manage), `GodlikeMessage` · 5 pts.**
Region/estate admin for owners — a restart/ban/management bot. `Session` gains
`request_estate_info` (`getinfo` → `Event::EstateInfo` +
`Event::EstateAccessList` per category),
`update_estate_access(EstateAccessDelta, target)` (ban/manager/allowed
agent/group add+remove via `estateaccessdelta`), `kick_estate_user`,
`teleport_home_user`/`teleport_home_all_users`, `restart_region` (`-1` delays),
`send_estate_message` (estate-wide blue box),
`set_region_info(&RegionInfoUpdate)`
(maturity/agent-limit/object-bonus/toggles), plus the god-level `god_kick_user`
(`GodKickUser`) and a generic `send_godlike_message`. Incoming
`EstateOwnerMessage` `estateupdateinfo`/`setaccess` replies are parsed (the
access-list UUIDs are raw 16-byte params, one category per message). Added
`Maturity::to_sim_access`. Wired through both runtimes. *Live-verified logged in
as the estate owner: `getinfo` returned the estate config ("My Estate", id 101,
flags) and all four access lists, and a ban add + re-`getinfo` round-trip showed
the banned agent (then removed it). The kick/teleport-home/restart/setregioninfo
and god ops are unit-tested only (disruptive or god-gated to live-test).*

### Tier C — world-rendering cluster (value compounds across the group)

Individually these do little; together they let the bevy crate render and
interact with the actual world. Do them as a set, in this order. **#15 first** —
it is the bandwidth prerequisite for the bulk UDP streams that the rest depend
on.

**15. Bandwidth throttle (done) ✅ — `AgentThrottle` · 2 pts.** Tell the sim how
to allocate the seven throttle categories
(resend/land/wind/cloud/task/texture/asset); without it the sim's conservative
defaults starve the object/terrain/texture firehose the rest of this tier needs.
Implemented: a `Throttle` value type holding the seven per-category rates in
kilobits per second (with `preset_300`/`preset_500`/`preset_1000` presets
mirroring the reference viewer's bandwidth tables, a `total`, and the wire
`bits_per_second` conversion), and `Session::set_throttle`, which packs the
rates as seven little-endian `f32` bits-per-second values into the
`AgentThrottle` `Throttles` byte array (`GenCounter` 0, as the viewer does) and
sends it reliably on the root circuit. The throttle is **remembered and re-sent
automatically on every region change** (each new root region starts with the
sim's defaults until re-told) — the re-send is funnelled through
`complete_arrival`, the single point where a new root region becomes active
(login *and* handover). Wired through both runtimes
(`Command::SetThrottle(Throttle)`). *Live-checked against the local OpenSim: the
example advertises `Throttle::preset_1000` at handshake and the session runs a
full clean lifecycle (login → throttle sent reliably → neighbours enabled →
clean logout) with no protocol error; the exact 28-byte wire payload, the
agent/session/circuit fields, and the re-send-on-region-change are covered by
unit tests. (`AgentThrottle` has no reply, and OpenSim's `debug lludp throttles`
console commands aren't dispatchable over the REST console in this build, so the
applied rate can't be read back live.)*

**16. Object/scene graph (done) ✅ — `ObjectUpdate`, `ObjectUpdateCompressed`,
`ObjectUpdateCached`, `ImprovedTerseObjectUpdate`, `KillObject`,
`ObjectProperties`, `RequestMultipleObjects` · 13 pts.** The largest single
piece — "seeing the world." Implemented an object cache keyed by source
simulator then region-local id (local ids are only unique within a sim), so the
current region *and* every neighbouring region streamed over a child circuit are
cached side by side; a sim's objects are dropped when its circuit goes away
(`DisableSimulator`, teleport handover, relogin, inactivity). All four update
decoders and an `ObjectAdded`/`Updated`/`Removed`/`Properties` event stream. New
value types `Object` (identity, parent, pcode, scale, `ObjectMotion`, owner,
sound, floating text, name-values, media URL, raw texture-entry/extra-params,
optional merged `ObjectProperties`), `ObjectMotion`, `ObjectProperties`, and a
`pcode` constants module. The three packed blobs are decoded by hand from the
generated `Vec<u8>` fields: **full** `ObjectUpdate` (60/76-byte motion blob:
pos/vel/acc full-f32 + packed-quat rotation + angvel, with the avatar
collision-plane prefix), **terse** `ImprovedTerseObjectUpdate` (local id + state

- full-f32 position + 16-bit quantized velocity ±128 / acceleration ±64 /
rotation ±1 (4 explicit comps) / angular velocity ±64, with LL's `U16_to_F32`
snap-to-zero), and **compressed** `ObjectUpdateCompressed` (the
`CompressedFlags` bitfield gating angvel / parent / tree / scratchpad /
floating-text / media-url; the reliable fixed prefix + text/media-url are
decoded, the trailing length-prefix-less particle/extra-param/ shape/texture
fields are left raw). Cache-miss handling: `ObjectUpdateCached` entries and
terse updates for unknown ids trigger a `RequestMultipleObjects` (full) fetch;
`KillObject` removes and emits `ObjectRemoved`; `ObjectProperties` (from
selecting via `ObjectSelect`) surfaces and merges into the cached object.
**Neighbour-region streaming:** object messages are handled on the child
circuits too (not just the root), keyed per sim; child circuits are driven with
the bandwidth throttle *and* periodic `AgentUpdate`s (camera/interest). The key
piece is the **per-neighbour seed-capability POST**: OpenSim gates a region's
entire initial scene push (`ScenePresence.SendInitialData`, which sends objects
to child agents too) on `Caps.CapsFlags.SentSeeds` — i.e. the viewer must POST
that region's seed cap. So `EstablishAgentCommunication` now surfaces an
`Event::NeighborSeed { sim, seed_capability }` and both runtimes POST it (the
same seed request the root does), which unlocks the neighbour's object stream
onto the child circuit. A sim's objects are dropped (with `ObjectRemoved`) when
its circuit goes away. Public API: `Session::objects()` (all regions) /
`objects_in_region(handle)` / `object(local_id)` (current region),
`request_objects`, `request_object_properties` (select) / `deselect_objects`,
wired as
`Command`/`SlCommand::{RequestObjects, RequestObjectProperties, DeselectObjects}`
through both runtimes. Decoders + neighbour streaming covered by seven
`sl-proto` lifecycle tests (full/terse/cached/compressed/kill/properties + a
child-circuit neighbour test). *Live-verified against the local 2×2 OpenSim:
logged into Default with an OAR-loaded prim in the East neighbour, the client
received the avatar (pcode 47, collision-plane variant) in Default **and the
East prim (pcode 9) under East's region handle** over the child circuit —
confirming end-to-end neighbour streaming. Also verified the in-region full
`ObjectUpdate` decode and an `ObjectSelect` → `ObjectProperties` round-trip
(name + owner, merged into the cache). Stock OpenSim sends full `ObjectUpdate`s,
so the compressed/cached/terse- miss decoders (heavier on the SL grid) are
unit-tested only.* Even before a renderer this enables a scene auditor or
proximity bot; its full payoff needs #18–#20. *Test: local OpenSim — rez prims
via console/viewer (or load an OAR) to populate the scene; load into a neighbour
to exercise child-circuit streaming.*

**17. Object interaction & editing (done) ✅ — `ObjectGrab`/`ObjectGrabUpdate`/
`ObjectDeGrab` (touch/click), `ObjectAdd` (rez), `ObjectDuplicate`,
`ObjectDelete`/`DeRezObject`, `MultipleObjectUpdate` (move/scale/rotate),
`ObjectName`/`ObjectDescription`/`ObjectFlagUpdate`, plus the single-field edit
messages · 8 pts.** Turns the read-only scene (#16) into an editable one — a
builder/rezzer or object mover, building on #16's object cache (an object is
named by its region-local id). Implemented across the full editing surface:
**touch/click** — `touch_object` (an `ObjectGrab` + immediate `ObjectDeGrab`,
which fires a script's `touch_start`/`touch_end` and the `CLICK_ACTION_*`
behaviours) plus the press-drag-release primitives `grab_object`,
`grab_object_update`, `degrab_object`; **rez** — `rez_object(&PrimShape, …)`
(`ObjectAdd`), with a `PrimShape::cube` constructor carrying the viewer's
default new-prim path/profile quantization (the prim is rezzed exactly at its
position via `BypassRaycast`); **copy/delete** — `duplicate_objects`
(`ObjectDuplicate`), `delete_objects` (`ObjectDelete`, to trash), and
`derez_objects` (`DeRezObject`
with a `DeRezDestination` — take/return/trash/attach/…); **transform** —
`update_object(&ObjectTransform)` (`MultipleObjectUpdate`) plus the convenience
`set_object_position`/`set_object_rotation`/`set_object_scale`, which hand-pack
the variable `Data` blob in the simulator's fixed position→rotation→scale order
(the rotation via LL's `packToVector3` three-float quaternion) and OR the
`Type` bits (position `0x01`, rotation `0x02`, scale `0x04`, link-set `0x08`,
uniform `0x10`); **metadata** — `set_object_name`/`set_object_description`,
`set_object_click_action` (a `ClickAction` enum), `set_object_material` (a
`Material` enum), `set_object_flags` (`ObjectFlagUpdate`: physics/temporary/
phantom), `set_object_group`, `set_object_permissions` (a `PermissionField` mask
selector with set/clear), `set_object_for_sale` (a `SaleType` enum),
`set_object_category`, `set_object_include_in_search`; and **linking** —
`link_objects` (root id first) / `delink_objects`. New value types `PrimShape`,
`ObjectTransform`, `ObjectFlagSettings`, `ClickAction`, `Material`, `SaleType`,
`DeRezDestination`, `PermissionField`. All wired as `Command`/`SlCommand`
variants through both runtimes. Covered by ten `sl-proto` encoding tests (the
`ObjectAdd` cube fields, the `MultipleObjectUpdate` position+rotation `Data`
packing and the scale/uniform/group `Type` byte, touch grab+degrab, name,
delete, derez, permissions, link order, and the single-field setters).
*Live-verified against local OpenSim as the estate owner (the
`rez_edit_object` tokio example): `rez_object` created a cube, which streamed
back as an `ObjectAdded`; `set_object_name` + `set_object_for_sale` were
confirmed by the `ObjectProperties` round-trip (name, sale type Copy, price);
`update_object` moved it +5 m (confirmed by the follow-up `ObjectUpdate`); and a
`DeRezObject` to the Trash folder removed it (`ObjectRemoved`). The
touch/grab/material/click-action/flags/permissions/link ops are unit-tested
only (they need a scripted object or a second observer to see live). **Note:**
`ObjectDelete` is the viewer's god/force-delete path and stock OpenSim has no
handler for it ("Unhandled packet … Ignoring"); the portable delete-to-trash is
`derez_objects` with `DeRezDestination::Trash` and the agent's trash folder id.
Most edit ops need object ownership or build rights, which the sim silently
enforces.*

**18. Terrain heightmaps — `LayerData` (LAND/WATER/WIND/CLOUD) · 8 pts. ✅
Done.** Decodes the patched-DCT-compressed terrain layers into per-region
heightmaps — the ground for a renderer. New `sl-proto/src/terrain.rs` is a
faithful port of the viewer's decoder (`indra/llmessage/patch_code.cpp` +
`patch_idct.cpp`, which agree with OpenSim's `TerrainCompressor.cs`): an
MSB-first `BitReader` (matching LL's `LLBitPack::bitUnpack`, little-endian byte
reassembly), the group/patch headers, the run-length/sign/magnitude entropy
decode (with the `10` end-of-block and `97` end-of-patches markers), and the
2-D inverse DCT (dequantize + un-zigzag via the de-copy matrix, an inverse-DCT
column pass then a row pass with the `2/size` normalisation), scaled to heights
via `range/2^prequant` and the `dc_offset`. Handles both standard 16×16 patches
and the variable-region 32×32 "extended" (`'M'`/`'X'`/`'9'`/`':'`) layers (10-
vs 32-bit patch ids). New value types `TerrainLayerType` (the four layers, their
extended variants, and `Unknown`) and `TerrainPatch` (region handle, layer, grid
position, size, row-major `values`, with a `value(x, y)` accessor). Decoded in
`try_dispatch_object` so it runs on the **root and every child circuit**
(neighbour terrain streams too); cached per sim then `(layer, x, y)` and dropped
with the sim's other state on `DisableSimulator`/handover/relogin. Because
`LayerData` carries no region handle, the session learns each sim's handle from
its object updates and `EnableSimulator` (a `regions` map) and labels the
patches with it. Surfaced as `Event::TerrainPatch`; public API
`Session::terrain_patches()` / `terrain_patches_in_region(handle)` /
`terrain_height(x, y)` (root-region LAND). Wired through both runtimes
(re-exports + the exhaustive example/survey event matches; no command — terrain
is sim-pushed). Covered by four `sl-proto` unit tests (bit-reader round-trip, a
flat-patch closed-form height, the zero-size reject, and the end-of-patches
case) plus a `lifecycle.rs` end-to-end test (a synthesised `LayerData` datagram
→ `Event::TerrainPatch` + `terrain_height`). *Live-verified against the local
OpenSim via the new `terrain_probe` tokio example: a single login decoded all
**256 LAND patches** (the full 16×16 grid of a 256×256 region) with a sensible
ground-height range (≈ −0.1..25 m), plus the wind/cloud/water layers. Test:
local OpenSim.*

**19. Asset & texture pipeline (done, fetch) ✅ — CAPS
`GetTexture`/`GetMesh`/`GetMesh2`/`GetAsset`, legacy `RequestImage` +
`ImageData`/`ImagePacket`/`ImageNotInDatabase` and
`TransferRequest`/`TransferInfo`/`TransferPacket` · 13 pts.** Fetches a texture,
mesh, or generic asset by UUID over **both** transports — usable alone as an
asset fetcher given known UUIDs, and the substrate for textured rendering (#20),
animations (#21) and sound (#22). Per the scope decision, this delivers the
**fetch** layer only; actual JPEG-2000 pixel decode is out of scope (the bytes
are surfaced raw). Implemented:

- **Legacy UDP textures** — `Session::request_texture(id, discard_level,
  priority)` (`RequestImage`), with `ImageData` (the codec/size/packet-count
  header plus packet 0) and `ImagePacket` (follow-ups) reassembled by packet
  index into a `Texture { id, codec, data }` (`Event::TextureReceived`);
  `ImageNotInDatabase` → `Event::TextureNotFound`. `discard_level` is the native
  LOD knob (the sim streams from that level up).
- **Legacy UDP generic assets** — `Session::request_asset(id, AssetType,
  priority)` (`TransferRequest` on channel/source `LLTST_ASSET` = 2, params =
  UUID ++ little-endian `LLAssetType`), with `TransferInfo` (size/status) then
  `TransferPacket` chunks reassembled in order until `LLTS_DONE` → `Asset { id,
  asset_type, data }` (`Event::AssetReceived`); a non-success status →
  `Event::AssetTransferFailed`.
- **HTTP CAPS** — runtime commands `FetchTexture { texture_id, discard_level }`
  (`GetTexture`, `?texture_id=`), `FetchMesh` (`GetMesh2`/`GetMesh`,
  `?mesh_id=`) and `FetchAsset { asset_id, asset_type }` (`GetAsset`, by class),
  HTTP-GET on a background task and surfaced as the same `Texture`/`Asset`
  events. The seed now also requests these four caps.
- **Minimal J2C LOD support** — new `sl-proto::j2c` parses the codestream `SIZ`/
  `COD` markers (dimensions, components, decomposition levels) and ports the
  viewer's `calcDataSizeJ2C` byte-size estimate (`j2c::truncate_to_discard`), so
  the HTTP texture fetch can return the lower-resolution prefix for a requested
  discard level — header parsing only, **not** a pixel decoder.

New value types `AssetType` (with `to_code`/`from_code`/`get_asset_query_key`),
`ImageCodec`, `TransferStatus`, `Texture`, `Asset`. All wired as
`Command`/`SlCommand` variants through both runtimes (the HTTP fetches return
fully-formed session events over a binary-asset channel). Covered by `sl-proto`
unit tests (j2c header parse / discard-size / truncate) and four `lifecycle.rs`
tests (UDP `ImageData`+`ImagePacket` reassembly, `ImageNotInDatabase`,
`TransferRequest`→`TransferInfo`→`TransferPacket` reassembly with a `Params`
round-trip, and a `TransferInfo` failure). *Live-verified against the local
OpenSim via the new `asset_fetch` tokio example: the standard plywood texture
(`8955…`) came back as a 79 234-byte J2C over **both** the HTTP `GetTexture` cap
and the UDP `RequestImage` path, and a default sound (`ed12…`, 9 431 bytes) over
**both** `GetAsset` and a 16-packet UDP `TransferRequest` (last packet
`LLTS_DONE`). Test: local OpenSim — default textures/sounds suffice; no upload
needed.* Deferred: HTTP range requests (the LOD prefix is truncated client-side
rather than byte-ranged), AIS3 inventory-asset semantics, J2C/mesh decode, and
asset *upload* (#23).

**20. Avatar appearance & wearables (done) ✅ — `AvatarAppearance` (receive),
`AgentSetAppearance`, `AgentWearablesUpdate`/`Request`, `AgentIsNowWearing`,
`AgentCachedTexture`/`Response`, plus the modern server-side-bake CAPS
(`UpdateAvatarAppearance`) · 13 pts.** Decode other avatars' baked-texture IDs +
visual params to render them, and manage the agent's own outfit. **Receive:**
incoming `AvatarAppearance` is surfaced as `Event::AvatarAppearance` (a decoded
`AvatarAppearance` value: avatar id, the per-face **`TextureEntry`** — see
below, the visual-param bytes, the optional appearance-version / COF-version /
flags, hover height, and attachments). The key piece is a faithful port of the
viewer's packed-`TextureEntry` decoder in new `sl-proto/src/appearance.rs`
(`decode_texture_entry`): the run-length
`(default value, then (face-bitmask, value) overrides terminated by a zero bitmask)`
form for all eleven per-face fields (texture id, tint colour un-inverted from
the wire's `255−x`, scale, offset, rotation, bump/shiny/fullbright, media, glow,
material id), matching `LLPrimitive::parseTEMessage`/`unpack_TEField`. New value
types `TextureEntry`/ `TextureFace`, a `WearableType` enum and `Wearable`, an
`AvatarAppearance`/ `AvatarAttachment`, and an `avatar_texture` module of slot
constants (`HEAD_BAKED`=8 … the 11 baked slots, `COUNT`=45). The agent's own
outfit is surfaced as `Event::AgentWearables` (from `AgentWearablesUpdate`,
pushed at login and on change). **Send:** `Session::request_wearables`
(`AgentWearablesRequest`), `set_wearing` (`AgentIsNowWearing`), `set_appearance`
(`AgentSetAppearance` — the legacy client-side bake), and
`request_cached_textures` (`AgentCachedTexture`, reply
`AgentCachedTextureResponse` → `Event::CachedTextureResponse`).
**Modern Second Life server-side baking ("Sunshine"):** on a baking-capable
region the viewer no longer computes or uploads bakes — it manages the COF in
inventory and POSTs `{cof_version}` to the new `UpdateAvatarAppearance`
capability (`CAP_UPDATE_AVATAR_APPEARANCE`, added to the seed); the grid
composites and broadcasts the result over the same UDP `AvatarAppearance`. The
cap POST is wired through both runtimes (`RequestServerAppearanceUpdate`), with
its `{success, error?, expected?}` reply decoded by `handle_caps_event` into
`Event::ServerAppearanceUpdate`. All wired as `Command`/`SlCommand` variants
(`RequestWearables`, `SetWearing`, `SetAppearance`, `RequestCachedTextures`,
`RequestServerAppearanceUpdate`) through both runtimes. Built on #19 (fetch the
baked textures by id) and #5 (the COF). Covered by `sl-proto` unit tests (the TE
decoder: default fill, face override, empty blob, full round-trip) and four
`lifecycle.rs` tests (`AvatarAppearance` baked-texture + visual-param decode,
`AgentWearablesUpdate` worn list, and the `UpdateAvatarAppearance` reply →
`ServerAppearanceUpdate`). *Live-verified against the local OpenSim via the
`tokio_login_hold_logout` example: one login decoded the avatar's
`AvatarAppearance` (218 visual params, **all 11 baked slots** carrying real
texture ids) and a `RequestWearables` round-trip returned the 6 worn wearables
(Shape/Skin/Hair/ Eyes/Shirt/Pants). The server-side-bake cap is SL-only
(OpenSim's central-bake version is 0, so it uses the legacy path), so
`UpdateAvatarAppearance` is unit-tested only. Test: local OpenSim.*

**21. Animations — `AgentAnimation` (send/trigger), `AvatarAnimation` (receive)
· 5 pts. ✅ Done.** Play/stop built-in and custom animations and observe others'
— a dance/gesture bot, or motion in a renderer. **Send:**
`Session::set_animations(&[(anim_id, start)])` is the batch surface
(`AgentAnimation`: each pair starts/stops one animation; the message always
carries the single empty `PhysicalAvatarEventList` block the reference viewer
appends), with `play_animation`/`stop_animation` single-animation convenience
wrappers. `anim_id` is a built-in animation UUID or an uploaded animation asset
(custom anims are fetched via #19). **Receive:** incoming `AvatarAnimation` is
surfaced as `Event::AvatarAnimation { avatar_id, animations }` carrying a new
`PlayingAnimation` value type (`anim_id`, the simulator's per-avatar
`sequence_id`, and the optional triggering `source_id` from the
positionally-correlated `AnimationSourceList`, matching the viewer's
`process_avatar_animation`). The list is the *complete* current set, not a delta
— a stopped animation simply drops out of a later update — so consumers treat
each event as authoritative state. Wired as
`Command`/`SlCommand::{SetAnimations, PlayAnimation, StopAnimation}` through
both runtimes. Covered by three `lifecycle.rs` tests (the `AgentAnimation` send
encoding for batch start/stop and the single-animation wrapper, plus the
`AvatarAnimation` decode with source correlation and nil-vs-missing source
slots). *Live-verified against the local OpenSim via the
`tokio_login_hold_logout` tokio example: `PlayAnimation(ANIM_AGENT_CLAP)`
round-tripped — the simulator echoed an `Event::AvatarAnimation` for the agent
listing the default stand plus the triggered clap animation. Test: local
OpenSim.*

**22. Sound — `SoundTrigger`, `AttachedSound`, `PreloadSound`,
`AttachedSoundGainChange` · 3 pts. ✅ Done.** Receive and locate spatial sound
events; fetch the clips via #19. Sound is entirely sim-pushed (a scripted
`llTriggerSound`/`llPlaySound`/`llPreloadSound`), so this is a receive-only
surface with no commands. Four new events, each decoded in the main dispatch and
surfaced verbatim: **`Event::SoundTrigger`** (a one-shot spatial sound — sound /
owner / object ids, the triggering object's `parent_id` as `Option<Uuid>` with
the wire's nil → `None`, the sound's own `region_handle` since a trigger can
come from a neighbouring region, the region-local `position`, and `gain`);
**`Event::AttachedSound`** (a sound bound to an object — ids, `gain`, and a new
`SoundFlags` bitfield mirroring the viewer's `LL_SOUND_FLAG_*` constants:
`LOOP`/`SYNC_MASTER`/`SYNC_SLAVE`/`SYNC_PENDING`/`QUEUE`/`STOP`, with
`is_loop`/`is_stop`/`contains` helpers); **`Event::AttachedSoundGainChange`**
(object id + new gain, applying to the current attached sound);
**`Event::PreloadSound`** (a pre-fetch hint carrying a `Vec<SoundPreload>`, each
`{sound_id, object_id, owner_id}`). New value types `SoundFlags` and
`SoundPreload`, re-exported through both runtimes' lib re-exports (no
command/`SlCommand` variants — nothing to send). Covered by three `lifecycle.rs`
tests (the `SoundTrigger` decode incl. nil-parent → `None`, the `AttachedSound`
flag decode, and the multi-entry `PreloadSound` decode). *Live-verified against
the local OpenSim via the `tokio_login_hold_logout` example and a new
`slclient22.oar` (a scripted prim looping
`llTriggerSound`/`llPlaySound`/`llPreloadSound` of the built-in `UISndAlert`
sound `ed124764-…` on a 5 s timer): logging in next to the prim at
(128, 128, 30) the client received, every tick, an `Event::SoundTrigger`
(position (128,128,30),
gain 1), an `Event::AttachedSound` (gain 1, loop=false/stop=false), and an
`Event::PreloadSound` for that sound. `AttachedSoundGainChange` is the
`llSetSoundVolume`-on-an-already-playing-sound path and is unit-tested only.
Test: local OpenSim with the script engine enabled and a sound-playing scripted
object (same OAR mechanism as #8).*

**23. Asset/texture/mesh upload (done) ✅ — CAPS `NewFileAgentInventory`,
`UploadBakedTexture`, `UpdateGestureAgentInventory`; legacy
`AssetUploadRequest`/`SendXferPacket` · 5 pts.** Upload content over **both**
transports — usable as a content uploader on its own. Per the scope decision,
mesh upload is **bytes-only**: the caller supplies the fully-formed mesh asset
and the client uploads it verbatim; the viewer's model-import pipeline (LOD /
physics-shape / cost generation) is deliberately out of scope. Implemented:

- **Legacy UDP** —
  `Session::upload_asset_udp(asset_type, data, temp_file, store_local)` stores
  an asset via `AssetUploadRequest`, returning the asset's **predicted** UUID
  (`combine(transaction_id, secure_session_id)` — the LL `LLUUID::combine` MD5,
  ported as `sl_wire::combine_uuids`). Small assets (≤ 1200 bytes) are inlined;
  larger ones stream over `Xfer` — the simulator answers with a `RequestXfer`
  (whose `VFileID` is that predicted asset id) and the session streams
  `SendXferPacket`s (packet 0 carrying the 4-byte little-endian length prefix,
  the last flagged `0x80000000`), each pulled by the simulator's
  `ConfirmXferPacket`. Terminates as [`Event::AssetUploadComplete`]. This path
  stores only the asset (no inventory item — a viewer follows up with
  `CreateInventoryItem`).
- **Modern CAPS** — the two-step uploader (POST metadata → `uploader` URL → POST
  raw bytes → `{ new_asset, new_inventory_item }`), wired through both runtimes:
  `UploadAsset` (`NewFileAgentInventory`: stores the asset **and** creates an
  inventory item — folder, asset/inventory type, name, permissions, expected
  cost), `UploadBakedTexture` (a temporary baked texture, no inventory item),
  and `UpdateInventoryAsset` (`Update{Gesture,Notecard,Script,Settings}Agent` —
  replacing an existing item's asset, the cap chosen by asset class). Surfaced
  as `Event::AssetUploaded` / `Event::AssetUploadFailed`.

New value types `InventoryType` (the `LLInventoryType` classes, with `caps_name`
/ `to_code` / `from_code`) and `AssetType::caps_asset_name` / `update_item_cap`;
new sl-wire LLSD builders (`build_new_file_agent_inventory_request`,
`build_update_item_asset_request`, `build_upload_baked_texture_request`) and
parser (`parse_asset_upload_response` → `AssetUploadResponse`). All wired as
`Command`/`SlCommand` variants through both runtimes; the CAPS uploads run on a
background task/thread and emit their events directly (like the #19 fetches).
Covered by four sl-wire unit tests (the `combine_uuids` digest, the
`NewFileAgentInventory` body, the two-step + baked + error response parse, the
update-item body) and three `lifecycle.rs` tests (UDP inline upload + complete,
UDP `Xfer`-streamed upload + multi-packet `SendXferPacket` + complete, and the
CAPS completion decode). *Live-verified against the local OpenSim via the new
`asset_upload` tokio example: a notecard uploaded over **both** the legacy UDP
path (`AssetUploadComplete`, the reported asset id matching the predicted
`combine()` id) and the CAPS `NewFileAgentInventory` (`AssetUploaded` returning
a new asset **and** a new inventory item); a 3 KB notecard exercised the multi-
packet `Xfer` upload path end to end (`success=true`). Test: local OpenSim — no
content tooling needed, the example synthesises a notecard.* Deferred:
`UploadBakedTexture` and the `Update*` caps are SL-shaped (OpenSim uses the
legacy bake) so they are unit-tested only; the full mesh model-import pipeline
is out of scope (bytes-only upload).

**24. Media-on-a-prim / parcel audio (done) ✅ — CAPS
`ObjectMedia`/`ObjectMediaNavigate`, `ParcelMediaCommandMessage`,
`ParcelMediaUpdate` · 5 pts.** Per-face media on the scene (#16) plus the
parcel's streaming-media control surface. Two halves:

- **Object media-on-a-prim (CAPS, read + write).** A new `MediaEntry` wire value
  type (`sl-wire`) faithfully mirrors the viewer's `LLMediaEntry` — the eleven
  per-face fields (`current_url`/`home_url`, `auto_loop`/`auto_play`/
  `auto_scale`/`auto_zoom`, `first_click_interact`, `width_pixels`/
  `height_pixels`, `controls`, the white-list, and the `perms_interact`/
  `perms_control` media-perms bytes, with the viewer's `PERM_ALL` defaults) —
  with LLSD-XML build/parse helpers (`build_object_media_get_request` /
  `_update_request` / `_navigate_request`, `ObjectMediaResponse::from_llsd`).
  The field keys/verbs were cross-checked against the viewer's
  `llmediadataclient.cpp` / `llmediaentry.cpp` and OpenSim's `MoapModule`. Wired
  as
  `Command`/`SlCommand::{RequestObjectMedia, SetObjectMedia, NavigateObjectMedia}`
  through both runtimes: `RequestObjectMedia` POSTs an `ObjectMedia` GET and the
  reply is decoded by `Session::handle_caps_event` into
  `Event::ObjectMedia { object_id, version, faces: Vec<Option<MediaEntry>> }`;
  `SetObjectMedia` POSTs an `ObjectMedia` UPDATE and `NavigateObjectMedia` POSTs
  an `ObjectMediaNavigate` (both fire-and-forget — the sim advances the object's
  media version rather than replying, so a client re-fetches to observe). The
  two new caps (`CAP_OBJECT_MEDIA`, `CAP_OBJECT_MEDIA_NAVIGATE`) are added to
  the seed.
- **Parcel media control (UDP, receive-only).** Both `ParcelMediaCommandMessage`
  and `ParcelMediaUpdate` are `Trusted` (sim→viewer only), so this is a receive
  surface with no commands. A scripted `llParcelMediaCommandList` surfaces as
  `Event::ParcelMediaCommand { flags, command, time }` with a
  `ParcelMediaCommand` enum
  (Stop/Pause/Play/Loop/Texture/Url/Time/Agent/Unload/AutoAlign/Type/Size/
  Desc/LoopSet/`Other`, matching the viewer's `PARCEL_MEDIA_COMMAND_*`), and a
  parcel media-settings change surfaces as `Event::ParcelMediaUpdate` (a
  `ParcelMediaUpdateInfo`: media URL/id/auto-scale plus the extended MIME
  type/desc/width/height/loop). This complements the read-side parcel
  stream/media URLs added with #13 (the *static* `music_url`/`media_url` on
  `ParcelInfo`) — together a client now has the parcel's configured media *and*
  its live play/pause/seek control stream.

New value types `MediaEntry` (sl-wire), `ObjectMediaResponse` (sl-wire),
`ParcelMediaCommand`, `ParcelMediaUpdateInfo`, and the `MEDIA_PERM_*` constants,
all re-exported through both runtimes; the survey/example exhaustive event
matches updated. Covered by an `sl-wire` unit test (the per-face serialize →
`ObjectMediaResponse` parse round-trip, incl. the `undef` no-media slot and the
default-fill of an absent field) and three `lifecycle.rs` tests (the
`ParcelMediaCommandMessage` decode with the command enum, the
`ParcelMediaUpdate` decode incl. NUL-trimming, and the `ObjectMedia` CAPS GET
decode). *Live-verified against the local OpenSim (whose `MoapModule` serves
both caps) via the new `object_media` tokio example: rezzed a cube, set media on
face 0 over the `ObjectMedia` UPDATE cap, then fetched it back over the GET cap
— the reply decoded as 6 faces with media on face 0 (`current_url`, `auto_play`,
1024×512) and a `version` of `x-mv:0000000001/…`, the simulator's advanced media
version. `ObjectMediaNavigate` and the parcel-media receive path (which need a
scripted `llParcelMediaCommandList`) are unit-tested only. Test: local OpenSim —
no external stream needed to exercise the protocol (rendering the media itself
is out of protocol scope).*

**25. PBR materials / GLTF (done) ✅ — `GenericStreamingMessage` GLTF override,
`RenderMaterials` / `ModifyMaterialParams` CAPS, material/GLTF assets · 8 pts.**
The surface-material protocol layered on objects (#16) and the asset pipeline
(#19/#23). Per the asset-fetch scope (as with #19/#23), the GLTF document itself
is **not** parsed — material assets are fetched/uploaded as raw bytes and the
per-face GLTF *overrides* are surfaced as their raw notation-LLSD documents.
Implemented across both kinds of Second Life surface material (referenced per
face by a `TextureEntry`'s 16-byte material id):

- **Legacy materials (normal/specular) — `RenderMaterials` CAPS (OpenSim's
  path).** A new `sl-wire::material` module ports the cap's *zlib-compressed
  binary-LLSD* codec: a minimal header-less binary-LLSD reader/writer (built on
  the existing `Reader` + big-endian helpers) plus `miniz_oxide` zlib.
  `build_render_materials_request` zips a binary-LLSD array of the wanted
  material ids into the `{ "Zipped": … }` POST body OpenSim's `MaterialsModule`
  expects, and `parse_render_materials_response` unzips the reply into
  `RenderMaterialEntry { material_id, LegacyMaterial }` (normal/specular maps,
  the `*10000` fixed-point texture transforms un-scaled, spec colour/exponent,
  env intensity, diffuse-alpha mode/cutoff — cross-checked against OpenSim's
  `SOPMaterial`/`FaceMaterial.toOSD`). Driven by the runtimes'
  `RequestRenderMaterials` command → `Event::RenderMaterials`.
- **Modern GLTF (PBR) overrides — `GenericStreamingMessage` (receive) +
  `ModifyMaterialParams` (set).** Incoming material overrides arrive as a
  `GenericStreamingMessage` (method `0x4175`) carrying *notation* LLSD; a small
  notation-envelope scanner (`parse_gltf_material_override`) decodes the object
  local id and affected face indices and surfaces each per-face override as its
  raw, **undecoded** notation document — `Event::GltfMaterialOverride
  { region_handle, local_id, faces, overrides }`, decoded on the root *and*
  neighbour (child) circuits. Setting GLTF materials on object faces uses the
  `ModifyMaterialParams` cap (`build_modify_material_params_request`, an
  array of `{ object_id, side, gltf_json?, asset_id? }` with the JSON passed
  through opaque); the `{ success, message }` reply →
  `Event::MaterialParamsResult`. Driven by the runtimes' `ModifyMaterialParams`
  command.
- **Material / GLTF assets — fetch + upload over the existing pipeline.**
  `AssetType` gains `Material`/`Gltf`/`GltfBin` query keys and upload-cap names
  (`material_id`; `caps_asset_name`/`update_item_cap` →
  `UpdateMaterialAgentInventory`), plus an `InventoryType::Material`, so a
  material asset fetches (UDP `TransferRequest` by code, or the CAPS asset cap)
  and uploads (`NewFileAgentInventory` / `UpdateMaterialAgentInventory`) through
  the #19/#23 commands with no new surface. The three new caps
  (`RenderMaterials`, `ModifyMaterialParams`, `UpdateMaterialAgentInventory`)
  join the seed.

All wired as `Command`/`SlCommand` variants through both runtimes (the CAPS
POSTs run on a background task/thread; the binary `RenderMaterials` reply is
decoded off-thread into the event, the others route their LLSD reply through
`handle_caps_event`). New value types `GltfMaterialOverride`, `LegacyMaterial`,
`RenderMaterialEntry`, `MaterialOverrideUpdate` re-exported through both.
Covered by four `sl-wire` unit tests (binary-LLSD round-trip, the
`RenderMaterials` zip round-trip + response decode, the GLTF override envelope,
the `ModifyMaterialParams` body) and two `lifecycle.rs` tests (the
`GenericStreamingMessage` override → `Event::GltfMaterialOverride`, and the
`ModifyMaterialParams` reply → `Event::MaterialParamsResult`). *Live-checked
against the local OpenSim via the new `pbr_materials` tokio example: a clean
login → throttle → scene-stream → logout with the three material caps seeded and
no protocol error; the example harvests per-face material ids from the scene's
texture entries and POSTs a `RenderMaterials` fetch for them (the empty test
region returns none — stock OpenSim only serves a material once a viewer has set
one). The GLTF override + `ModifyMaterialParams` paths are SL-only (stock
OpenSim sends no overrides, nor serves the cap), so are
unit/lifecycle-tested only, as with #20's server-side bake and #23's `Update*`
caps. **All `ExtraParams` object sub-blocks are now decoded** (they are small
SL-specific structs, not standardised formats): a new `sl-proto::extra_params`
module walks the `ObjectUpdate` `ExtraParams` container (`u8` count, then per
entry a little-endian `u16` type / `u32` size / payload) into
`Object.extra: ObjectExtraParams` — `flexible` (`0x10`), `light` (`0x20`),
`sculpt`/mesh (`0x30`/`0x60`), `light_image` (`0x40`), `extended_mesh` (`0x70`),
per-face GLTF `render_material` refs (`0x80`, the `(face, material id)` list
tying #16 objects to the material assets here), and `reflection_probe` (`0x90`),
each mirroring its `unpack` in the viewer's `llprimitive.cpp`. A reflection
probe's content is a viewer-rendered cubemap, so there is nothing to fetch. The
GLTF material *document decode* (glTF 2.0) and J2C pixel decode remain out of
scope (those bytes/notation are surfaced raw).* *Test: a recent SL grid for the
GLTF paths; local OpenSim for the seed/caps + `RenderMaterials` round-trip.*

#### Tier D — specialized (needs more than local OpenSim)

**26. Voice chat (done, signalling) ✅ — Vivox/WebRTC signalling via CAPS
(`ProvisionVoiceAccountRequest`, `ParcelVoiceInfoRequest`,
`VoiceSignalingRequest`) · 13 pts.** Per the scope decision (as with the
fetch-only #19/#23/#25), this delivers the grid-side **signalling** only — the
audio transport itself (a Vivox SIP/RTP session or a WebRTC peer connection) is
out of scope, the way rendering is for the world-cluster items. A caller that
supplies its own audio engine gets the full CAPS protocol; the WebRTC SDP/ICE it
produces is passed through verbatim and the grid's answer SDP is surfaced
opaque. Implemented in a new `sl-wire/src/voice.rs`:

- **`ProvisionVoiceAccountRequest`** — `Session`-driver command
  `RequestVoiceAccount { VoiceProvisionRequest }`.
  `VoiceProvisionRequest::vivox` POSTs `{ voice_server_type: "vivox" }` (the
  grid replies with the SIP account);
  `VoiceProvisionRequest::webrtc(offer_sdp, channel_type, parcel_local_id)`
  POSTs the nested `jsep` offer (`{ type: "offer", sdp }`) plus `channel_type`,
  `parcel_local_id?` and `voice_server_type: "webrtc"`, and `webrtc_logout`
  tears a session down. The reply (Vivox
  `{ username, password, voice_sip_uri_hostname, voice_account_server_name }`
  **or** WebRTC `{ viewer_session, jsep: { type: "answer", sdp } }`) decodes
  into a single `VoiceAccountInfo` (all-optional fields; `is_webrtc()`
  discriminates) → `Event::VoiceAccountProvisioned`.
- **`ParcelVoiceInfoRequest`** — command `RequestParcelVoiceInfo` POSTs the
  empty (`undef`) body; the reply
  `{ parcel_local_id, region_name, voice_credentials: { channel_uri } }` decodes
  into `ParcelVoiceInfo` (empty `channel_uri` → `None`, i.e. no voice on the
  parcel) → `Event::ParcelVoiceInfo`.
- **`VoiceSignalingRequest`** (WebRTC ICE trickle) — command
  `SendVoiceSignaling` (`viewer_session`, a `Vec<IceCandidate>`, `completed`)
  POSTs the `candidates` array (or the end-of-gathering
  `{ candidate: { completed: true } }`) keyed by the viewer session;
  fire-and-forget (the sim returns only an HTTP status, so no event).

New value types `VoiceProvisionRequest`, `VoiceAccountInfo`, `ParcelVoiceInfo`,
`IceCandidate` and the `VOICE_SERVER_TYPE_VIVOX`/`_WEBRTC` constants; LLSD
builders `build_provision_voice_account_request` /
`build_parcel_voice_info_request` / `build_voice_signaling_request`. Three caps
(`CAP_PROVISION_VOICE_ACCOUNT`, `CAP_PARCEL_VOICE_INFO`, `CAP_VOICE_SIGNALING`)
join the seed; the provision/parcel replies route through
`Session::handle_caps_event`. All wired as `Command`/`SlCommand` variants
through both runtimes (the cap POSTs run on a background task/thread). Field
names and request/response shapes were cross-checked against the Firestorm
viewer (`llvoicevivox.cpp` / `llvoicewebrtc.cpp`) and OpenSim's
`VivoxVoiceModule` / `FreeSwitchVoiceModule`. Covered by five `sl-wire`
unit tests (the Vivox/WebRTC provision build+decode, the parcel-voice
build+decode incl. the no-voice case, and the signaling bodies) and three
`lifecycle.rs` tests (the Vivox and WebRTC provision replies and the
parcel-voice reply through `handle_caps_event`), plus a new `voice` tokio
example. *Test: stock local
OpenSim ships **no** voice module, so the caps are usually absent there (the
commands then no-op and a clean login/logout is observed); real credentials need
a FreeSWITCH/Vivox-configured OpenSim or a Second Life region. Deferred (out of
scope): the audio media transport — opening the SIP/RTP or WebRTC session, audio
codecs, and generating the SDP/ICE.*

**27. Experiences (done) ✅ — the CAPS experience APIs · 5 pts.** Permission
grants and experience-keyed scripts — an experience-permission client. The UDP
half was already in place: a script's `llRequestExperiencePermissions` arrives
in the `ScriptQuestion` `Experience` block, surfaced by #8 as
`ScriptPermissionRequest.experience_id` with the `ScriptPermissions::EXPERIENCE`
bit, and granted via the existing `answer_script_permissions`. This item adds
the full **CAPS** surface in a new `sl-wire/src/experience.rs`, faithfully
ported from the viewer's `llexperiencecache.{h,cpp}` /
`llfloaterexperiences.cpp`:

- **Read** — `RequestExperienceInfo` (`GetExperienceInfo`, batching every id as
  a `…/id/?page_size=N&public_id=<id>&…` GET → `Event::ExperienceInfo`, with
  unresolved `error_ids` folded in as `missing` placeholders), `FindExperiences`
  (`FindExperienceByName`, a paged `?query=` GET →
  `Event::ExperienceSearchResults`), `RequestExperiencePermissions`
  (`GetExperiences` → `Event::ExperiencePermissions` `{ allowed, blocked }`),
  `RequestOwnedExperiences` / `RequestAdminExperiences` /
  `RequestCreatorExperiences` (`AgentExperiences` / `GetAdminExperiences` /
  `GetCreatorExperiences` → `Event::{Owned,Admin,Creator}Experiences`),
  `RequestGroupExperiences` (`GroupExperiences`, `?<group_id>` →
  `Event::GroupExperiences`, the runtime echoing the queried group),
  `RequestExperienceAdmin` / `RequestExperienceContributor` (`IsExperienceAdmin`
  / `IsExperienceContributor`, `?experience_id=` →
  `Event::Experience{Admin,Contributor}Status`, the runtime echoing the queried
  experience), and `RequestRegionExperiences` (`RegionExperiences` GET →
  `Event::RegionExperiences` `{ allowed, blocked, trusted }`).
- **Write** — `SetExperiencePermission` (`ExperiencePreferences`: an
  `Allow`/`Block` PUT of `{ "<id>": { permission } }`, or a `Forget` DELETE of
  `?<id>`; the reply echoes the updated `{ experiences, blocked }`),
  `UpdateExperience` (`UpdateExperience` POST of the editable metadata →
  `Event::ExperienceUpdated`), and `SetRegionExperiences` (`RegionExperiences`
  POST of the three id lists, estate-gated).

New value types `ExperienceInfo` (public/agent/group ids, name, description,
`ExperienceProperties` bitfield, quota, expiration, maturity, slurl, extended
metadata, `missing`), `ExperienceProperties` (the `PROPERTY_*` bits —
`INVALID`/`PRIVILEGED`/`GRID`/`PRIVATE`/`DISABLED`/`SUSPENDED`, with `is_grid`/…
helpers), `ExperiencePermission` (`Allow`/`Block`/`Forget`), and
`ExperienceUpdate`; sl-wire builders/parsers for each cap body and reply. Twelve
caps join the seed; the self-describing replies route through
`Session::handle_caps_event` (decoded once in `sl-proto`), while the three
context-needing GETs (group/admin/contributor) build their event in the runtimes
so they can echo the queried id. All wired as `Command`/`SlCommand` variants
through both runtimes (the cap GET/PUT/DELETE/POSTs run on a background
task/thread, like the #19/#23/#26 caps). Covered by seven `sl-wire` unit tests
(the info batch query + decode incl. `error_ids`, the search escaping, the
id-list and permission decodes, the permission PUT body, the `UpdateExperience`
round-trip, the `RegionExperiences` round-trip, and the status/properties
helpers) and three `lifecycle.rs` tests (`GetExperienceInfo`, `GetExperiences`,
`RegionExperiences` through `handle_caps_event`), plus a new `experiences` tokio
example. *Test: stock local OpenSim ships **no** experience module, so the caps
are absent there — the new `experiences` example logs in, fires the queries
(which no-op as the caps are not in the map) and logs out cleanly with no
protocol error, which is what was live-verified; real data needs a Second Life
region (or an OpenSim grid with an experience module). Deferred (out of scope,
as with #19/#23/#25/#26's asset-bytes and signalling): experience *event/asset*
contents beyond the metadata records, and the experience key-value store an
experience-keyed script uses server-side.*

## Planned — deferred follow-ups of #1–#27

Everything above is done. The items below are the protocol surface that #1–#27
explicitly **deferred** — recorded in their entries as "Deferred:", "follow-up",
"remain roadmap #…", "waits on", or "unit-tested only". They are collected here
rather than interleaved among the done tiers because each is forward-looking
(unbuilt) and extends a *specific* earlier item; the "value compounds as you go
down" ordering of #1–#27 does not apply to them. Out-of-scope large items
(J2C/glTF/mesh *decode*, rendering, the voice audio transport, experience
asset-byte contents) are deliberately **excluded** — see the closing note.

**28. Complete the IM surface (done) ✅ — `ImprovedInstantMessage` offer/session
flows, `StartLure`/`TeleportLureRequest`, `RetrieveInstantMessages`,
`ReadOfflineMsgs` CAPS · 8 pts. (extends #2, Tier A.)** Item #2 implemented 1:1
IM send/receive and surfaced every inbound `ImDialog` sub-type, but several
reply/send flows were deferred; this finishes them. Implemented:
**teleport offer/lure** — `offer_teleport` (`StartLure`), `accept_teleport_lure`
(`TeleportLureRequest` with `TELEPORT_FLAGS_VIA_LURE`, driving the existing
teleport handover; the lure id's encoded region handle is parsed via OpenSim's
`BuildFakeParcelID` layout), `decline_teleport_lure` (`IM_LURE_DECLINED`), and
`request_teleport` (`IM_TELEPORT_REQUEST`). **Inventory offers** —
`give_inventory` / `give_inventory_folder` (`IM_INVENTORY_OFFERED` with the
`[asset-type byte] ++ [16-byte id]` binary bucket, a new `AssetType::Folder`
leading a folder offer), and `accept_inventory_offer` /
`decline_inventory_offer` (`IM_INVENTORY_ACCEPTED` / `_DECLINED`, the bucket
carrying the destination / trash folder id). Incoming offers decode via a new
`InstantMessage::inventory_offer` → `InventoryOffer` value type (asset type,
item id, transaction id, sender, task-vs-agent).
**Conference / ad-hoc sessions** — `start_conference`
(`IM_SESSION_CONFERENCE_START`, invitee ids packed in the bucket; call again to
add invitees), `send_conference_message` (`IM_SESSION_SEND`), `leave_conference`
(`IM_SESSION_LEAVE`), with incoming traffic surfaced as
`Event::Conference{SessionMessage,SessionParticipant,Invited}` (the
`from_group`-clear siblings of #7's group-session events; the modern CAPS
`ChatterBoxInvitation` is decoded too). **Offline-IM history** — the legacy
`retrieve_instant_messages` (`RetrieveInstantMessages` UDP, replies re-delivered
as offline `Event::InstantMessageReceived`) plus the modern `ReadOfflineMsgs`
capability (added to the seed; GET decoded by `handle_caps_event` into one
offline `Event::InstantMessageReceived` per stored record). All wired as
`Command`/`SlCommand` variants through both runtimes. Field values and the
binary-bucket layouts were cross-checked against the Firestorm viewer
(`llgiveinventory.cpp`, `llviewermessage.cpp`, `llavataractions.cpp`,
`llimview.cpp`, `llteleportflags.h`) and OpenSim's `LureModule` /
`InventoryTransferModule` / `OfflineMessageModule`. Covered by thirteen
`lifecycle.rs` tests (the lure offer/accept/decline encodings, give-item and
give-folder buckets, the inventory-offer decode + accept/decline round-trip, the
conference start/send/leave encodings and inbound decode, the
`RetrieveInstantMessages` trigger, the `ReadOfflineMsgs` array decode, and the
`ChatterBoxInvitation` decode). *Live-verified against the local OpenSim with
two accounts (Avatar Tester + Friend Tester): A offered B a teleport (B received
the `LureUser` IM and declined), and A gave B a worn body-part item — B received
the `InventoryOffered` IM (OpenSim having rewritten the session id to the copy
id, as the viewer expects), accepted it into B's inventory root, and the
`InventoryAccepted` reply round-tripped back to A. The offline-IM and conference
caps are SL-shaped (stock OpenSim's `InstantMessageModule` handles only 1:1 IMs,
and `ReadOfflineMsgs` is absent), so those are unit-tested only and the commands
no-op cleanly on OpenSim. Test: local OpenSim — two accounts for the offer
round-trips; the grid's offline-IM module plus an offline-then-relogin test for
UDP history.*

**29. Profile & pick/classified editing (done) ✅ — `AvatarPropertiesUpdate`,
`AvatarInterestsUpdate`, `AvatarNotesUpdate`, pick/classified
create-update-delete, `pickinforequest`/`ClassifiedInfoRequest` detail · 5 pts.
(extends #4, Tier A.)** Item #4 delivered the read side
(`request_avatar_properties`/`picks`/`notes`); this finishes the deferred write
side and the per-item detail fetches. Implemented: **profile editing** —
`update_profile` (`AvatarPropertiesUpdate`, a `ProfileUpdate` builder: second/
first-life images + about text, allow/mature-publish flags, web URL),
`update_interests` (`AvatarInterestsUpdate`, an `InterestsUpdate`:
want-to/skills masks + free text, languages), and `update_avatar_notes`
(`AvatarNotesUpdate`). **The classifieds *list*** item #4 never had —
`request_avatar_classifieds` (the `GenericMessage` `avatarclassifiedsrequest` →
`Event::AvatarClassifieds`, the `AvatarClassifiedReply` siblings of #4's picks
list, each a header-only `AvatarClassified` id+name). **Detail fetches** —
`request_pick_info` (`pickinforequest` `GenericMessage`, params
`[creator_id, pick_id]` as the viewer sends → `PickInfoReply` →
`Event::PickInfo`, a full `PickInfo`: creator, parcel, name/desc, snapshot, sim
name, global position, sort order, enabled) and `request_classified_info`
(`ClassifiedInfoRequest` → `ClassifiedInfoReply` → `Event::ClassifiedInfo`, a
full `ClassifiedInfo`: creator, creation/expiration dates, category, name/desc,
parcel, snapshot, sim name, global position, flags, listing price) — the
picks/classifieds lists carry only summaries. **Pick CRUD** — `update_pick`
(`PickInfoUpdate`, a `PickUpdate` builder; the session fills `creator_id` with
the agent and never sets the god-only `TopPick` flag, as the viewer does —
supply a fresh id to create, an existing one to edit), `delete_pick`
(`PickDelete`), and the god-gated `god_delete_pick` (`PickGodDelete`).
**Classified CRUD** — `update_classified` (`ClassifiedInfoUpdate`, a
`ClassifiedUpdate` builder; the sim fills the parent estate),
`delete_classified` (`ClassifiedDelete`), and `god_delete_classified`
(`ClassifiedGodDelete`). New value types `ProfileUpdate`, `InterestsUpdate`,
`AvatarClassified`, `PickInfo`, `ClassifiedInfo`, `PickUpdate`,
`ClassifiedUpdate`, and events `AvatarClassifieds`/`PickInfo`/`ClassifiedInfo`,
all wired as `Command`/`SlCommand` variants through both runtimes (plus a new
`Client::agent_id()` accessor on the tokio runtime for self-directed requests).
Field layouts and the `pickinforequest` `[creator_id, pick_id]` param order were
cross-checked against the Firestorm viewer (`llavatarpropertiesprocessor.cpp`)
and OpenSim's `UserProfileModule` / `LLClientView`. Covered by six
`lifecycle.rs` tests (the classifieds-list generic message +
`AvatarClassifiedReply` decode, the `pickinforequest` params + `PickInfoReply`
decode, the `ClassifiedInfoRequest`→`Reply` round-trip, the
profile/interests/notes update encodings, and the pick/classified create+delete
encodings). *Live-verified against the local OpenSim with the profile module
enabled (`[UserProfilesService] Enabled = true` plus pointing `[UserProfiles]
ProfileServiceURL` at the standalone's own `:9000`, not the unbound ROBUST
`:8002`) via the new `profile_edit` tokio example: a full round-trip —
`update_profile` set the about text (confirmed persisted in the `userprofile`
SQLite row and read back cold as "Edited by sl-client #29"), a pick was created
(`PickInfoUpdate`), listed (`AvatarPicksReply`), its details fetched
(`pickinforequest` → `PickInfoReply` with parcel/sim/desc) and deleted
(`PickDelete`; the roster went 1 → 0), and the same create → detail → delete
cycle for a classified (`ClassifiedInfoUpdate`/`Request`/`Reply`/`Delete`). The
interests/notes updates and the two god-delete ops are unit-tested only (the
former need a second observer to see; the latter are god-gated). Test: local
OpenSim — `[UserProfilesService] Enabled = true` (off by default; the SQLite
`UserProfiles` realm auto-migrates) with `ProfileServiceURL` reachable.*

**30. Inventory mutation & AIS3 (done) ✅ — `CreateInventoryFolder`/`Item`,
`MoveInventory*`, `CopyInventoryItem`, `RemoveInventoryItem`/`Objects`,
`UpdateInventoryItem`, `ChangeInventoryItemFlags`, `PurgeInventoryDescendents`,
`BulkUpdateInventory`/`UpdateCreateInventoryItem`, `CreateInventoryCategory` +
`InventoryAPIv3` · 8 pts. (extends #5, Tier A.)** Item #5 delivered the fetch
tree over both UDP and CAPS but deferred all mutation; this adds the full write
surface plus a **live inventory cache**. **Cache:** `Session` now keeps a
folder/item cache (`inventory_folder`/`inventory_item`/`inventory_folders`/
`inventory_items`/`inventory_children`), seeded from the login skeleton, grown
by descendents fetches (both transports), kept current by the simulator's
`BulkUpdateInventory`/`UpdateCreateInventoryItem` pushes — decoded as
`Event::InventoryBulkUpdate` / `Event::InventoryItemCreated` over both the UDP
packets and the CAPS event-queue `BulkUpdateInventory` — and updated
optimistically by the agent's own mutations. **UDP mutation:**
`create_inventory_folder`, `update_inventory_folder`,
`move_inventory_folder(s)`, `remove_inventory_folders`, `create_inventory_item`
(→ `UpdateCreateInventoryItem` with the echoed `CallbackID`),
`update_inventory_item` (with a faithful `UpdateInventoryItem` **CRC** — a port
of the viewer's `LLInventoryItem::getCRC32`, so SL's checksum matches; this
added a `last_owner_id` field to `InventoryItem`, populated from the CAPS/AIS
permissions map), `move_inventory_item(s)`, `copy_inventory_item`,
`remove_inventory_items`, `change_inventory_item_flags`,
`purge_inventory_descendents`, `remove_inventory_objects`, all wired as
`Command`/`SlCommand` variants through both runtimes. **CAPS:** the
`CreateInventoryCategory` cap (served by **both** OpenSim and Second Life) gives
a *confirmed* folder create (a synchronous
`{ folder_id, name, parent_id, type }` reply), and the modern **AIS3**
(`InventoryAPIv3`/`LibraryAPIv3`) REST surface —
`POST /category/<parent>?tid=`, `PATCH`/`DELETE /category/<id>` and
`/item/<id>`, `GET …/children?depth=` — is built in a new
`sl-wire/src/inventory.rs` (URL + LLSD-body builders) and driven by `Ais3*`
runtime commands (a new `patch_caps_llsd` verb), with replies decoded into
`Event::InventoryBulkUpdate` (the `_embedded` categories/items). New value type
`NewInventoryItem`; three caps (`InventoryAPIv3`, `LibraryAPIv3`,
`CreateInventoryCategory`) join the seed. Covered by five `sl-wire` unit tests
(the AIS URL/body builders + `CreateInventoryCategory` body) and six `sl-proto`
lifecycle tests (the create-folder / create-item / update-item-golden-CRC /
move-item encodings, and the `UpdateCreateInventoryItem` + `BulkUpdateInventory`
inbound decode + cache). *Live-verified against the local OpenSim via the new
`inventory_edit` tokio example: logged in (20-folder skeleton, root learned),
the `CreateInventoryCategory` cap returned a confirmed new folder
(`InventoryBulkUpdate`), a `CreateInventoryItem` round-tripped its
`UpdateCreateInventoryItem` (`InventoryItemCreated`), then the item was renamed
(`UpdateInventoryItem`) and the item + folder removed — a clean create → update
→ delete cycle on one login.* **AIS3 is Second-Life only** — stock OpenSim
serves no `InventoryAPIv3` cap, so the `Ais3*` commands no-op there and are
unit-tested only (as with #20/#26/#27's SL-only caps); the UDP mutation, cache,
and `CreateInventoryCategory` paths are the OpenSim-testable ones.
*Test: local OpenSim.*

**31. Group management edits (done) ✅ — group-notice creation,
`GroupRoleUpdate`, `GroupRoleChanges`, `EjectGroupMemberRequest` · 5 pts.
(extends #7, Tier A.)** Item #7 implemented membership, roster/role/profile
reads, group IM sessions, and the join/leave/invite/contribution/accept-notices
writes, but deferred the admin edits. This completes the roster-admin surface
for an owner/officer bot. Implemented: **role create/update/delete** —
`Session::update_group_roles` (`GroupRoleUpdate`, one `RoleData` block per edit)
taking a `Vec<GroupRoleEdit>` (`role_id`, name/description/title, a `powers`
u64, and a `GroupRoleUpdateType` selecting
`Create`/`UpdateData`/`UpdatePowers`/`UpdateAll`/`Delete`, the wire bytes
matching the viewer's `LLRoleChangeType` and OpenSim's
`OpenMetaverse.GroupRoleUpdate`); **member-role assignment** —
`change_group_role_members` (`GroupRoleChanges`, `Vec<GroupRoleMemberChange>`
with a `GroupRoleChange` `Add`=0/`Remove`=1); **ejecting members** —
`eject_group_members` (`EjectGroupMemberRequest`), with the
`EjectGroupMemberReply` surfaced as `Event::EjectGroupMemberResult`; and
**group-notice creation** — `send_group_notice` (`ImprovedInstantMessage`,
`IM_GROUP_NOTICE`, subject and body joined with `|`, `from_group` false) taking
an optional `GroupNoticeAttachment` (`item_id`, `owner_id`), packed into the
binary bucket as the viewer's serialized LLSD stream — the 15-byte
`<? LLSD/XML ?>\n` header (which OpenSim's group module strips verbatim) plus an
LLSD-XML `{ item_id, owner_id }` map, with the one-byte empty bucket sent when
there is no attachment (new sl-wire `build_group_notice_bucket`). New value
types `GroupRoleEdit`, `GroupRoleUpdateType`, `GroupRoleMemberChange`,
`GroupRoleChange`, `GroupNoticeAttachment`, and a `group_powers` constants
module (the `GP_*` power bits). All wired as `Command`/`SlCommand`
(`UpdateGroupRoles`, `ChangeGroupRoleMembers`, `EjectGroupMembers`,
`SendGroupNotice`) through both runtimes. Covered by one sl-wire test (the
notice bucket's LLSD header) and five `lifecycle.rs` tests (the three send
encodings, the eject reply → event, and the notice IM with/without attachment).
*Live-verified against the local OpenSim (Groups V2) via the new `group_admin`
tokio example: created a group, posted a notice (relayed back to the agent as a
member — `"sl-client #31|group management edits work"`), then ran a full role
create → list → update → delete cycle (the new role appeared with powers
`0x4000_0000_0002` = `MEMBER_INVITE | NOTICES_SEND`, its `UpdateAll` changed the
title to "Senior Tester" and powers to `NOTICES_SEND`, and the delete dropped it
from the 4-role list back to 3). The role-member assignment and eject paths need
a second group member (`SL_MEMBER`), so they are unit-tested only. Test: local
OpenSim with the Groups V2 module (MariaDB backend).*

**32. Camera & interest control — real `AgentUpdate` camera fields · 3 pts. ✅
Done. (extends #3; was blocked on #16, Tier C.)** Item #3 noted the camera
"stays at region centre — true camera control waits on position tracking from
the object/scene graph (#16)." With #16 done, the `AgentUpdate` camera position
and at/left/up axes are now a real, caller-set viewpoint. A new `Camera` value
type (`sl-proto`) holds the eye `center` and the orthonormal `at`/`left`/`up`
basis, with a `Camera::looking_at(eye, target)` helper that derives the basis
with the world-up vector exactly as the reference viewer's
`LLCoordFrame::lookAt` does (`left = up × at`, `up = at × left`), plus
`Camera::region_center` (the historic default). `Session::set_camera` persists
it on the session and sends an immediate `AgentUpdate` on the root **and** every
child circuit; the viewpoint is then re-sent on every keep-alive (root and
neighbours) and survives region changes, exactly like #3's controls and #15's
throttle, so the simulator's interest list — and thus the per-category bandwidth
(#15) — follows where the agent looks rather than the region origin. The
previously-hardcoded region-centre camera in the `AgentUpdate` builder became
the `Camera::region_center` default, so behaviour is unchanged until a client
calls `set_camera`. Draw distance keeps its existing separate surface
(`Session::set_draw_distance`, the `AgentUpdate` `far` field). Wired as
`Command`/`SlCommand::SetCamera(Camera)` through both runtimes (re-exporting
`Camera`). Covered by four unit tests (the `looking_at` right-handed
orthonormal-basis construction, the straight-down degenerate fallback, the
region-centre default matching the legacy viewpoint, and a `lifecycle.rs`
`set_camera` test asserting the `AgentUpdate` carries the camera position/axes
and persists them on the next keep-alive). *Live-verified against the local
OpenSim via the `tokio_login_hold_logout` example: a `SetCamera` looking from
above the region centre toward the north-east ground round-tripped on one login
(a real orthonormal basis `at≈(0.65,0.65,−0.40)`, `left≈(−0.71,0.71,0)`,
`up≈(0.29,0.29,0.91)`), re-sent on each keep-alive across a 12 s hold, with a
clean login→logout lifecycle and no protocol error. Test: local OpenSim.*

**33. World-stream decode & LOD-fetch completeness (done) ✅ —
`ObjectUpdateCompressed` trailing fields, HTTP `Range` LOD · 5 pts. (extends #16
& #19, Tier C.)** Two faithfulness gaps the rendering tier left raw, now closed.
**Full `ObjectUpdateCompressed` decode:** Item #16 decoded only the compressed
update's reliable fixed prefix (identity/motion/flags/text/media-url) and left
the trailing length-prefix-less fields raw, noting that walking past the legacy
particle block was "not possible from the stream alone." Cross-checking the
reference viewer's `LLViewerObject`/`LLVOVolume::processUpdateMessage` against
OpenSim's `CreateCompressedUpdateBlock` established the exact packing order and
the two fixed block sizes that make the walk possible: the legacy particle block
is a fixed `PS_LEGACY_DATA_BLOCK_SIZE` (86 bytes) and the path+profile shape is
a fixed 23 bytes, both prefix-less. `compressed_object` now decodes,
best-effort, the full tail in order — the generic `Data` field (tree genome /
linkset prim count, captured from the tree/scratchpad slot), the legacy particle
system (raw bytes), `ExtraParams` (measured by a new
`extra_params::extra_params_len` walker, then decoded via #25's
`decode_extra_params` and stored raw, exactly as a full update), attached sound
(id/gain/flags/radius), name-values, the path/profile shape (decoded into a new
`PrimShapeParams` value), the packed texture entry (its little-endian `u32`
length then the raw `TextureEntry` bytes, ready for #20's
`decode_texture_entry`), the texture-animation block (raw bytes), and the
trailing "new" particle system (raw bytes).
**The full-update decoder was completed to match:** `Object` gained
`shape: PrimShapeParams` (decoded path/profile params), `texture_anim`,
`particle_system` and `data` raw blobs, and `object_from_full_update` now
populates them from the full `ObjectUpdate` block's individual shape fields and
`TextureAnim`/`PSBlock`/`Data` fields it previously dropped — so a compressed
update yields the *same fully-populated* `Object` as a full one (shape, particle
system, texture-animation, generic data, sound, name-value, texture entry,
decoded `extra`/raw `extra_params`, text colour). The only full-block fields
still dropped are the deprecated Linden physical-joint fields (`JointType`,
`JointPivot`, `JointAxisOrAnchor`) — the reference viewer's
`processUpdateMessage` reads none of them and OpenSim's encoder `AddZeros` them,
so they carry no data. The prefix still decodes even when a malformed tail runs
short (the trailing decode short-circuits, leaving the already-decoded fields in
place). Added a non-consuming `Reader::peek_rest` to `sl-wire` to measure the
embedded `ExtraParams` container before consuming it. **HTTP range/LOD fetch:**
Item #19 fetched a texture's whole J2C codestream over the `GetTexture` cap then
truncated it client-side for a discard level; this replaces that with real HTTP
`Range` requests so only the LOD prefix crosses the wire. For a non-zero discard
the runtimes issue a small `Range: bytes=0-599` probe (`j2c::FIRST_PACKET_SIZE`,
now public) to read the J2C `SIZ`/`COD` header, compute the prefix byte length
via the existing `j2c::discard_data_size`, then fetch exactly that prefix with a
second `Range` request when the probe did not already cover it; a server that
ignores `Range` (replying `200` with the whole image) still yields the correct
prefix, just without the saving. `FetchMesh` and `FetchAsset` gained an optional
inclusive `byte_range: Option<(u32, u32)>` that issues a
`Range: bytes=start-end` request against `GetMesh2`/`GetMesh`/`GetAsset` (e.g. a
single mesh LOD whose offsets the caller read from the mesh header). All wired
through both runtimes (the tokio async and bevy blocking fetch paths). Covered
by two new `sl-proto` tests (the full compressed-tail decode of text, media,
particle, extra-params, sound, name-values, shape, texture-entry and texanim
into an `Object`; and the `extra_params_len` walker incl. its truncation clamp),
plus a shape assertion added to the existing full-`ObjectUpdate` test, on top of

## 19's existing j2c header/discard-size tests. *Live-verified against the local

OpenSim via the `asset_fetch` tokio example: the standard plywood texture
(`8955…`, a 512×512 J2C) fetched as the full 79 234 bytes at discard 0 and as a
1 536-byte prefix at discard 3 (= 64×64×3/8, the codestream truncated three LOD
levels via `Range`), on one login with a clean lifecycle. The compressed decode
is unit-tested only — stock OpenSim sends full, not compressed, `ObjectUpdate`s
(as #16 noted) — and is the SL-grid path; the mesh/asset `byte_range` rounds out
the `Range` surface for those caps. Test: local OpenSim.*

**34. Experience key-value store · ⛔ out of scope (not a client protocol
feature).** Item #27 implemented the experience metadata and permission CAPS and
deferred the experience's server-side **key-value store**, which a later pass
promoted here on the assumption it was a small client CAPS API. On
investigation it is **not** a client-facing protocol feature at all, so it is
reclassified out of scope:

- The reference viewer (Firestorm) requests all 13 experience capabilities in
  `indra/newview/llviewerregion.cpp` (`GetExperiences`, `AgentExperiences`,
  `FindExperienceByName`, `GetExperienceInfo`, `GetAdminExperiences`,
  `GetCreatorExperiences`, `ExperiencePreferences`, `GroupExperiences`,
  `UpdateExperience`, `IsExperienceAdmin`, `IsExperienceContributor`,
  `RegionExperiences`, `ExperienceQuery`) — **all implemented by #27** — and
  **no** key-value capability.
- The SL wiki's authoritative *Current Sim Capabilities* list contains no
  `ExperienceKeyValue` (nor any `KeyValue`) capability.
- `llReadKeyValue` / `llCreateKeyValue` / `llUpdateKeyValue` /
  `llDeleteKeyValue` / `llKeysKeyValue` / `llDataSizeKeyValue` appear in the
  viewer tree only inside `app_settings/keywords_lsl_default.xml` — the
  script-editor keyword list — never as cap requests or HTTP calls.

The key-value store is an **in-world LSL surface only**: scripts call those
functions and the simulator services them against an internal Linden datastore
over a service-to-service path that is never surfaced to a viewer/client. A
client cannot read or write it (not even its own experience's store — that too
is script-only). There is consequently no client wire protocol to implement, so
this joins the other out-of-scope items below (the asset-byte *decode* /
rendering / voice-transport family). With this reclassified, **the roadmap's
client protocol *feature* surface is complete: #1–#33 are done.** The only
remaining open work is the **Tier E decode-fidelity fixes (#35–#51)** — not new
features, but information-loss gaps where an already-shipped item decodes a wire
field and then drops it before the caller sees it. (#35–#49 are now
done; #50–#51 remain.)

## Tier E — decode-fidelity & information-loss fixes (#35–#51)

Gaps found by the 2026-06-18 decode audit (see the note above the tiers). Each
recovers data the wire already carries and the codec already decodes, but which
`session.rs` drops before the caller sees it. Ordered by severity. Story points
are mostly small because the wire decode exists — the work is adding fields to
the user-facing value/`Event` types and forwarding them (and, for the two
blob items, writing a structured decoder). "Test" notes whether the local
`opensim.service` exercises the path.

| # | Fix | Pts | Recovers | Test |
|---|-----|-----|----------|------|
| 35 ✅ | `ParcelProperties` full field surface | 3 | Parcel name/desc, group-ownership, sale/pass pricing, prim accounting, landing point, access/env flags, `RequestResult` | Local OpenSim |
| 36 ✅ | `ObjectProperties` full field surface | 2 | `ItemID`/`FolderID`/`FromTaskID`, `InventorySerial`, aggregate perms, texture-id list | Local OpenSim |
| 37 ✅ | `TextureAnim` & particle-system decoders | 3 | Structured prim texture-animation and particle (`llParticleSystem`) params | Local OpenSim |
| 38 ✅ | `AvatarSitResponse` complete `SitTransform` | 1 | Sit rotation, sit-camera eye/at offsets, force-mouselook | Local OpenSim |
| 39 ✅ | `RegionInfo`/`RegionHandshake` extended fields | 3 | Region owner, estate-manager flag, water height, terrain limits, 64-bit flags, chat/combat blocks | Local OpenSim |
| 40 ✅ | `AvatarAnimation` physical-event list | 1 | `PhysicalAvatarEventList` (physics/ragdoll) block | Local OpenSim |
| 41 ✅ | Asset-transfer success event + size | 2 | A success event for `TransferInfo` carrying declared `Size` | Local OpenSim |
| 42 ✅ | Group-reply pagination totals | 1 | `RoleCount` / `TotalPairs` so a client knows a set is complete | Local OpenSim (Groups V2) |
| 43 ✅ | `MoneyBalanceReply` transaction id | 1 | `TransactionID` to correlate a balance reply to its pay/buy | Money module or SL |
| 44 ✅ | Inventory push fidelity | 2 | All `UpdateCreateInventoryItem` entries; per-item bulk `CallbackID` | Local OpenSim |
| 45 ✅ | `ChatterBoxInvitation` session type & bucket | 2 | `type` + `binary_bucket` (group/session name, session kind) | SL grid |
| 46 ✅ | Terse-update trailing `TextureEntry` | 2 | Texture/colour change delivered via a terse update | SL grid |
| 47 ✅ | `ParcelAccessListReply` per-entry flags | 1 | The per-entry access-vs-ban `Flags` | Local OpenSim |
| 48 ✅ | Login-response extra fields | 2 | `home`, `look_at`, `agent_access[_max]`, `max-agent-groups`, Library inventory roots | Local OpenSim |
| 49 ✅ | `TeleportFinish` (CAPS) maturity & flags | 1 | Destination `SimAccess` (maturity), `TeleportFlags` (cause) | SL grid |
| 50 ✅ | Minor dropped-field batch | 3 | `TimeDilation`, `AlertInfo`, `MapBlockReply` water height, joint fields, collision plane, `Options.Flags`, NameValue/bump-shiny accessors | Local OpenSim |
| 51 ✅ | Attachment-point `state` un-swizzle helper | 1 | Correct attachment point from the swizzled `state` byte | Local OpenSim |

### Critical — large structural losses

**35. `ParcelProperties` full field surface (extends #13, Tier B). ✅ Done.**
`ParcelInfo` (`sl-proto/src/types.rs`) previously carried only ~16 of the ~50
`ParcelData` fields. Added the full surface and populated it in *both* decode
paths — the UDP `parcel_info` (now taking the whole `ParcelProperties` message
so it can read the three trailing single-blocks) and the CAPS
`parcel_info_from_llsd`. Recovered fields: **`Name`/`Desc`** (decoded and thrown
away before), `GroupID` + `IsGroupOwned` (a group-owned parcel can now be told
from `owner_id`), `SalePrice`/`AuthBuyerID`/`AuctionID`/`ClaimDate`/
`ClaimPrice`/`RentPrice`/`PassPrice`/`PassHours`, the full prim accounting
(`OwnerPrims`/`GroupPrims`/`OtherPrims`/`SelectedPrims`/`TotalPrims`/
`ParcelPrimBonus`), avatar counts (`SelfCount`/`OtherCount`/`PublicCount`),
`Status`/`Category`/`LandingType` (as typed `ParcelStatus`/`ParcelCategory`/
`LandingType` enums), `SnapshotID`, `UserLocation`/`UserLookAt` (the teleport
landing point), and the region access/environment booleans
(`RegionDenyAnonymous`/`…Identified`/`…Transacted`/`…AgeUnverified`, push
override, `SeeAVs`/`AnyAVSounds`/`GroupAVSounds` as `Option<bool>` since the UDP
form omits them, and the `ParcelEnvironmentBlock`). **`RequestResult`** is now a
typed `ParcelRequestResult` (`NoData`/`Single`/`Multiple`) with a `has_data()`
helper, so a "no access / not found" reply is no longer silently surfaced as a
normal parcel. `ClaimDate` is read tolerantly — an integer `time_t` (SL/UDP) or
an ISO-8601 `date` (OpenSim CAPS), via a small clippy-clean
`parse_iso8601_to_unix` + `days_from_civil` helper. The three new enums and the
extended struct are re-exported through both runtimes, and `sl-survey`'s
`ParcelRecord` JSON now carries the parcel name/description, owner,
group-ownership, sale price and prim total. Covered by `sl-proto` lifecycle
tests (full UDP field surface, the `NoData` result, and the full CAPS LLSD form
incl. the ISO-date `ClaimDate` and the per-parcel AV-sound booleans).
*Live-verified against the local OpenSim via the `survey_probe` example: a
whole-region `ParcelProperties` over the CAPS event queue decoded `name="Your
Parcel"`, `request_result=Single`, owner id, `claim_date` (ISO date parsed to a
Unix `time_t`), `status=Leased`, `total_prims`/`other_prims`,
`parcel_prim_bonus=1.0`, `region_allow_access_override=true`,
`parcel_environment_version=-1`, and the three `Some(true)` AV-sound booleans
(`see_avs`/`any_av_sounds`/`group_av_sounds`) — all previously dropped. Test:
local OpenSim (both the UDP and CAPS parcel paths).*

**36. `ObjectProperties` full field surface (extends #17, Tier C). ✅ Done.**
The `ObjectProperties` struct (`types.rs`) ended at `sit_name`; the decoder
`object_properties` (`session.rs`) dropped 8 wire fields of the `ObjectData`
block. Added and populated all of them: **`ItemID`** (`item_id` — correlate
an in-world object back to the inventory item it was rezzed from, needed for
attachments and "find in inventory"), `FolderID` (`folder_id`), `FromTaskID`
(`from_task_id` — the source object when rezzed from another object's
contents), `InventorySerial` (`inventory_serial`, an `i16` that bumps on
task-inventory changes so a client can detect them without re-fetching), the
three aggregate-permission rollups
(`aggregate_perms`/`aggregate_perm_textures`/`aggregate_perm_textures_owner` —
the build-floater "next owner can…" summary), and `TextureID`, surfaced as a
structured **`texture_ids: Vec<Uuid>`** by splitting the wire blob into
back-to-back 16-byte UUIDs (a new `concatenated_uuids` helper, ignoring any
trailing partial id). The struct is re-exported through both runtimes (no
destructuring sites needed changes — every consumer binds the whole
`ObjectProperties`). Covered by the extended
`object_properties_surface_and_merge` `sl-proto` lifecycle test (asserts the
serial, the three source ids, the three aggregate-perm bytes, and a two-UUID
`texture_ids` decode). *Live-verified against the local OpenSim via the
`rez_edit_object` example: an `ObjectSelect` → `ObjectProperties` round-trip
on a freshly-rezzed cube decoded all eight new fields end-to-end with no
protocol error — nil source ids, serial 0, zero aggregate perms and an empty
`texture_ids` (faithful: a fresh `ObjectAdd` prim has no source-inventory item
and OpenSim sends no texture-id blob in its `ObjectProperties`); the
non-trivial values are covered by the unit test. Test: local OpenSim.*

**37. `TextureAnim` & particle-system structured decoders (extends #16/#33, Tier
C). ✅ Done.** `Object::texture_anim` and `Object::particle_system` were
retained only as raw `Vec<u8>` with no decoder in the crate. Added a new
`sl-proto/src/particles.rs` with two faithful ports of the viewer's parsers, and
two new value types on `Object` alongside the (kept) raw blobs:
**`Object::texture_animation: Option<TextureAnimation>`** — the 16-byte
`TextureAnim` / `LLTextureAnim::unpackTAMessage` block (`mode` bit field, `face`
as `i8` with `-1` = all faces, the `size_x`/`size_y` frame grid, and
`start`/`length`/`rate` `f32`s), with a `texture_anim_mode` constants module
(`ON`/`LOOP`/`REVERSE`/`PING_PONG`/`SMOOTH`/`ROTATE`/`SCALE`) and the viewer's
non-`SMOOTH` "floor the grid at 1" behaviour. **`Object::particles:
Option<ParticleSystem>`** — the `PSBlock` / `LLPartSysData::unpackBlock`,
handling **both** wire forms: the legacy fixed 86-byte block (`unpackLegacy`)
and the modern size-prefixed form (`unpack` → `LLPartData::unpack`) with the
optional
trailing glow / blend-func fields gated by the `LL_PART_DATA_GLOW` /
`LL_PART_DATA_BLEND` particle flags. Recovered the full source surface (CRC,
flags, `pattern` — with a `particle_pattern` constants module —
inner/outer angle, burst rate/radius/speed-min/max/part-count, source max/start
age, angular velocity, particle acceleration, particle-texture id, target id)
**and** the per-particle template (flags, max age, start/end colour, start/end
scale, start/end glow, source/dest blend funcs). The viewer's `unpackFixed`
fixed-point reads are ported as small unsigned-`u8`/`u16` and signed-`u16`
helpers. Both decoders run at every site that fills the raw blobs — the full
`ObjectUpdate` and both the legacy and "new" particle paths of the compressed
update. The two value types, the two `decode_*` functions, and the two constants
modules are re-exported through both runtimes. Covered by five `sl-proto` unit
tests (texture-anim decode + wrong-size/grid-floor; particle legacy form,
modern-with-glow/blend form, and empty/bad-size rejection) and a `lifecycle.rs`
end-to-end test (a full `ObjectUpdate` carrying both blobs → the decoded
`Object::texture_animation` and `Object::particles`). *Unit- and
lifecycle-tested only: a live exercise needs an in-world scripted object running
`llSetTextureAnim`/`llParticleSystem` (no headless rez path — it must arrive via
an OAR or a viewer), the same constraint that left #16's compressed/terse
decoders unit-tested. The decoders are deterministic ports of
`lltextureanim.cpp`/`llpartdata.cpp`. Test: local OpenSim (rez a scripted object
running `llSetTextureAnim`/`llParticleSystem`).*

### High — fields the user clearly wants

**38. `AvatarSitResponse` complete `SitTransform` (extends #17, Tier C). ✅
Done.** `Event::SitResult` (`session.rs`) surfaced only `sit_object`,
`autopilot`, and `sit_position`, dropping the rest of `SitTransform`. Added and
populated the remaining four fields: **`sit_rotation`** (the seated
orientation — which way the avatar faces once seated), `camera_eye_offset` /
`camera_at_offset`
(scripted-sit cameras, `llSetCameraEyeOffset`/`…AtOffset`; the zero vector when
the seat's script sets no custom camera), and **`force_mouselook`**
(vehicles/weapons HUDs force the avatar into mouselook on sit). For consistency
with the codebase's geometry convention (`ObjectMotion`, the `position: Vector`
events) `sit_position` was also promoted from a bare `(f32, f32, f32)` tuple to
`sl_types::lsl::Vector`, and the new offsets/rotation use `Vector`/`Rotation`.
Re-exported through both runtimes (the `tokio_login_hold_logout` /
`bevy_login_hold_logout` examples now log the orientation and mouselook flag).
Covered by the `sl-proto` `sit_request_completes_on_response` lifecycle test,
extended to assert all four new fields round-trip (the quaternion's `s` is
reconstructed from the wire's `x/y/z`, so it is compared with an epsilon).
*Test: local OpenSim (a scripted sit target); the decode is unit-tested.*

**39. `RegionInfo` / `RegionHandshake` extended fields (extends #14, Tier B). ✅
Done.** `region_identity` and `region_limits` (`session.rs`) surfaced only the
agent/object limits, maturity, product, and the 32-bit flags. Both builders now
take the whole `RegionHandshake` / `RegionInfo` message (so they can read the
optional trailing blocks) and populate the full surface. **`RegionIdentity`**
(from `RegionHandshake`) gains `sim_owner` (the region/estate owner),
`is_estate_manager` (whether *this* agent manages the estate — gates estate UI),
`water_height`, `billable_factor`, and the **64-bit `region_flags_extended`** +
`region_protocols` from the optional `RegionInfo4` block (falling back to the
zero-extended 32-bit flags / `0` when the grid sends no `RegionInfo4`).
**`RegionLimits`** (from `RegionInfo`) gains `estate_id`/`parent_estate_id`,
`water_height`, `billable_factor`, `object_bonus_factor`, `terrain_raise_limit`/
`terrain_lower_limit`, `price_per_meter`, `redirect_grid_x`/`redirect_grid_y`,
`use_estate_sun`/`sun_hour`, the 64-bit `region_flags_extended` (from the
optional `RegionInfo3` block, same fallback), and two new optional sub-structs —
`RegionChatSettings` (the `RegionInfo5` chat whisper/normal/shout ranges +
offsets + flags) and `RegionCombatSettings` (the `CombatSettings` block) —
present only when the grid sends those blocks (`None` on OpenSim and older
grids). Both value types dropped `Eq` (now `f32` fields). The three new structs
are re-exported through both runtimes; `survey_probe` already debug-prints the
whole `RegionIdentity`/`RegionLimits`. Covered by two new `sl-proto` lifecycle
tests (`region_handshake_surfaces_extended_fields` with a populated
`RegionInfo4`, and `region_info_surfaces_extended_fields` with populated
`RegionInfo3`/`RegionInfo5`/`CombatSettings`), plus the existing two tests
extended to assert the no-optional-block fallbacks. *Live-verified against the
local OpenSim via `survey_probe`: the handshake decoded `sim_owner`,
`water_height=20.0`, `is_estate_manager=false`, and a real `RegionInfo4`
(`region_protocols=0x8000000000000000`), and the `RegionInfo` reply decoded
`estate_id=101`, `parent_estate_id=1`, `terrain_raise_limit=100`/`lower=-100`,
`object_bonus_factor=1.0`, `price_per_meter=1` — OpenSim sends no
`RegionInfo3`/`5`/`CombatSettings`, so those fall back / are `None` as designed.
Test: local OpenSim.*

**40. `AvatarAnimation` physical-event list (extends #21, Tier C). ✅ Done.**
The `AvatarAnimation` handler (`session.rs`) read `animation_list` and
`animation_source_list` but never the `PhysicalAvatarEventList` block, which the
codec decodes into the struct and the handler then dropped. Surfaced it as a new
**`physical_events: Vec<Vec<u8>>`** field on `Event::AvatarAnimation` — one
opaque `TypeData` byte blob per block, **verbatim, not decoded**: neither the
reference viewer's `process_avatar_animation` (which reads only the two
animation lists and ignores this block) nor OpenSim (which never populates it)
assigns the payload any documented structure, so a faithful surface is the raw
bytes (almost always empty). Re-exported through both runtimes via the shared
`Event` type; the `tokio_login_hold_logout` example now logs the block count.
Covered by the extended `avatar_animation_surfaces_playing_animations`
`sl-proto` lifecycle test (a populated single block round-trips to
`physical_events == [[0xDE, 0xAD, 0xBE, 0xEF]]`). *Live-verified against the
local OpenSim via `tokio_login_hold_logout`: a `PlayAnimation` round-trip echoed
`Event::AvatarAnimation` with 2 animations and `0 physical event block(s)` —
OpenSim sends an empty `PhysicalAvatarEventList`, so the field is empty as
designed, confirming the path decodes end-to-end with no protocol error. Test:
local OpenSim.*

**41. Asset-transfer success event + declared size (extends #19, Tier C). ✅
Done.** The `TransferInfo` handler (`session.rs`) emitted an event only on the
*failure* path; a successful transfer produced nothing, so `Size` (the total
asset byte size, useful for progress / preallocation) was lost. Added a new
**`Event::AssetTransferStarted { asset_id, asset_type, size }`** emitted on the
success path (status `LLTS_OK`/`LLTS_DONE`) — looking the in-flight transfer up
*without* removing it (the bytes still follow as `TransferPacket`s, reassembled
into the existing `Event::AssetReceived`); the failure path is unchanged
(`AssetTransferFailed`, which removes the transfer). `size` is surfaced as the
wire's `i32` (the simulator may send `0` when it does not know the size up
front). Re-exported through both runtimes via the shared `Event` type; the
`asset_fetch` tokio example logs the started event. Covered by the extended
`request_asset_reassembles_transfer_packets` `sl-proto` lifecycle test (asserts
the `AssetTransferStarted { sound, Sound, 6 }` fires on the `TransferInfo`
before the packets, then the `AssetReceived` reassembly). *Live-verified against
the local OpenSim via the `asset_fetch` example (`SL_ASSET_ID` = the default
sound `ed12…`): the UDP `TransferRequest` path surfaced `AssetTransferStarted …
(Sound, declared 9431 bytes)` immediately before `AssetReceived … (Sound, 9431
bytes)` — the declared size matching the reassembled asset exactly. Test: local
OpenSim.*

**42. Group-reply pagination totals (extends #7, Tier A). ✅ Done.**
`GroupRoleDataReply` dropped the `RoleCount` header and `GroupRoleMembersReply`
dropped `TotalPairs`. These replies are multi-packet, and `GroupMembersReply`
already surfaces its `member_count`, so a client could tell when the member set
was complete but *not* the role or role-member sets. Surfaced both totals as new
fields on the existing events: **`Event::GroupRoleData.role_count`** (`i32`,
from the `GroupData` block) and **`Event::GroupRoleMembers.total_pairs`**
(`u32`, from the `AgentData` block) — the simulator-reported totals across all
packets of the
reply, so a client comparing them against the accumulated `roles.len()` /
`pairs.len()` knows when a (potentially multi-packet) set is complete. Both
fields flow through both runtimes unchanged (the events are shared `sl-proto`
types; every consumer binds them with `{ .. }`, so no command wiring was
needed). Covered by two new `sl-proto` lifecycle tests
(`group_role_data_reply_surfaces_role_count` and
`group_role_members_reply_surfaces_total_pairs`, each asserting the header total
decodes alongside a single-entry packet). *Live-verified against the local
OpenSim (Groups V2) via the `group_admin` tokio example, extended to fetch role
members and log both totals: a freshly-created group's `GroupRoleDataReply`
surfaced `role_count=4` (Everyone/Officers/Owners + the new role; dropping to 3
after the role delete) and its `GroupRoleMembersReply` surfaced `total_pairs=2`
(the owner in Everyone + Owners) — both previously dropped. Test: local OpenSim
with the Groups V2 module.*

**43. `MoneyBalanceReply` transaction id (extends #11, Tier B). ✅ Done.**
`money_balance` (`session.rs`) dropped `MoneyData.TransactionID`, so a balance
reply (and its `MoneyTransaction` description) couldn't be correlated back to
the pay/buy that triggered it. Surfaced it as a new **`transaction_id: Uuid`**
field on the `MoneyBalance` value, populated from `data.transaction_id`. It is
the id the simulator echoes from the triggering transaction — e.g. the
`TransactionID` a `Session::send_money_transfer` carried — so a client tracking
in-flight payments can match the resulting (often unsolicited) balance reply to
the pay/buy that caused it; it is nil for a plain balance poll, which has no
triggering transaction. The field flows through both runtimes unchanged (the
event is a shared `sl-proto` type and every consumer binds `MoneyBalance(_)`, so
no command wiring was needed). Covered by the two existing `sl-proto` lifecycle
tests, extended to assert the new field: `money_balance_reply_surfaces_balance`
(a plain poll → nil `transaction_id`) and
`money_balance_reply_surfaces_transaction_details` (a real payment → the
`TransactionID` round-trips alongside the `MoneyTransaction` details).
*Unit-tested only: on a plain balance poll OpenSim sends a nil `TransactionID`,
and its `BetaGridLikeMoneyModule` routes no real transactions (the same reason
the #11 transfer path is unit-tested), so the non-nil correlation case — the
point of this fix — needs a money backend (Gloebit/DTL) or the real SL grid.
Test: money module or SL grid.*

### Medium

**44. Inventory push fidelity (extends #30, Tier A). ✅ Done.**
`UpdateCreateInventoryItem` (`session.rs`) used `.first()` on the repeatable
`InventoryData` block, so when the simulator batched more than one created item
into one message all but the first were dropped (from both the event and the
cache); and `bulk_update_item` dropped the per-item `CallbackID`, breaking
create-callback correlation when a result arrives as a `BulkUpdateInventory`
rather than an `UpdateCreateInventoryItem`. Fixed both: the handler now iterates
**every** `InventoryData` entry — caching each item and emitting an
[`Event::InventoryItemCreated`] per entry — and the `BulkUpdateInventory` path
collects each item's non-zero `CallbackID` into a new
**`InventoryBulkUpdate::item_callbacks: Vec<(Uuid, u32)>`** field (`(item_id,
callback_id)` pairs), so a client that issued a `copy_inventory_item` /
`create_inventory_item` (each returning a callback id) can correlate the result
to the resulting item id even when it lands as a bulk update. The three CAPS
delivery paths (event-queue `BulkUpdateInventory`, AIS3, category-create), which
carry no callback id, pass an empty `item_callbacks`. The new field flows
unchanged through both runtimes (events pass through), and the
`inventory_edit` example now copies the item it creates and logs any surfaced
callback correlation. Covered by the extended `update_create_inventory_item_*`
(two batched `InventoryData` entries → two cached items + two events) and
`bulk_update_inventory_*` (a non-zero `CallbackID` round-trips into
`item_callbacks`) `sl-proto` lifecycle tests. *Live-verified against the local
OpenSim via `inventory_edit`: create → rename → **copy** → remove ran with no
protocol error and both the original create and the copy surfaced as
`InventoryItemCreated` (a live finding: OpenSim answers `CopyInventoryItem` with
`UpdateCreateInventoryItem`, not the `BulkUpdateInventory` Second Life sends, so
the bulk-callback path is exercised by the deterministic lifecycle test rather
than this grid). Test: local OpenSim.*

**45. `ChatterBoxInvitation` session type & bucket (extends #28, Tier A). ✅
Done.** `chatterbox_invitation_from_llsd` (`session.rs`) read only
`id`/`from_id`/`from_name`/`message` from `message_params`, dropping `type` (the
session kind — group vs. ad-hoc conference vs. P2P) and `binary_bucket` (which
for a group IM carries the group/session name used to label the session), plus
`from_group`/`region_id`/`position`/`timestamp` — so a client could surface that
*an* invitation arrived but not classify or name the session it was being asked
to join. Added the full surface to `Event::ConferenceInvited`: **`dialog`** (a
typed `ImDialog` from the `type` byte — `SessionGroupStart` vs.
`SessionConferenceStart` vs. a plain add), **`from_group`** (group IM vs. ad-hoc
conference; for a group IM the `session_id` is the group id), **`session_name`**
(the human-readable label, taken from the event body's top-level `session_name`
that OpenSim supplies), **`binary_bucket`** (the dialog-dependent payload — for
a group IM the group/session name — read from the
`message_params.data.binary_bucket` nesting both OpenSim and the reference
viewer use, with a fallback to a flat `binary_bucket`), and the source
`region_id`/`position`/`parent_estate_id`/`timestamp`. The cross-checks:
OpenSim's `EventQueueGetHandlers.InstantMessageBody`
(field names, the `data.binary_bucket` nesting, `type`/`from_group`) and the
viewer's `LLViewerChatterBoxInvitation::post` (which reads the same
`message_params["data"]["binary_bucket"]`, `region_id`, `position`,
`parent_estate_id`, `timestamp`). A new `llsd_position` helper reads the
`[x, y, z]` real array. The event flows unchanged through both runtimes (every
consumer binds it with `{ .. }`; the `tokio_login_hold_logout` example now logs
the session name, dialog, and `from_group`). Covered by the extended
`chatterbox_invitation_surfaces_conference_invited` `sl-proto` lifecycle test (a
group-start invite with the `data`-nested bucket → `dialog=SessionGroupStart`,
`from_group=true`, `session_name="My Group"`, the decoded bucket, region id,
`position=(1.5, 2.5, 3.5)`, estate id, and timestamp all round-trip).
*Unit-tested only: stock OpenSim emits no CAPS `ChatterBoxInvitation` (its group
IM uses the UDP `ImprovedInstantMessage` path #7 already covers), so the CAPS
delivery is exercised by the deterministic lifecycle test rather than the local
grid. Test: SL grid.*

**46. Terse-update trailing `TextureEntry` (extends #16/#33, Tier C). ✅ Done.**
`terse_update` (`session.rs`) decoded only the motion blob, and the
`ImprovedTerseObjectUpdate` handler ignored the block's separate `TextureEntry`
field — so a texture/colour change the simulator delivers via a terse update
(when it flags the update `Textures`) was silently dropped, leaving a stale
cached `texture_entry`. Fixed: the handler now extracts that field via a new
`terse_texture_entry` helper and `apply_terse_update` writes it onto the cached
[`Object::texture_entry`] (the raw blob, decodable with
[`decode_texture_entry`], consistent with how the full `ObjectUpdate` surfaces
its own texture entry), emitting the usual [`Event::ObjectUpdated`]; a terse
update with no texture change passes `None` and leaves the cached entry
untouched. The key wire detail (cross-checked against OpenSim's
`CreateImprovedTerseBlock` vs. the full-update `CreatePrimUpdateBlock`):
unlike a full update — whose `TextureEntry` field is the bare blob — the
**terse** field is wrapped as a 2-byte inner length, two zero bytes, then the
`TextureEntry` (the outer 2-byte field length the codec already strips), so the
helper skips the four-byte wrapper to recover the blob. No new command/event
variant and no runtime wiring: `Object` already flows through both runtimes
via `Event::ObjectUpdated`. Covered by a new `sl-proto`
`terse_update_applies_trailing_texture_entry` lifecycle test (a full update
establishes an object with an empty texture entry, then a wrapped terse
`TextureEntry` field round-trips the unwrapped blob into both the event and the
cache). *Unit-/lifecycle-tested only: the reference viewer itself ignores the
terse `TextureEntry` (it reads it only on full updates), and triggering a
texture-flagged terse update needs an in-world scripted object changing a face
rapidly — the same OAR-only constraint as #37 — so the decode is exercised by
the deterministic lifecycle test rather than the local grid. Test: SL grid (the
texture-flagged terse path).*

**47. `ParcelAccessListReply` per-entry flags (extends #13, Tier B). ✅ Done.**
Each `List` entry of a `ParcelAccessListReply` (`session.rs`) carries `ID`,
`Time`, **and `Flags`**, but only id and time were mapped into
`ParcelAccessEntry` — the per-entry `AL_*` flags (the access/ban classification,
plus the Second Life experience allow/block sub-types) were dropped. Added a
`ParcelAccessFlags(u32)` bitfield value type (`ACCESS`/`BAN`/`ALLOW_EXPERIENCE`/
`BLOCK_EXPERIENCE`, with `union`/`contains`/`is_empty`, mirroring Firestorm's
`llparcelflags.h` `AL_*` constants) and a `flags` field on `ParcelAccessEntry`.
The reply handler now decodes each entry's wire `Flags` into it. The *update*
(send) path OR's any per-entry `ParcelAccessFlags` onto the list-level
`ParcelAccessScope` (so existing callers that leave it `NONE` still send just
the scope, while a Second Life client can flag an entry as an experience
allow/block) — matching OpenSim, whose `SendLandAccessListData`
(`LLClientView.cs` ~6651) sets each entry's `Flags` equal to the list's access
flag. Re-exported through both runtimes. Covered by the two existing
`lifecycle.rs` parcel-access tests, extended to assert the per-entry decode (a
`AL_BAN | AL_BLOCK_EXPERIENCE` entry) and the OR-onto-scope encode (an
`AL_ALLOW_EXPERIENCE` entry sent on an `AL_ACCESS` list). *Test: local OpenSim
(the existing #13 access-list round-trip already exercises the path; OpenSim
echoes the scope as the per-entry flags, so the experience sub-types are
unit-tested only — they need the SL grid).*

**48. Login-response extra fields (extends #5, Tier A). ✅ Done.**
`handle_login_response` (`session.rs`) plus the requested options and parser
(`sl-wire/src/login.rs`) previously captured only
`inventory-root`/`inventory-skeleton`/`buddy-list`. Now the login request also
asks for the Library options
(`inventory-lib-root`/`inventory-lib-owner`/`inventory-skel-lib`), and
[`LoginSuccess`](sl-wire/src/login.rs) carries the broadly-useful extras: the
**`home`** location (a new `HomeLocation { region_handle, position, look_at }`,
parsed from the quasi-LLSD `r`-prefixed string), the start **`look_at`**,
**`agent_access`/`agent_access_max`** (the account maturity short codes), the
**`max-agent-groups`** join limit, and the **Library** root/owner ids and folder
skeleton. `sl-proto` classifies the access codes into the typed `Maturity`
(`Maturity::from_login_access`), stores the lot in a new `LoginAccount`
reachable via `Session::login_account()`, and emits it once as `Event::Account`
(plus `Event::LibraryInventory` for the library tree) right after
`Event::CircuitEstablished`. Both runtimes forward the new events. Verified
against local OpenSim: `access Mature/Adult, max groups 42, region_handle
(256000, 256000)`, library skeleton of 19 folders. The lower-value
`gestures`/`global-textures`/`login-flags`/category lists in the same response
were left out. *Test: local OpenSim.*

**49. `TeleportFinish` (CAPS) maturity & flags (extends #10, Tier A). ✅ Done.**
Both teleport-finish decode paths (the UDP `TeleportFinish` handler and the CAPS
`teleport_finish_from_llsd`) previously read only `SimIP`/`SimPort`/
`SeedCapability` and dropped `SimAccess` (destination region maturity —
PG/Mature/Adult) and `TeleportFlags` (how/why the teleport happened — lure,
landmark, login, telehub, home, …). Both are now surfaced as a new
`Event::TeleportFinished { region_handle, sim, maturity, flags }`, emitted right
when the teleport finish is decoded (before the circuit handover, which still
proceeds to its eventual `RegionChanged`). The maturity is the typed
`Maturity::from_sim_access`; the flags are a new `TeleportFlags(u32)` bitfield
value type mirroring the reference viewer's `TELEPORT_FLAGS_*`
(`llteleportflags.h`) with named constants and a `contains` helper. The CAPS
decoder reads `SimAccess`/`TeleportFlags` tolerantly (integer or binary LLSD).
Re-exported through both runtimes (`SlSessionEvent` is `sl_proto::Event`, so the
new variant flows automatically) and added to the runtimes'/survey's exhaustive
event matches. Covered by two lifecycle tests (the UDP path asserting
Mature + `VIA_LURE | IS_FLYING`, and the CAPS path extended to assert
Mature + `VIA_LURE | VIA_LANDMARK`). *Note: OpenSim collapses the flags it
sends to `VIA_LOCATION` (+`IS_FLYING`), so the full `VIA_*` set is only
observable on the SL grid; the decode is unit-tested. Test: SL grid (the CAPS
teleport path).*

### Low

**50. Minor dropped-field batch. ✅ Done.** Small, individually-low-value drops,
all now surfaced:

- `ScriptTeleportRequest.Options.Flags` — added as
  `ScriptTeleportRequest::flags` (the first option block's `Flags`).
- `TeleportFailed.AlertInfo` — a new `AlertInfo { message, extra_params }`
  (the localizable message *key* + its substitution params) is attached to
  `Event::TeleportFailed` as `alert_info: Option<AlertInfo>` (`None` for the
  timeout path); the plain `Reason` string is still surfaced as `reason`.
- `MapBlockReply` per-block `WaterHeight` — added as
  `MapRegionInfo::water_height`.
- `TimeDilation` (the U16 in the `RegionData` of each object-update
  message, affecting motion dead-reckoning) — tracked per sim, surfaced anew
  `Event::TimeDilation { region_handle, dilation }` (the `0.0`..=`1.0`
  fraction), emitted only when the value *changes* for a region (de-duped on
  the raw `u16` so a steady sim does not re-emit on every update); cleared with
  the rest of a sim's state on `DisableSimulator`/handover/relogin.
- The avatar collision plane (the `LLVector4` read-and-discarded in
  `full_object_motion_inner` and `terse_update`) — added as
  `ObjectMotion::collision_plane` (`Option<[f32; 4]>`; `Some` for avatar
  updates, `None` for ordinary objects).
- The deprecated `JointType`/`JointPivot`/`JointAxisOrAnchor` trio dropped
  by `object_from_full_update` — added as `Object::joint_type` / `joint_pivot` /
  `joint_axis_or_anchor` (zeroed for compressed updates, which do not carry it).
- Convenience accessors for the packed bytes (no prior loss; every bit was
  retained, but the caller had to mask them): `TextureFace::bumpmap` /
  `fullbright` / `shininess` / `media_enabled` / `tex_gen` (mirroring the
  viewer's `getBumpmap()`/`getFullbright()`/`getShiny()`/…), and
  `Object::name_values` / `name_value_data` which parse the packed
  newline-separated `name_value` string into structured `NameValue` entries
  (faithful to the viewer's `LLNameValue` parser: optional `class`/`sendto`
  keywords, defaulting to `RW`/`S`).

Covered by three new `types.rs` unit tests (the bump/shiny + media-flag
unpacking and the `name_value` parser incl. defaults + blank-line skipping) and
four new `lifecycle.rs` tests (the `ScriptTeleport` option flags, the
`TeleportFailed` `AlertInfo`, the `TimeDilation` emit-on-change + de-dup, the
joint trio, and the terse avatar collision-plane vs. plain-prim `None`), plus a
`MapBlockReply` water-height assertion folded into the existing map-block test.
*Live-verified against the local OpenSim: one login showed the region's
`TimeDilation` (1.0, emitted once thanks to the de-dup), the test avatar's
collision plane (`[0, 0, 1, 30.249]` — a +Z foot plane at the standing height),
and its `name_value` pairs parsed into `FirstName`/`LastName`/`Title` entries.*
*Test: local OpenSim.*

### Interpretation trap (not loss — but a correctness footgun)

**51. Attachment-point `state` un-swizzle helper (extends #16, Tier C). ✅
Done.** The object `state` byte is passed through verbatim (no data loss), but
for an *attachment* OpenSim/SL send a **swizzled** attachment-point value
(`((st & 0xf0) >> 4) + ((st & 0x0f) << 4)`, OpenSim `LLClientView.cs`
~7208/7454/7730) in that same byte. A consumer reading `Object::state` as the
attachment point gets the wrong value unless it un-swizzles. Added two
documented accessors. **`Object::attachment_point_id() -> Option<u8>`** reverses
the nibble-swap — the reference viewer's `ATTACHMENT_ID_FROM_STATE`
(`indra_constants.h`, the macro `((st & 0xf0) >> 4) | ((st & 0x0f) << 4)`) — and
strips the transient `ATTACHMENT_ADD` (`0x80`) bit, returning the plain
attachment-point id (`1` = chest, `6` = right hand, `35` = HUD center 1).
**`Object::attachment_point() -> Option<AttachmentPoint>`** decodes that id into
the shared **`sl_types::attachment::AttachmentPoint`** enum (via its
`from_repr`, whose discriminants already match the wire ids), giving a named
point — covering both avatar points (`AttachmentPoint::Avatar`, e.g. chest,
right hand) and HUD points (`AttachmentPoint::Hud`, e.g. top-left, center) in
one value. Both return `None` for anything that is not an attachment, mirroring
the viewer's `LLVOVolume::isAttachment` (`mAttachmentState != 0`): a plain prim
(`state == 0`) and trees/grass (whose `state` byte instead carries the species,
so they are excluded by `pcode`). The typed form also returns `None` for any
id the enum does not yet name — those remain reachable via the lossless
`attachment_point_id`. The raw
`Object::state` field now carries a doc note pointing at the accessors and
explaining the per-`pcode` meanings. Backed by a small
`const fn attachment_point_from_state` helper and an `ATTACHMENT_ADD` constant;
available through both runtimes via the re-exported `Object` type with no
further wiring (`AttachmentPoint` is reached from `sl-types` directly, as #38's
geometry types are). Covered by three new `types.rs` unit tests
(`attachment_point_unswizzles_state_nibbles` — chest/right-hand nibble swaps,
the raw id, and the `ATTACHMENT_ADD`-strip case as both id and enum;
`attachment_point_decodes_hud_points` — a HUD id surfaces as both the raw id and
a typed `AttachmentPoint::Hud`; and `attachment_point_none_for_non_attachments`
— plain prim, tree, grass). *Unit-tested only: the transform is a deterministic
bit-swizzle cross-checked against both the OpenSim encoder and the viewer
decoder, operating on the `state` byte that #16/#50 already surface correctly;
a live exercise would need to attach an inventory object to the avatar (an
attach flow this headless client does not drive). Test: local OpenSim
(rez/attach an object).*

**Server-side protocol support (2026-06-18) — #52–#65, Tier F.** Everything
above is the *client* direction of the protocol: the workspace encodes what a
viewer sends and decodes what a simulator sends. Tier F adds the **server** side
so `sl-wire`/`sl-proto` can act as the *other* peer — a complete
bidirectional protocol library plus a sans-I/O skeleton per grid server role.
The generated LLUDP message codec is already symmetric (`build.rs` emits both
`encode_body` and `decode_body` for all 483 messages, and the
framing/ack/zerocode layer is direction-agnostic), so Tier F is *not* about
that layer. The work is the one-directional **hand-written sub-codecs** —
every bespoke binary blob and CAPS/LLSD payload currently has only the client
direction — plus the per-role state skeletons that have no equivalent today.
Each item is the literal inverse of an existing decoder/encoder (the existing
direction is the spec), validated by round-trip tests with that counterpart as
the oracle. The grid is several distinct servers, so the skeleton is split by
role (login server vs. simulator vs. the CAPS/grid services) rather than one
monolithic "server". Story points and the "Test" column follow the same
convention as the other tiers; most items are
unit round-trip tests (no live grid), and the `SimSession` is exercised by an
in-memory loopback against the existing client `Session`.

## Tier F — server / simulator role (#52–#65)

| # | Feature | Pts | Inverse of | Test |
|---|---------|-----|-----------|------|
| 52 ✅ | Generic LLSD-XML serializer (`Llsd` → XML) | 2 | `parse_llsd_xml` | Unit round-trip |
| 53 ✅ | Login request parse / response build (`LoginServer`) | 3 | `build_login_request` / `parse_login_response` | Unit round-trip |
| 54 ✅ | `TextureEntry` encoder | 3 | `decode_texture_entry` | Unit round-trip |
| 55 | `ExtraParams` encoder (all subtypes) | 3 | `decode_extra_params` | Unit round-trip |
| 56 | `ParticleSystem` + `TextureAnim` encoders | 3 | `decode_particle_system` / `decode_texture_anim` | Unit round-trip |
| 57 | Object-motion encoders (full / terse / compressed) | 5 | `full_object_motion` / `terse_update` / `compressed_object` | Unit round-trip |
| 58 | Terrain `LayerData` compressor | 8 | `decode_layer` | Unit (heightmap round-trip) |
| 59 ✅ | CAPS event serializers + `EventQueueGet` response | 5 | the `*_from_llsd` parsers / `build_event_queue_request` | Unit round-trip |
| 60 | `SimSession` skeleton (sans-I/O simulator session) | 8 | client `Session` | Loopback vs. `Session` |
| 61 | AIS3 inventory service pairing | 5 | the AIS3 URL/body builders | Unit round-trip |
| 62 | Experiences service pairing | 3 | `parse_experience_*` | Unit round-trip |
| 63 | Voice service pairing | 3 | `*::from_llsd` / voice request builders | Unit round-trip |
| 64 | Materials service pairing | 3 | `parse_render_materials_response` / `parse_gltf_material_override` | Unit round-trip |
| 65 | Map service pairing (`MapBlockReply` / `MapItemReply`) | 2 | the map request encoders | Unit round-trip |

Ordered foundation-first; each is one commit with reverse-direction round-trip
tests. "Inverse of" names the existing function/path the new code mirrors
field-for-field — same field order, fixed-point scales, and length-prefix
conventions.

### Foundation

**52. Generic LLSD-XML serializer (new, foundation for #53/#59/#61–#64). ✅
Done.** `sl-wire/src/llsd.rs` parsed LLSD-XML into an [`Llsd`] tree but could
only *serialize* via the bespoke per-request string builders. Added
`Llsd::to_llsd_xml`, which emits any tree as a complete `<llsd>…</llsd>`
document — the element-by-element inverse of `parse_llsd_xml`/`node_to_llsd`:
`<undef />`, `<boolean>true|false</boolean>` (round-trips through the parser's
`1`/`true` acceptance), `<integer>`/`<real>` (Rust's shortest finite-float
formatting), `<uuid>`, `<string>`/`<date>`/`<uri>` (all run through the existing
`push_escaped`), `<binary>` (standard base64, the inverse of the parser's
decode), and recursive `<array>`/`<map>`. Map keys are emitted in **sorted**
order so two equal `Llsd` trees serialize byte-for-byte identically (LLSD maps
are unordered, so the order is a free choice made deterministic). This is the
foundation every CAPS- and login-side LLSD producer (#53/#59/#61–#64) builds on
rather than hand-concatenating XML. Covered by four `sl-wire/tests/llsd.rs`
round-trip tests: every scalar kind (incl. XML-metacharacter escaping) →
serialize → re-parse-equal, nested arrays/maps round-trip, deterministic
sorted-key output (exact-string assertion), and a hand-built `EventQueueGet`
response that the existing `parse_event_queue_response` reads back. *Test: unit
round-trip (no grid).*

### Login server role

**53. Login request parse / response build (extends #5/#48, Tier A). ✅ Done.**
`sl-wire/src/login.rs` had `build_login_request` + `parse_login_response` (the
client direction). Added the server direction. **`parse_login_request`** → a new
`ParsedLoginRequest` (the same fields as `LoginRequest`, but with the
already-hashed `passwd` the server actually receives — never the plaintext — and
the `agree_to_tos`/`read_critical`/`extended_errors` acknowledgement flags
surfaced), reusing the existing `collect_members`/`member_value_node` machinery
plus a new `array_strings` for the `options` list. **`build_login_response`** is
the element-by-element inverse of `parse_login_response`: it emits the
`<methodResponse>` struct (`login` plus, on success, the ids / sim placement /
seed cap and every optional inventory-root/skeleton, buddy-list, quasi-LLSD
`home`+`look_at` with `r`-prefixed reals, access, max-agent-groups, and library
field — written only when present, so the output re-parses to an equal value),
or a failure's `reason`/`message`, or an `mfa_challenge`. The login endpoint is
XML-RPC, so `build_login_response` mirrors `build_login_request`'s XML-RPC
helpers rather than #52's LLSD-XML serializer (#52 is reused by the LLSD-side
producers #59/#61–#64). Plus a small **`LoginServer`** helper (with `Credential`
and `MfaPolicy`) whose `respond(request, credential, success)` maps a parsed
request and supplied account/sim facts to the `LoginResponse` to return:
`Success` when the hashed password matches and any MFA policy is satisfied (by a
matching one-time `token` or an echoed remembered `mfa_hash`); an `MfaChallenge`
(handing out the policy's hash + message) when MFA is required but unmet; or a
`Failure` (reason `"key"`) on a password mismatch. Covered by five
`sl-wire/tests/login.rs` round-trip tests: `parse_login_request` of
`build_login_request` (incl. the hashed password, flags, and options); a full
success, a failure, and an MFA challenge through `build_login_response` →
`parse_login_response`; and the `LoginServer` decision matrix (good/bad
password, MFA challenge, MFA satisfied by token and by remembered hash). *Test:
unit round-trip (no grid).*

### Simulator role — binary sub-codec encoders

**54. `TextureEntry` encoder (extends #16/#20, Tier C). ✅ Done.**
`sl-proto/src/appearance.rs` had `decode_texture_entry` only. Added the inverse
`encode_texture_entry`, a faithful port of the reference viewer's
`LLPrimitive::packTEMessage`/`packTEField`: the **last** face's value becomes
each field's default, then faces are scanned high→low and every value not
already carried by a higher-indexed face is emitted as a `(face bitmask, value)`
override (the bitmask flagging all at-or-below faces that share it), with the
per-field terminating zero bitmask written between the eleven fields — and none
after the trailing material field, which the decoder self-terminates. The
natural-unit values are re-quantized to the wire grid (colour re-inverted to
`255 − channel`; offsets `clamp(−1,1)·0x7FFF`; rotation `fmod(·,2π)/2π·0x8000`;
glow `clamp(0,1)·0xFF`) — the exact inverses of the decoder's de-quantization —
and faces beyond [`MAX_FACES`] (64, the wire bitmask width) are dropped to match
the decoder's cap. The variable-length face bitmask is emitted as the
most-significant-first base-128 integer the decoder reassembles. Exported from
`sl-proto` alongside `decode_texture_entry`; no runtime wiring (a server-side
binary sub-codec, reused by #57's `ObjectUpdate` body assembly and #60's
`SimSession`). Covered by three new `appearance.rs` tests (empty entry → empty
blob; an `encode`→`decode` round trip over exactly-representable values with a
shared run that exercises the default-plus-override packing and colour
re-inversion; and `decode`→`encode`→`decode` idempotency over a hand-built blob
with non-trivial quantized offset/rotation/glow and a multi-face override).
*Test: unit round-trip (no grid).*

**55. `ExtraParams` encoder (extends #16, Tier C). ✅ Done.** `extra_params.rs`
had `decode_extra_params` only. Added the inverse `encode_extra_params`: the
container framing (a `u8` count of present parameters, then per-param
little-endian `u16` type / `u32` size / payload) plus a faithful port of each
subtype's `LLNetworkData::pack` from `indra/llprimitive/llprimitive.cpp` —
flex/light/sculpt/light-image/extended-mesh/render-material/reflection-probe. A
field that is `None` (or, for render materials, an empty list) is omitted, so a
default round-trips to a lone zero count byte; present parameters are emitted in
ascending type-code order (the order the viewer's parameter list, keyed by
`type >> 4`, iterates). Subtype details mirror the decoder's inverses: the
flexi "softness" bits are re-stashed in the high bits of the tension/drag bytes
with the viewer's `* 10.01`-then-truncate quantization; the reflection-probe
booleans are recombined into the box/dynamic/mirror flag byte; render materials
are capped at the viewer's 14-entry block limit; and sculpt/mesh is always
written under the canonical `PARAMS_SCULPT` code (the decoder accepts the
`PARAMS_MESH` alias too). Exported from `sl-proto` alongside
`encode_texture_entry`; no runtime wiring (a server-side binary sub-codec,
reused by #57's `ObjectUpdate` body assembly and #60's `SimSession`). Covered by
three new `extra_params.rs` tests (default → lone zero byte; a fully-populated
`encode`→`decode` round trip across all seven subtypes that also checks the
encode is the exact deterministic inverse; and the 14-entry render-material
cap). *Test: unit round-trip (no grid).*

**56. `ParticleSystem` + `TextureAnim` encoders (extends #37, Tier E). ✅
Done.** `particles.rs` decodes the legacy 86-byte and modern size-prefixed
particle forms and the 16-byte texture-anim block. Added the inverse
`encode_texture_anim` and `encode_particle_system`. `encode_texture_anim` is a
port of the viewer's `LLTextureAnim::packTAMessage` — the four header bytes
(mode / face / grid x / grid y) then three little-endian `F32`s — writing the
grid dimensions verbatim (the decoder, not the encoder, applies the
non-`SMOOTH` floor-to-1). `encode_particle_system` chooses the wire form the way
the decoder distinguishes them (mirroring `LLPartSysData::isLegacyCompatible`):
a system carrying neither glow nor blend-func data (neither `LL_PART_DATA_GLOW`
nor `LL_PART_DATA_BLEND` in `part_flags`) is the legacy fixed 86-byte form — the
68-byte source block then the 18-byte legacy particle block, no size prefixes —
otherwise the modern form, prefixing each sub-block with its `S32` size and
appending the glow / blend-func bytes gated by those flags. Every fixed-point
field is re-quantized with the exact inverse of the decoder's `unpackFixed`
(`LLDataPacker::packFixed`): clamp to range, scale by `2^frac_bits`, truncate
toward zero — the unsigned 8.8 scalars (`max_age`/`start_age`/the burst fields/
`part_max_age`), the unsigned 3.5 angles and scales, and the signed 8.7
angular-velocity / acceleration vectors (bias `+2^int_bits` then scale); glow is
`trunc(value · 255)`. Exported from `sl-proto` alongside the decoders; no
runtime wiring (server-side binary sub-codecs, reused by #57's `ObjectUpdate`
body assembly and #60's `SimSession`). Covered by four new `particles.rs` tests
(the
texture-anim `decode`→`encode` byte identity plus an `encode`→`decode` value
round trip; a legacy `encode`→`decode` full-struct round trip over
exactly-representable values that asserts the 86-byte length; a modern round
trip exercising glow+blend and a glow-only form with the matching size checks;
and `decode`→`encode` byte-for-byte idempotency over the hand-built modern
blob). *Test: unit round-trip (no grid).*

**57. Object-motion encoders (extends #16/#33/#46, Tier C). ✅ Done.** The
motion decoders (`full_object_motion`, `terse_update`, `compressed_object` +
`compressed_object_trailing`, with their `read_quantized_vector` /
`read_compressed_shape` / `read_nul_string` helpers and the `COMPRESSED_*`
flags) were extracted out of `session.rs` into a new
`sl-proto/src/object_update.rs`, co-located with the new inverse encoders:
`encode_object_motion` (the full-precision `ObjectUpdate` `ObjectData` blob),
`encode_terse_object_data` (the `ImprovedTerseObjectUpdate` `Data` blob),
`encode_terse_texture_entry` (the wrapped terse `TextureEntry` field), and
`encode_compressed_object` (the `ObjectUpdateCompressed` `Data` blob). The
public `TerseUpdate` struct and all four encoders are exported from `sl-proto`;
the decoders stay `pub(crate)` and `object_from_full_update` /
`shape_from_full_block` (full-update glue using session's `trimmed_string`) stay
in `session.rs`, calling into the new module. The 16-bit terse fields
re-quantize with the round-tripping `F32_to_U16_ROUND` (LL's plain `F32_to_U16`
floors and can re-encode one quantum short); the compressed encoder computes the
`CompressedFlags` from which fields the `Object` carries (non-empty `data`
always as the scratchpad form; an 86-byte particle blob as legacy, else "new")
and emits the raw `texture_entry` / `texture_anim` / `particle_system` /
`extra_params` byte fields a server assembles via #54/#55/#56 (the `ExtraParams`
container is rebuilt from the decoded `extra` via #55 when the raw field is
empty, so it is always a valid framed block). NO runtime wiring beyond rerouting
the existing decode call sites; `ZERO_VECTOR` / `IDENTITY_ROTATION` made
`pub(crate)` for sharing. Six round-trip tests in `object_update.rs`
(full-motion 60/76-byte byte-identity, terse byte-identity over grid-point
quantized values,
the terse texture-entry wrapper, and rich + minimal compressed-object
decode→encode→decode round trips with byte identity). NB the public encoder docs
must use a plain code span for the `pub(crate)`/private decoders (`f32_to_u16`,
`compressed_object`, …), not an intra-doc link (the `cargo doc`
`private_intra_doc_links` `-D` check, same as #55/#56). *Test: unit round-trip
(no grid).*

**58. Terrain `LayerData` compressor (extends #18, Tier C). ✅ Done.**
`terrain.rs` gains `encode_layer` (exported from `sl-proto`), the inverse of
`decode_layer`: prescan each patch for its range/DC-offset, scale heights onto
the `2^PREQUANT` (=10, as the viewer/OpenSim use) quantizer grid, run a forward
DCT, quantize+zigzag the coefficients into transmission order, choose the
minimal lossless `word_bits`, and entropy-code them (`0`=zero,
`10`=end-of-block, `11`+sign+magnitude), framed by the group header
(`stride`/`size`/layer
code) and a trailing `END_OF_PATCHES` written through a new MSB-first
`BitWriter` (the exact inverse of the decoder's `BitReader`). The **forward DCT
is the exact algebraic inverse of the decoder's `inverse_dct`** — `B =
(2/size)·E·spatial·Eᵀ` with the same `icosines` table and `w(0)=1/√2`,
`w(u>0)=1` weights, so the single `2/size` and both weights mirror the decoder
rather than re-deriving OpenSim's hardcoded 16×16 Ooura routine; this handles
the 32×32 extended (variable-region) patches too, which the OpenSim reference
does not. The patch-coordinate width (10 vs 32 bits) follows
`layer.is_extended()`, the cell-grid size comes from each patch's `size`. NO
runtime wiring (terrain is sim-pushed; this is a server-side encoder). Five
round-trip tests in `terrain.rs` (flat patch near-lossless; smooth ramp+bump
within the quantization tolerance; multi-patch coordinate preservation; a 32×32
`LandExtended` patch with a large 32-bit patch X; and decode→encode→decode
stability) plus the existing decoder/lifecycle tests unchanged. NB the public
`encode_layer` doc uses a plain code span for the `pub(crate)` `decode_layer`,
not an intra-doc link (the `cargo doc` `private_intra_doc_links` `-D` check,
same as #55–#57). *Test: unit round-trip (no grid); the local OpenSim's live
`LayerData` already exercises the matching decoder (#18).*

### Simulator role — CAPS event queue & session

**59. CAPS event serializers + `EventQueueGet` response (extends the CAPS
items #10/#13/#28/#30). ✅ Done.** For each inbound CAPS parser in `session.rs`
the inverse `*_to_llsd` was added — the element-by-element mirror that lets a
simulator / grid service *produce* the LLSD body the client decodes, so an
`Llsd` round-trips back to an equal decoded value: `teleport_finish_to_llsd`,
`enable_simulator_to_caps_llsd`, `crossed_region_to_caps_llsd`,
`establish_agent_communication_to_llsd`, `server_appearance_update_to_llsd`,
`parcel_info_to_llsd`, `offline_messages_to_llsd`,
`chatterbox_invitation_to_llsd`, `group_memberships_to_caps_llsd`,
`group_members_to_caps_llsd`, `inventory_descendents_to_llsd`,
`bulk_update_inventory_to_llsd`, `ais_inventory_update_to_llsd` and
`created_category_to_llsd` (with `pub(crate)` folder/item/record leaf helpers
`inventory_folder_to_llsd` / `inventory_item_to_llsd` /
`bulk_update_item_to_llsd` / `offline_message_to_record`). Plus
`build_event_queue_response(id, &[EventQueueEvent])` in `sl-wire/src/llsd.rs` —
a `{ id, events: [{ message, body }…] }` batch built on #52's
`Llsd::to_llsd_xml`, the inverse of `parse_event_queue_response` and the server
counterpart of the client's `build_event_queue_request`. The top-level encoders
are exported `pub` from `sl-proto` (terrain-style: no runtime consumer yet,
reused by the `SimSession` skeleton, #60). New `u32`/`u64` LLSD encoders mirror
the tolerant `llsd_u32`/`llsd_u64` readers (plain integer when it fits an `i32`,
else big-endian binary — the `big_endian_bytes` lint forbids `to_be_bytes`, so
the bytes are extracted by hand); `ParcelRequestResult`/`ParcelStatus` gained
`to_i32` and `LandingType` a `to_u8` (the inverse classifiers). Covered by 14
`sl-proto` round-trip tests + 1 `sl-wire` test (each value → `*_to_llsd` →
`*_from_llsd` → equal; AIS uses uuid-keyed unordered maps so its test sorts
before comparing). Built on #52.

**60. `SimSession` skeleton (new; mirror of the client `Session`).** A sans-I/O
type in `sl-proto` that accepts a circuit (`UseCircuitCode` +
`CompleteAgentMovement`), tracks sequence / pending acks (reusing the symmetric
`sl-wire` ack & seq machinery), answers `StartPingCheck`/`CompletePingCheck`,
handles `LogoutRequest`, exposes a typed API to push server messages
(`RegionHandshake`, `ChatFromSimulator`, `ObjectUpdate`, `LayerData`, …) and to
enqueue CAPS events (#59), and decodes the ~123 client-only messages into a
server-side `ServerEvent` enum (the inverse of the client `Command`/`Event`
split). Tested by driving a `SimSession` and a client `Session` against each
other in memory through the real framing/ack/zerocode path.

### Grid / CAPS service roles

**61. AIS3 inventory service pairing (extends #30, Tier A).**
`sl-wire/src/inventory.rs` has the AIS3 URL + request-body builders. Add the
`parse_ais_*` request-body parsers and the AIS3 response builders.

**62. Experiences service pairing (extends #27, Tier D).**
`sl-wire/src/experience.rs` has query builders + the `parse_experience_*`
response parsers. Add the request parsers (`parse_set_experience_permission_`
`request`, update, region) and the response builders.

**63. Voice service pairing (extends #26, Tier D).** `sl-wire/src/voice.rs` has
the request builders + `VoiceAccountInfo`/`ParcelVoiceInfo::from_llsd`. Add
`parse_provision_voice_account_request`, `parse_voice_signaling_request`,
and the account/parcel-info response builders. (The voice *audio* transport
stays out of scope — this is only the signalling-endpoint pairing.)

**64. Materials service pairing (extends #25, Tier C).**
`sl-wire/src/material.rs` is partially bidirectional. Add the gaps:
`build_gltf_material_override`,
`parse_modify_material_params_request`, and `build_render_materials_response`
(the zipped binary-LLSD legacy-materials reply).

**65. Map service pairing (extends #12, Tier B).** Encoders for the
`MapBlockReply` / `MapItemReply` payloads the map server returns, mirroring the
existing client request encoders.

## Out of scope (not LLUDP/CAPS protocol)

Rendering/physics engines, J2C/mesh *display* (vs. decode), the in-viewer UI,
and the Marketplace (web, not protocol) are deliberately excluded — this roadmap
covers protocol features only. **Server roles are in scope as of Tier F
(#52–#65) — the bidirectional codec and the per-role sans-I/O skeletons — but
a *running* grid is not: world authority, persistence, multi-client broadcast,
and the socket/event-loop I/O remain the consumer's job, not these crates'.**
The same
scope as #19/#23/#25/#26/#27 holds for the follow-ups above: J2C/JPEG-2000 pixel
*encode/decode*, the glTF 2.0 document decode, the mesh model-import
(LOD/physics/cost) pipeline, the voice audio media transport (SIP/RTP/WebRTC),
and an experience's event/asset *byte contents* remain out — those are large
items an existing crate would own, not protocol wiring. The **experience
key-value store (#34)** is out for a different reason: it has no client
capability at all (the viewer never accesses it — see #34's entry), so there is
no client wire protocol to own.
