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
| 19 | Asset & texture pipeline | 13 | Asset fetch + textured rendering | Local OpenSim (upload content) |
| 20 | Avatar appearance & wearables | 13 | Render avatars; outfit control | Local OpenSim |
| 21 | Animations | 5 | Dance/gesture bot; animate scene | Local OpenSim |
| 22 | Sound | 3 | Spatial audio playback | Local OpenSim |
| 23 | Asset/texture/mesh upload | 5 | Content uploader | Local OpenSim |
| 24 | Media-on-a-prim / parcel audio | 5 | Media surfaces, streaming audio | Local OpenSim (external stream) |
| 25 | PBR materials / GLTF | 8 | Modern materials in a renderer | **Recent SL grid; OpenSim varies** |
| 26 | Voice chat | 13 | Voice-enabled client | **SL Vivox/WebRTC or FreeSWITCH** |
| 27 | Experiences | 5 | Experience-permission client | **SL grid only** |

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
read via a tolerant `llsd_u32` (binary/integer/string). *Live-verified against
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

**19. Asset & texture pipeline — CAPS `GetTexture`/`GetMesh2`, legacy
`TransferRequest`/ `TransferPacket`/`RequestImage` + `ImageData`/`ImagePacket` ·
13 pts.** HTTP texture/mesh fetch (range requests, J2C/JPEG2000 decode, mesh LOD
parsing) plus the legacy UDP asset path for
sounds/animations/notecards/landmarks. Underpins textured rendering, appearance,
animations, and sound; usable alone as an asset fetcher given known UUIDs. Big
because of the J2C decoder and the dual HTTP+UDP paths. *Test: local OpenSim —
upload assets first; SL grid for full-scale CDN behavior.*

**20. Avatar appearance & wearables — `AvatarAppearance` (receive),
`AgentSetAppearance`, `AgentWearablesUpdate`, `AgentIsNowWearing`, CAPS baking ·
13 pts.** Decode other avatars' baked-texture IDs + visual params to render
them; manage own outfit via the COF. Depends on #19 (textures) and #5
(inventory). *Test: local OpenSim.*

**21. Animations — `AgentAnimation` (send/trigger), `AvatarAnimation` (receive)
· 5 pts.** Play/stop built-in and custom animations and observe others' — a
dance/gesture bot, or motion in a renderer. Custom (uploaded) anims depend on

### 19. *Test: local OpenSim.*

**22. Sound — `SoundTrigger`, `AttachedSound`, `PreloadSound`,
`AttachedSoundGainChange` · 3 pts.** Receive and locate spatial sound events;
fetch the clips via #19. *Test: local OpenSim.*

**23. Asset/texture/mesh upload — CAPS `NewFileAgentInventory`,
`UploadBakedTexture`, `UpdateGestureAgentInventory`; legacy
`AssetUploadRequest`/`SendXferPacket` · 5 pts.** Upload content and create
inventory items; needed for appearance baking (#20). Depends on #5.
*Test: local OpenSim.*

**24. Media-on-a-prim / parcel audio — CAPS `ObjectMedia`/`ObjectMediaNavigate`,
parcel audio/media URLs · 5 pts.** Per-face media and streaming audio on the
scene (#16). *Test: local OpenSim with an external stream URL (rendering the
media itself is out of protocol scope).*

**25. PBR materials / GLTF / reflection probes — `RenderMaterialParams`, CAPS
material assets, GLTF override decode · 8 pts.** Modern SL rendering layered on
objects (#16) and textures (#19).
*Test: a recent SL grid; OpenSim support varies by build/version.*

#### Tier D — specialized (needs more than local OpenSim)

**26. Voice chat — Vivox/WebRTC signalling via CAPS
(`ProvisionVoiceAccountRequest`, `ParcelVoiceInfoRequest`, WebRTC session SDP) ·
13 pts.** An entirely separate subsystem (SIP/RTP or WebRTC) bolted on via CAPS.
*Test: SL grid (Vivox/WebRTC backend) or an OpenSim configured with a FreeSWITCH
voice module — not available on stock local OpenSim.*

**27. Experiences — CAPS experience APIs, `ScriptExperience*` · 5 pts.**
Permission grants and experience-keyed scripts.
*Test: SL grid only — no OpenSim equivalent.*

##### Out of scope (not LLUDP/CAPS protocol)

Rendering/physics engines, J2C/mesh *display* (vs. decode), the in-viewer UI,
and the Marketplace (web, not protocol) are deliberately excluded — this roadmap
covers protocol features only.
