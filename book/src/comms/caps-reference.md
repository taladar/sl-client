# Capability Reference

A reference for the region capabilities the reference viewer (Firestorm)
requests: what each is for, how it is called, and where this client stands.
How capabilities work in general is in [Capabilities](caps.md); which grid
grants which is measured in
[Grid Behaviour → Capabilities](../gridspec/capabilities.md).

This chapter covers the 51 names Firestorm requests that this client did not
when they were surveyed (2026-10-04). Each section names the Firestorm code it
was read from, so a shape can be re-checked there. "Granted" is what the
conformance case `seed-capabilities` measured on Second Life (aditi) and a
stock OpenSim.

The client's rule for every one of them: request it, use it where the region
grants it, and keep the older path (usually a UDP message) where it does not.

## Summary

| capability | SL | OS | status | our client |
| --- | --- | --- | --- | --- |
| `AbuseCategories` | yes | — | current | task `viewer-report-abuse` |
| `AcceptFriendship` | yes | — | current | task `protocol-cap-offline-friendship-answers` (UDP today) |
| `AgentProfile` | yes | — | current | task `protocol-cap-agent-profile` (UDP today) |
| `AgentState` | yes | — | current | task `viewer-pathfinding-floaters` |
| `AvatarRenderInfo` | yes | — | current | task `protocol-avatar-render-info` |
| `CharacterProperties` | yes | — | current | task `viewer-pathfinding-floaters` |
| `DeclineFriendship` | yes | — | current | task `protocol-cap-offline-friendship-answers` (UDP today) |
| `DispatchOpenRegionSettings` | — | — | Aurora-Sim only | not adopted |
| `DispatchRegionInfo` | yes | — | current | task `protocol-cap-dispatch-region-info` (UDP today) |
| `EnvironmentSettings` | yes | yes | deprecated by `ExtEnvironment` | not adopted |
| `EstateAccess` | yes | yes | current | task `protocol-cap-estate-access` (UDP today) |
| `EstateChangeInfo` | — | off | unused | not adopted (UDP) |
| `GetMetadata` | yes | — | current | task `protocol-cap-get-metadata` |
| `GroupAPIv1` | yes | — | current | task `protocol-cap-group-bans` |
| `GroupProposalBallot` | yes | — | removed feature | not adopted |
| `HomeLocation` | yes | yes | current | task `protocol-cap-home-location` (UDP today) |
| `InterestList` | yes | — | current | adopted (`Command::SetInterestListMode`) |
| `InventoryThumbnailUpload` | yes | — | current | task `viewer-inventory-thumbnails` |
| `MapLayer` | yes | yes | unused since viewer 2.0 | not adopted |
| `MapLayerGod` | yes | — | unused | not adopted |
| `MeshUploadFlag` | yes | yes | current | task `viewer-mesh-upload-sequence` |
| `ModifyRegion` | yes | — | current | task `viewer-pbr-terrain` |
| `NavMeshGenerationStatus` | yes | — | current | task `viewer-pathfinding-floaters` |
| `ObjectNavMeshProperties` | yes | — | current | task `viewer-pathfinding-floaters` |
| `ParcelPropertiesUpdate` | yes | yes | current | **used** since 2026-10-05; UDP where not granted |
| `ProductInfoRequest` | yes | — | current | task `protocol-cap-product-info` |
| `RegionObjects` | yes | — | current | task `viewer-pathfinding-floaters` |
| `RegionSchedule` | yes | — | current | task `viewer-region-restart-schedule` |
| `RequestTaskInventory` | yes | — | current | task `protocol-request-task-inventory-cap` (UDP today) |
| `RequestTextureDownload` | yes | — | never used | not adopted |
| `RetrieveNavMeshSrc` | yes | — | current | task `viewer-pathfinding-navmesh-view` |
| `SearchStatRequest` | yes | — | current | task `viewer-classified-click-stats` |
| `SearchStatTracking` | yes | — | telemetry | task `viewer-telemetry-opt-in` |
| `SendPostcard` | yes | — | current | task `viewer-snapshot-postcard` |
| `ServerReleaseNotes` | yes | yes | current | task `viewer-about-release-notes` |
| `SetDisplayName` | yes | — | current | task `viewer-display-name-set` |
| `SimConsoleAsync` | yes | yes | current (admin) | task `viewer-region-debug-console` |
| `SpatialVoiceModerationRequest` | yes | — | current | task `viewer-nearby-voice-moderation` |
| `StartGroupProposal` | yes | — | removed feature | not adopted |
| `TerrainNavMeshProperties` | yes | — | current | task `viewer-pathfinding-floaters` |
| `TextureStats` | yes | — | telemetry (debug) | task `viewer-telemetry-opt-in` |
| `UntrustedSimulatorMessage` | yes | yes | current (god actions) | task `viewer-god-tools` |
| `UpdateAgentInformation` | yes | yes | current | task `protocol-cap-update-agent-information` |
| `UpdateAgentLanguage` | yes | yes | current | task `viewer-i18n-agent-language` |
| `UpdateGestureTaskInventory` | yes | yes | current | task `viewer-gesture-management-ui` |
| `UpdateMaterialTaskInventory` | yes | — | current | task `viewer-material-save-to-object` |
| `UploadAgentProfileImage` | yes | — | current | task `viewer-profile-image-editing` |
| `ViewerBenefits` | yes | — | never used | not adopted |
| `ViewerMetrics` | yes | — | telemetry | task `viewer-telemetry-opt-in` |
| `ViewerStartAuction` | — | — | unused | not adopted (UDP) |
| `ViewerStats` | yes | — | telemetry | task `viewer-telemetry-opt-in` |

