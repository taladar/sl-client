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
| 28 | Complete the IM surface | 8 | Offer-handler / IM bot (full) | Local OpenSim (2 accts) |
| 29 | Profile & pick/classified editing | 5 | Profile editor | Local OpenSim (profiles) |
| 30 | Inventory mutation & AIS3 | 8 | Inventory manager, product bot | Local OpenSim |
| 31 | Group management edits | 5 | Group admin bot | Local OpenSim (Groups V2) |
| 32 | Camera & interest control | 3 | Look-aware roaming bot | Local OpenSim |
| 33 | World-stream decode & LOD fetch | 5 | *(faithfulness for 16/19)* | Local OpenSim |
| 34 | Experience key-value store | 3 | Experience datastore client | **SL grid only** |

**Items #28–#34 are not yet built.** They are the **deferred follow-ups** that
items #1–#27 knowingly left for later (the "Deferred:" / "follow-up" /
"waits on #…" / "unit-tested only" notes in those entries), now promoted to
first-class roadmap items so the gap analysis stays complete. Each is grouped
under and extends an earlier item; their full prose is in **"Planned — deferred
follow-ups"** below the done items. They carry no ✅ and the prose is
forward-looking. Out-of-scope large items (J2C/glTF/mesh decode, rendering, the
voice audio transport) are *not* among them — see the closing "Out of scope"
note.

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
`login_hold_logout` example: one login decoded the avatar's `AvatarAppearance`
(218 visual params, **all 11 baked slots** carrying real texture ids) and a
`RequestWearables` round-trip returned the 6 worn wearables (Shape/Skin/Hair/
Eyes/Shirt/Pants). The server-side-bake cap is SL-only (OpenSim's central-bake
version is 0, so it uses the legacy path), so `UpdateAvatarAppearance` is
unit-tested only. Test: local OpenSim.*

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
slots). *Live-verified against the local OpenSim via the `login_hold_logout`
tokio example: `PlayAnimation(ANIM_AGENT_CLAP)` round-tripped — the simulator
echoed an `Event::AvatarAnimation` for the agent listing the default stand plus
the triggered clap animation. Test: local OpenSim.*

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
the local OpenSim via the `login_hold_logout` tokio example and a new
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