Telemetry is opt-in in this client: nothing is reported until the resident
turns it on, on a page that lists every field each report carries.

## The two-step upload

Several capabilities share one upload flow (Firestorm
`llviewerassetupload.cpp`): POST an LLSD body to the capability, which answers
`{state:"upload", uploader:<url>}`; POST the raw bytes to that URL, which
answers `{state:"complete", new_asset:<uuid>}`. Firestorm retries twice, each
time with a fresh uploader. Below, "two-step upload with `{…}`" names the
first body.

## Profile and social

**`AgentProfile`** (`llavatarpropertiesprocessor.cpp`). GET `{cap}/{avatar}` →
`{id, sl_image_id, fl_image_id, partner_id, sl_about_text, fl_about_text,
member_since, hide_age, customer_type, notes, online, allow_publish,
identified, transacted, charter_member | caption, groups:[{id, name,
image_id}], picks:[{id, name}]}`. PUT `{cap}/{agent}` with a one-key partial
map (`sl_about_text`, `fl_about_text`, `sl_image_id`, `fl_image_id`, `notes`,
`allow_publish`, `hide_age`). With it, Firestorm sends no UDP profile request
at all; the UDP notes path is the older, shorter one. Replaces
`AvatarPropertiesRequest` / `Update`, `AvatarNotesUpdate` and the
`avatarnotesrequest` / `avatarpicksrequest` generic messages.

**`UploadAgentProfileImage`** (`llpanelprofile.cpp`). Two-step upload with
`{profile-image-asset:"sl_image_id"|"fl_image_id"}`, the image as JPEG2000
(`application/jp2`). No UDP equivalent.

**`SetDisplayName`** (`llviewerdisplayname.cpp`). POST `{display_name:[old,
new]}` with an `Accept-Language` header; the outcome arrives as the
event-queue `SetDisplayNameReply` `{status, reason, content}`. On 409,
re-fetch the name.

**`AcceptFriendship` / `DeclineFriendship`** (`llviewermessage.cpp`). For
offers delivered offline over `ReadOfflineMsgs` only. Accept: POST
`{cap}?from=<uuid>&agent_name="<name>"`, empty body → `{success}`. Decline:
DELETE `{cap}?from=<uuid>` → `{success}`. Firestorm reads offline messages over
`ReadOfflineMsgs` only when this pair and `AcceptGroupInvite` are all granted,
because cap-delivered offers carry no transaction id for the UDP answers.

**`GroupAPIv1`** (`llgroupmgr.cpp`). Group bans. GET `{cap}?group_id=<id>` →
`{group_id, ban_list:{<agent>:{ban_date}}}`. POST `{cap}?group_id=<id>`
`{ban_action: 1 create | 2 delete, ban_ids:[uuid]}`, then re-fetch.

**`UpdateAgentInformation`** (`llagent.cpp`). The maturity preference: POST
`{access_prefs:{max:"PG"|"M"|"A"}}`, the reply echoing `{access_prefs:{max}}`.

**`UpdateAgentLanguage`** (`llagentlanguage.cpp`). POST `{language,
language_is_public}` at login and on change; the reply is ignored.