**28. Complete the IM surface — `ImprovedInstantMessage` offer/session flows,
`RetrieveInstantMessages`, `ReadOfflineMsgs` CAPS · 8 pts. (extends #2, Tier
A.)** Item #2 implemented 1:1 IM send/receive and surfaced every inbound
`ImDialog` sub-type, but several reply/send flows were deferred; this finishes
them. Will add: **offer reply flows** — accept/decline a **teleport** offer/lure
(the `TeleportLureRequest` / lure-accept handshake) and accept/decline an
**inventory** offer received over IM (`IM_INVENTORY_OFFERED`, replying with the
accepted/declined dialog so the sim files the offered item into the target
folder or drops it); **give inventory** — an outgoing inventory-offer helper
(`IM_INVENTORY_OFFERED` send with the binary-bucket asset/folder reference, the
counterpart to #5's inventory and #30's mutation); **conference / ad-hoc
multi-party sessions** — start/invite/leave a non-group conference
(`IM_SESSION_CONFERENCE_START` and the ad-hoc session dialogs), the sibling of
item #7's group sessions over the same IM multiplexing; and **offline-IM
history** — the legacy `RetrieveInstantMessages` UDP trigger plus the modern SL
`ReadOfflineMsgs` CAPS path. New `Command`/`SlCommand` variants + `Event`s
through both runtimes. *Test: local OpenSim — two accounts for the
offer/conference round-trips; the grid's offline-IM module plus an
offline-then-relogin test for history.*

**29. Profile & pick/classified editing — `AvatarPropertiesUpdate`,
pick/classified create-update-delete, `PickInfoRequest`/`ClassifiedInfoRequest`
detail · 5 pts. (extends #4, Tier A.)** Item #4 delivered the read side
(`request_avatar_properties`/`picks`/`notes`); the write side and the per-item
detail fetches were the deferred half. Will add: editing one's own profile
(`AvatarPropertiesUpdate`), create/update/delete of picks and of classifieds,
and the pick/classified **detail** fetches (`PickInfoRequest`/`PickInfoReply`,
`ClassifiedInfoRequest`/`ClassifiedInfoReply`) — the picks/notes lists item #4
returns carry only summaries. New `Session` setters + detail events through both
runtimes. *Test: local OpenSim with the profile module enabled (`[UserProfiles]
ProfileServiceURL`).*

**30. Inventory mutation & AIS3 — `CreateInventoryFolder`/`Item`,
`MoveInventory*`, `CopyInventoryItem`, `RemoveInventoryItem`,
`UpdateInventoryItem`, `BulkUpdateInventory`, `InventoryAPIv3` · 8 pts.
(extends #5, Tier A.)** Item #5 delivered the fetch tree over both UDP and CAPS
but deferred all mutation. Will add: create/move/copy/delete/update of folders
and items, watching the sim's `BulkUpdateInventory`/`UpdateInventoryItem` pushes
to keep the cached tree live, and the modern **AIS3** (`InventoryAPIv3`) REST
capability semantics (which also covers #19's deferred "AIS3 inventory-asset
semantics"). Turns #5's read-only manager into a true inventory manager /
product-update bot, and provides the file-into-folder step behind #28's
inventory-offer accept. *Test: local OpenSim.*

**31. Group management edits — group-notice creation, `GroupRoleUpdate`,
`GroupRoleChanges`, `EjectGroupMemberRequest` · 5 pts. (extends #7, Tier A.)**
Item #7 implemented membership, roster/role/profile reads, group IM sessions,
and the join/leave/invite/contribution/accept-notices writes, but deferred the
admin edits. Will add: group-**notice creation** (with an inventory attachment
in the binary bucket), role create/delete (`GroupRoleUpdate`), member-role
assignment edits (`GroupRoleChanges`), and ejecting members
(`EjectGroupMemberRequest`) — completing the roster-admin surface for an
owner/officer bot. *Test: local OpenSim with the Groups V2 module.*

**32. Camera & interest control — real `AgentUpdate` camera fields · 3 pts.
(extends #3; was blocked on #16, Tier C.)** Item #3 noted the camera "stays at
region centre — true camera control waits on position tracking from the
object/scene graph (#16)." With #16 done, this populates the `AgentUpdate`
camera position, the at/left/up axes and the draw distance from a real,
caller-set viewpoint (a `set_camera`-style surface, persisted and re-sent on
keep-alive like #3's controls), so the simulator's interest list and the
per-category bandwidth (#15) follow where the agent actually looks rather than
the region origin. *Test: local OpenSim.*

**33. World-stream decode & LOD-fetch completeness — `ObjectUpdateCompressed`
trailing fields, HTTP `Range` LOD · 5 pts. (extends #16 & #19, Tier C.)** Two
faithfulness gaps the rendering tier left raw. **Full `ObjectUpdateCompressed`
decode:** Item #16 decodes the compressed update's reliable fixed prefix
(text/media-url) but leaves the trailing length-prefix-less fields raw — this
adds the particle-system block, name-values, the shape (path/profile params),
the packed texture-entry (via #20's `decode_texture_entry`), and the
compressed-path extra-params (via the `extra_params` decoder #25 added for full
updates), so a compressed-heavy SL region yields the same decoded `Object` as a
full update. **HTTP range/LOD fetch:** Item #19 fetches textures/meshes/assets
over CAPS but truncates the J2C codestream client-side for a discard level; this
replaces that with real HTTP `Range` requests against
`GetTexture`/`GetMesh2`/`GetAsset` so only the needed byte prefix is
transferred. *Test: local OpenSim — stock OpenSim sends full (not compressed)
updates, so the compressed decode is unit-tested there; range fetch is
live-checkable against the texture caps.*

**34. Experience key-value store — `ExperienceKeyValue` datastore CAPS · 3 pts.
(extends #27, Tier D.)** Item #27 implemented the experience metadata and
permission CAPS but deferred the experience's server-side **key-value store** as
out of scope; reconsidered, it is a small CAPS protocol API (not a large
external-crate item), so it is promoted here. Will add the `ExperienceKeyValue`
datastore verbs — read / write / delete / list keys for an experience, with the
quota the viewer's experience cache tracks — the store an experience-keyed
script reads and writes server-side. *Test: SL grid (or an OpenSim grid with an
experience module); the caps are absent on stock OpenSim, as with #27.*

### Out of scope (not LLUDP/CAPS protocol)

Rendering/physics engines, J2C/mesh *display* (vs. decode), the in-viewer UI,
and the Marketplace (web, not protocol) are deliberately excluded — this roadmap
covers protocol features only. The same scope as #19/#23/#25/#26/#27 holds for
the follow-ups above: J2C/JPEG-2000 pixel decode, the glTF 2.0 document decode,
the mesh model-import (LOD/physics/cost) pipeline, the voice audio media
transport (SIP/RTP/WebRTC), and an experience's event/asset *byte contents*
remain out — those are large items an existing crate would own, not protocol
wiring.