**`InterestList`** (`llviewerregion.cpp`). POST `{mode:"default"|"360"}` →
`{mode, previous_mode}`; `360` is meant to stream every object around the agent
instead of only the view frustum. A mode the simulator does not know is
answered as `default`; a DELETE, which Firestorm's "reset interest lists" sends,
got no answer from aditi in 30 s. Firestorm applies the mode to every region
and after each region change, and uses it for 360° snapshots and area search;
so do we, for the 360° capture. Measured in
[Objects](../gridspec/objects.md#the-interest-list-mode).

**`AbuseCategories`** (`llfloaterreporter.cpp`). GET `{cap}[?lc=<language>]` →
`{categories:[{category, description_localized}]}`, replacing the report
floater's static categories.

**`SpatialVoiceModerationRequest`** (`llnearbyvoicemoderation.cpp`). WebRTC
regions. POST `{operand:"mute"|"unmute", agent_id}` to the avatar's region, or
`{operand:"mute_all"|"unmute_all"}` to the agent's own.

## Land, region and estate

**`ParcelPropertiesUpdate`** (`llviewerparcelmgr.cpp`, `llparcel.cpp`). POST
the parcel as LLSD: `local_id, parcel_flags, sale_price, name, description,
music_url, media_url, media_desc, media_type, media_width, media_height,
auto_scale, media_loop, media_current_url, obscure_media, obscure_music,
media_id, media_allow_navigate, media_prevent_camera_zoom, media_url_timeout,
group_id, pass_price, pass_hours, category, auth_buyer_id, snapshot_id,
user_location, user_look_at, landing_type, see_avs, group_av_sounds,
any_av_sounds, obscure_moap`, plus `flags` (`0x01`, "push the parcel back").
`flags` and `parcel_flags` are 4-byte big-endian binary. The reply is ignored;
the grid pushes the parcel back instead (or does not: what each grid answers
is in [Land](../gridspec/land.md)). The UDP `ParcelPropertiesUpdate` lacks
`media_type` and the last four; on OpenSim, leaving `media_type` unset corrupts
the parcel's stored record. The client builds the body in `sl-proto`'s
`build_parcel_properties_update_request`, and the fake grid parses it with
`parse_parcel_properties_update_request`.

**`HomeLocation`** (`llagent.cpp`). POST `{HomeLocation:{LocationId,
LocationPos:{X,Y,Z}, LocationLookAt:{X,Y,Z}}}` → `{success,
HomeLocation:{LocationPos}}`. Replaces `SetStartLocationRequest`.

**`EstateAccess`** (`llfloaterregioninfo.cpp`). GET → `{AllowedAgents:[{id}],
AllowedGroups:[{id}], BannedAgents:[{id, banning_id, last_login_date,
ban_date}], Managers:[{agent_id}]}`. Firestorm re-fetches over it whenever the
UDP `setaccess` reply arrives; edits stay on UDP `estateaccessdelta`.

**`DispatchRegionInfo`** (`llfloaterregioninfo.cpp`). POST `{block_terraform,
block_fly, block_fly_over, allow_damage, allow_land_resell, agent_limit,
prim_bonus, sim_access, restrict_pushobject, allow_parcel_changes,
block_parcel_search}`. The UDP `setregioninfo` has no `block_fly_over` or
`block_parcel_search`.

**`RegionSchedule`** (`llfloaterregionrestartschedule.cpp`). GET →
`{restart:{type:"W"|"D", days:"MTWRFSU subset", time:<seconds after
midnight>}}`, no `restart` meaning none. POST the same shape; empty `days`
with type `W` resets.

**`ModifyRegion`** (`llpbrterrainfeatures.cpp`). PBR terrain transforms. GET on
every region handshake → `{success, message?, overrides:[4 GLTF material
overrides]}`; POST `{overrides:[4]}`.

**`ProductInfoRequest`** (`llproductinforequest.cpp`). GET → `[{sku, name,
description}]`, mapping a land `ProductSKU` to a land-type name. The SKU only
arrives in the LLSD forms of `DirLandReply` / `PlacesReply`.

**`ServerReleaseNotes`** (`llfloaterabout.cpp`). GET with redirects off; the
`Location:` header is the release-notes URL.

## Inventory, assets and objects

**`RequestTaskInventory`** (`llviewerobject.cpp`). GET
`{cap}?task_id=<object>[&inventory_serial=<n>]` → `{inventory_serial,
contents:[items]}`, without the "Contents" folder; 304 when unchanged.
Replaces UDP `RequestTaskInventory` → `ReplyTaskInventory` → Xfer.

**`UpdateGestureTaskInventory`**, **`UpdateMaterialTaskInventory`**
(`llpreviewgesture.cpp`, `llmaterialeditor.cpp`). Two-step upload with
`{task_id, item_id}`: saving a gesture or a material held in an object.

**`InventoryThumbnailUpload`** (`llfloatersimplesnapshot.cpp`). Two-step upload
with `{item_id}`, `{category_id}` or `{item_id, task_id}`, the image as
JPEG2000, 64–256 px.

**`GetMetadata`** (`llexperiencecache.cpp`). POST `{"object-id", "item-id",
"fields":["experience"]}` → `{experience}`: a script's experience.

**`MeshUploadFlag`** (`llfloatermodeluploadbase.cpp`). GET →
`{mesh_upload_status}`; anything but `valid` or empty disables mesh upload.

**`SendPostcard`** (`llpanelsnapshotpostcard.cpp`). Two-step upload with
`{pos-global, to, name, subject, msg}` (and `from` off Second Life), the JPEG
snapshot as the second step. Firestorm never sends the UDP `SendPostcard`.

**`AvatarRenderInfo`** (`llavatarrenderinfoaccountant.cpp`). GET every 15 s →
`{agents:{<id>:{weight}}, reportinglimit, overlimit}`; POST every 60 s
`{agents:{<id>:{weight, tooComplex}}}`. Together they drive the "N residents
render you as a jellydoll" notice.

**`UntrustedSimulatorMessage`** (`message.cpp`). A transport: POST `{message,
body}` for messages flagged as LLSD in `message.xml`; in practice
`GodKickUser` (god kick, freeze, unfreeze).

**`SimConsoleAsync`** (`llfloaterregiondebugconsole.cpp`). POST the command as
an LLSD string; output arrives as the event-queue `SimConsoleResponse`.

## Pathfinding

All Second Life only (`llpathfindingmanager.cpp`). The presence of
`RetrieveNavMeshSrc` is Firestorm's "pathfinding enabled" test.

- **`NavMeshGenerationStatus`**: GET → `{region_id, status, version}`; POST
  `{command:"rebuild"}` rebakes.
- **`AgentState`**: GET → `{can_modify_navmesh}`; also pushed as the
  event-queue `AgentStateUpdate`.
- **`RetrieveNavMeshSrc`**: POST an empty map → `{navmesh_version,
  navmesh_data}`. Firestorm decodes the data only with the closed Havok
  library.
- **`RegionObjects`**: GET → linksets keyed by object id: `{name, description,
  owner, owner_is_group, position, landimpact, modifiable, navmesh_category,
  can_be_volume, is_scripted, phantom, A, B, C, D}`.
- **`ObjectNavMeshProperties`**: PUT the changed linkset fields (`phantom`,
  `navmesh_category` 0 include / 1 exclude / 2 ignore, `A`–`D`).
- **`TerrainNavMeshProperties`**: GET / PUT the terrain entry; Firestorm
  refuses linkset edits without it.
- **`CharacterProperties`**: GET → characters with `cpu_time`, `horizontal`,
  `length`, `radius`.

## Telemetry

Sent by this client only when the resident opts in.

- **`ViewerStats`** (`llviewerstats.cpp`): POST every 300 s and at quit — run
  time, frame rates, ping, distance travelled, regions visited, memory, OS,
  CPU, RAM, GPU and driver, download totals, network totals and failure
  counters.
- **`ViewerMetrics`** (`llappviewer.cpp`, `llvoavatarself.cpp`): asset-fetch
  metrics every 600 s (per region and asset type: counts, response times) and
  appearance metrics every 300 s (rez status, decloud times, nearby avatars).
- **`TextureStats`** (`lltexturestats.cpp`): cache statistics; Firestorm sends
  it only with a debug setting.
- **`SearchStatTracking`** (`llpanelclassified.cpp`): POST `{type, from_search,
  classified_id, parcel_id, dest_pos_global, region_name}` on opening, mapping
  or teleporting to a classified.

**`SearchStatRequest`** is the read side and not telemetry: POST
`{classified_id}` → the six click counters shown on the owner's own
classified.

## Not adopted

| capability | why |
| --- | --- |
| `ViewerBenefits`, `RequestTextureDownload` | Firestorm requests them and never calls them |
| `StartGroupProposal`, `GroupProposalBallot` | group voting was removed from Second Life |
| `EnvironmentSettings` | the pre-EEP sky API; `ExtEnvironment` replaced it |
| `MapLayer`, `MapLayerGod` | unused since viewer 2.0; the world map uses tiles |
| `DispatchOpenRegionSettings` | Aurora-Sim only; neither grid serves it |
| `EstateChangeInfo` | Second Life refuses it and OpenSim ships it disabled; estate edits use UDP |
| `ViewerStartAuction` | refused by Second Life; the god auction goes over UDP |
