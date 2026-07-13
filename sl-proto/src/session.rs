//! The sans-I/O session state machine: login, circuit establishment,
//! keep-alive, and clean logout, driven entirely by passed-in time.

use crate::bookkeeping_ids::{PingId, XferId};
use crate::scoped_id::CircuitId;
use crate::types::{
    Camera, Diagnostic, Event, Friend, ImageCodec, LoginAccount, LoginParams, Object, TerrainPatch,
    Throttle,
};
use sl_types::key::{AgentKey, ExperienceKey, FriendKey, InventoryKey, ObjectKey};
use sl_types::lsl::Rotation;
use sl_types::lsl::ScriptPermissions;
use sl_types::map::Distance;
use sl_wire::CircuitCode;
use sl_wire::ControlFlags;
use sl_wire::RegionHandle;
use sl_wire::RegionLocalObjectId;
use sl_wire::SequenceNumber;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// How often an `AgentUpdate` is sent to keep the agent active.
const AGENT_UPDATE_INTERVAL: Duration = Duration::from_millis(1000);
/// How often a keep-alive `StartPingCheck` is sent on the root circuit to
/// measure the round-trip time to the simulator, matching the reference
/// viewer's circuit ping cadence (`LLCircuit`'s ~5-second periodic ping). The
/// simulator answers each with a `CompletePingCheck` echoing the ping id, which
/// the session times to surface an [`Event::Ping`](crate::Event::Ping).
const PING_INTERVAL: Duration = Duration::from_secs(5);
/// How long owed acknowledgements may wait before being flushed as a `PacketAck`.
const ACK_FLUSH_DELAY: Duration = Duration::from_millis(150);
/// How long without inbound traffic before the link is considered dead. Kept
/// well under OpenSim's 60-second `AckTimeout`.
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(45);
/// How long to wait for a `LogoutReply` before giving up on a clean logout.
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(5);
/// The retransmission timeout for an unacknowledged reliable packet.
const RESEND_TIMEOUT: Duration = Duration::from_millis(1500);
/// The maximum number of times a reliable packet is sent before giving up.
const MAX_RESEND_ATTEMPTS: u32 = 6;
/// The maximum number of inbound reliable sequence numbers remembered for
/// duplicate suppression.
const SEEN_CAPACITY: usize = 4096;
/// The maximum number of acknowledgements packed into a single `PacketAck`.
const MAX_ACKS_PER_PACKET: usize = 255;
/// How long to wait for a `TeleportFinish` before declaring the teleport failed.
const TELEPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for an `AvatarSitResponse` before giving up on a sit
/// request and surfacing a [`Diagnostic::ExpectedReplyMissing`](crate::Diagnostic::ExpectedReplyMissing).
const SIT_TIMEOUT: Duration = Duration::from_secs(15);
/// The default draw distance (metres) advertised in keep-alive `AgentUpdate`s,
/// large enough that the simulator enables the neighbouring regions.
const DEFAULT_DRAW_DISTANCE: Distance = Distance::new(256.0);
/// The identity (no-op) rotation: the default body/head facing.
pub(crate) const IDENTITY_ROTATION: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// The HTTP capability for fetching inventory folder contents (a POST of an LLSD
/// folder list). Used as the seed capability name, the request cap, and the
/// message tag a driver feeds back via [`Session::handle_caps_event`].
pub const CAP_FETCH_INVENTORY: &str = "FetchInventoryDescendents2";

/// The HTTP capability for fetching **shared Library** folder contents — the same
/// request/response shape as [`CAP_FETCH_INVENTORY`] but POSTed with the Library
/// owner id so the read-only Library tree (held under
/// [`InventoryOwner::Library`](crate::InventoryOwner)) is fetched separately from
/// the agent's own inventory. Stock OpenSim does not serve this cap (the Library
/// is reachable there only over the UDP `FetchInventoryDescendents` path); it is a
/// Second-Life capability.
pub const CAP_FETCH_LIBRARY: &str = "FetchLibDescendents2";

/// The HTTP capability for fetching a group's full member roster (a POST of an
/// LLSD `{ group_id }` map — the modern Second Life path that replaces the UDP
/// `GroupMembersRequest`/`Reply`). The LLSD response is decoded by
/// [`Session::handle_caps_event`] into [`Event::GroupMembers`].
pub const CAP_GROUP_MEMBER_DATA: &str = "GroupMemberData";

/// The HTTP capability for fetching a texture by UUID (an HTTP `GET` of
/// `?texture_id=<uuid>`, returning a `.j2c` codestream). The modern Second Life
/// path that replaces the legacy UDP `RequestImage`/`ImageData` stream; the
/// driver fetches it and surfaces an [`Event::TextureReceived`].
pub const CAP_GET_TEXTURE: &str = "GetTexture";

/// The HTTP capability for fetching a mesh asset by UUID (an HTTP `GET` of
/// `?mesh_id=<uuid>`). Surfaces as an [`Event::AssetReceived`].
pub const CAP_GET_MESH: &str = "GetMesh";

/// The newer HTTP capability for fetching a mesh asset by UUID, preferred over
/// [`CAP_GET_MESH`] when offered.
pub const CAP_GET_MESH2: &str = "GetMesh2";

/// The HTTP capability for fetching a generic asset by UUID and class (an HTTP
/// `GET` of `?<class>_id=<uuid>`, e.g. `?sound_id=`/`?animatn_id=`). The modern
/// path that replaces the legacy UDP `TransferRequest` for every asset class;
/// surfaces as an [`Event::AssetReceived`]. Both Second Life and OpenSim expose
/// it under the name `ViewerAsset`.
pub const CAP_VIEWER_ASSET: &str = "ViewerAsset";

/// The HTTP capability for the modern Second Life **server-side appearance bake**
/// ("Sunshine" / central baking): a POST of an LLSD `{ "cof_version": <int> }`
/// map asking the grid's bake service to composite the agent's current outfit.
/// On a baking-capable region the client no longer computes or uploads baked
/// textures itself (the legacy `AgentSetAppearance` / `UploadBakedTexture`
/// path); it manages the Current Outfit Folder in inventory and triggers this
/// capability, after which the server broadcasts the resulting baked-texture ids
/// to every viewer via the UDP `AvatarAppearance` ([`Event::AvatarAppearance`]).
/// The POST's own LLSD reply (`{ success, error?, expected? }`) is surfaced as
/// [`Event::ServerAppearanceUpdate`]. Driven by the runtimes'
/// `RequestServerAppearanceUpdate` command (an HTTP POST, like the inventory
/// and group-roster capabilities), whose LLSD reply is decoded by
/// [`Session::handle_caps_event`].
pub const CAP_UPDATE_AVATAR_APPEARANCE: &str = "UpdateAvatarAppearance";

/// The HTTP capability for the modern asset upload: storing a new asset **and**
/// creating an inventory item for it (`NewFileAgentInventory`). A two-step
/// uploader — the driver POSTs the LLSD metadata (folder, asset/inventory type,
/// name, permissions, expected cost) and receives an `uploader` URL, then POSTs
/// the raw asset bytes there and receives `{ new_asset, new_inventory_item }`.
/// Surfaced as [`Event::AssetUploaded`] (or [`Event::AssetUploadFailed`]).
pub const CAP_NEW_FILE_AGENT_INVENTORY: &str = "NewFileAgentInventory";

/// The HTTP capability for uploading a client-computed **baked avatar texture**
/// (`UploadBakedTexture`): the legacy (pre-server-side-bake) appearance path.
/// Same two-step uploader as [`CAP_NEW_FILE_AGENT_INVENTORY`] but the metadata
/// POST is an empty map and the result is a *temporary* asset with no inventory
/// item (`new_inventory_item` is nil → `None`).
pub const CAP_UPLOAD_BAKED_TEXTURE: &str = "UploadBakedTexture";

/// The HTTP capability for replacing the asset of an existing **gesture**
/// inventory item (`UpdateGestureAgentInventory`). Two-step uploader; the
/// metadata POST carries the `item_id`. See also
/// [`AssetType::update_item_cap`](crate::AssetType::update_item_cap) for the
/// notecard / script / settings equivalents.
pub const CAP_UPDATE_GESTURE_AGENT_INVENTORY: &str = "UpdateGestureAgentInventory";

/// The HTTP capability for replacing the asset of an existing **notecard**
/// inventory item (`UpdateNotecardAgentInventory`). Two-step uploader carrying
/// the `item_id`.
pub const CAP_UPDATE_NOTECARD_AGENT_INVENTORY: &str = "UpdateNotecardAgentInventory";

/// The HTTP capability for replacing the source of an existing **script**
/// inventory item in the agent's own inventory (`UpdateScriptAgent`). Two-step
/// uploader carrying the `item_id` and a compile `target`; the completion reply
/// carries the simulator's compile result (`compiled` + `errors`).
pub const CAP_UPDATE_SCRIPT_AGENT: &str = "UpdateScriptAgent";

/// The HTTP capability for replacing the source of a **script** inside an
/// in-world object's task inventory (`UpdateScriptTask`). Two-step uploader
/// carrying `task_id`/`item_id`, `is_script_running`, a compile `target`, and an
/// optional `experience`; the completion reply carries the compile result.
pub const CAP_UPDATE_SCRIPT_TASK: &str = "UpdateScriptTask";

/// The HTTP capability for replacing the asset of an existing **settings**
/// inventory item (`UpdateSettingsAgentInventory`). Two-step uploader carrying
/// the `item_id`.
pub const CAP_UPDATE_SETTINGS_AGENT_INVENTORY: &str = "UpdateSettingsAgentInventory";

/// The HTTP capability for the **media-on-a-prim** read/write surface
/// (`ObjectMedia`): a POST of a `{ verb, object_id, … }` map. A `GET` verb asks
/// for an object's current per-face media (the simulator replies with an
/// `object_media_data` array decoded into [`Event::ObjectMedia`]); an `UPDATE`
/// verb sets it. Driven by the runtimes' `RequestObjectMedia` /
/// `SetObjectMedia` commands; the GET reply is decoded by
/// [`Session::handle_caps_event`].
pub const CAP_OBJECT_MEDIA: &str = "ObjectMedia";

/// The seed capability that advertises animesh support (`ObjectAnimation`).
///
/// Despite the name, this capability is **never fetched or POSTed** — an animated
/// object's animation state arrives as the UDP `ObjectAnimation` message
/// ([`Event::ObjectAnimation`](crate::Event::ObjectAnimation)), not over HTTP.
/// Listing it in the seed-capabilities request is how a viewer tells the simulator
/// it can render animesh; a simulator withholds the `ObjectAnimation` UDP stream
/// from a viewer that did not request the capability, so an animated object stays
/// frozen at its rest pose (the reference viewer lists it in
/// `LLViewerRegionImpl::buildCapabilityNames`). Kept in
/// [`REQUESTED_CAPABILITIES`] purely to opt in to that stream.
pub const CAP_OBJECT_ANIMATION: &str = "ObjectAnimation";

/// The HTTP capability for navigating the media on a single prim face to a new
/// URL (`ObjectMediaNavigate`): a POST of a `{ object_id, current_url,
/// texture_index }` map. Driven by the runtimes' `NavigateObjectMedia` command;
/// the simulator advances the object's media version (visible on a subsequent
/// [`CAP_OBJECT_MEDIA`] GET) rather than replying with media data.
pub const CAP_OBJECT_MEDIA_NAVIGATE: &str = "ObjectMediaNavigate";

/// The HTTP capability for the legacy (normal/specular) **materials** surface
/// (`RenderMaterials`): a POST of a `{ "Zipped": <binary> }` map whose binary is
/// the zlib-compressed binary-LLSD array of the material ids to fetch. The
/// simulator replies with the matching materials (decoded into
/// [`Event::RenderMaterials`]). This is the path stock OpenSim implements.
/// Driven by the runtimes' `RequestRenderMaterials` command.
pub const CAP_RENDER_MATERIALS: &str = "RenderMaterials";

/// The HTTP capability for setting **GLTF (PBR) materials** on object faces
/// (`ModifyMaterialParams`): a POST of an array of `{ object_id, side,
/// gltf_json?, asset_id? }` maps. Driven by the runtimes' `ModifyMaterialParams`
/// command; the `{ success, message }` reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::MaterialParamsResult`].
pub const CAP_MODIFY_MATERIAL_PARAMS: &str = "ModifyMaterialParams";

/// The HTTP capability for replacing the asset of an existing **material**
/// inventory item (`UpdateMaterialAgentInventory`). Two-step uploader carrying
/// the `item_id` (see [`AssetType::update_item_cap`](crate::AssetType::update_item_cap)).
pub const CAP_UPDATE_MATERIAL_AGENT_INVENTORY: &str = "UpdateMaterialAgentInventory";

/// The HTTP capability for obtaining voice-chat account credentials
/// (`ProvisionVoiceAccountRequest`): a POST that returns either the legacy Vivox
/// SIP account (`{ username, password, voice_sip_uri_hostname,
/// voice_account_server_name }`) or, for the modern WebRTC path, a JSEP answer
/// SDP plus a viewer session. Driven by the runtimes' `RequestVoiceAccount`
/// command; the reply is decoded by [`Session::handle_caps_event`] into
/// [`Event::VoiceAccountProvisioned`]. Only the *signalling* is handled — the
/// audio transport (Vivox SIP/RTP or a WebRTC peer connection) is out of scope.
pub const CAP_PROVISION_VOICE_ACCOUNT: &str = "ProvisionVoiceAccountRequest";

/// The HTTP capability for a parcel's voice channel (`ParcelVoiceInfoRequest`):
/// a POST (empty body) that returns `{ parcel_local_id, region_name,
/// voice_credentials: { channel_uri } }`. Driven by the runtimes'
/// `RequestParcelVoiceInfo` command; the reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::ParcelVoiceInfo`].
pub const CAP_PARCEL_VOICE_INFO: &str = "ParcelVoiceInfoRequest";

/// The HTTP capability for WebRTC ICE-candidate trickling
/// (`VoiceSignalingRequest`): a fire-and-forget POST of the gathered candidates
/// (or an end-of-gathering marker) keyed by the viewer session from the
/// provision reply. Driven by the runtimes' `SendVoiceSignaling` command; the
/// simulator returns only an HTTP status, so there is no surfaced event. WebRTC
/// only (Vivox does not use it).
pub const CAP_VOICE_SIGNALING: &str = "VoiceSignalingRequest";

/// The HTTP capability for batch experience-metadata lookup (`GetExperienceInfo`):
/// a GET of `…/id/?page_size=N&public_id=<id>&…` returning `{ experience_keys,
/// error_ids }`. Driven by the runtimes' `RequestExperienceInfo` command; the
/// reply is decoded by [`Session::handle_caps_event`] into [`Event::ExperienceInfo`].
/// Experiences are a Second Life feature (stock OpenSim ships no module).
pub const CAP_GET_EXPERIENCE_INFO: &str = "GetExperienceInfo";

/// The HTTP capability for searching experiences by name (`FindExperienceByName`):
/// a GET of `…?page=N&page_size=M&query=<text>` returning `{ experience_keys }`.
/// Driven by `FindExperiences`; decoded into [`Event::ExperienceSearchResults`].
pub const CAP_FIND_EXPERIENCE_BY_NAME: &str = "FindExperienceByName";

/// The HTTP capability for the agent's admitted/blocked experiences
/// (`GetExperiences`): a GET returning `{ experiences, blocked }`. Driven by
/// `RequestExperiencePermissions`; decoded into [`Event::ExperiencePermissions`].
pub const CAP_GET_EXPERIENCES: &str = "GetExperiences";

/// The HTTP capability for the experiences the agent owns (`AgentExperiences`): a
/// GET returning `{ experience_ids }`. Driven by `RequestOwnedExperiences`;
/// decoded into [`Event::OwnedExperiences`].
pub const CAP_AGENT_EXPERIENCES: &str = "AgentExperiences";

/// The HTTP capability for the experiences the agent administers
/// (`GetAdminExperiences`): a GET returning `{ experience_ids }`. Driven by
/// `RequestAdminExperiences`; decoded into [`Event::AdminExperiences`].
pub const CAP_GET_ADMIN_EXPERIENCES: &str = "GetAdminExperiences";

/// The HTTP capability for the experiences the agent created
/// (`GetCreatorExperiences`): a GET returning `{ experience_ids }`. Driven by
/// `RequestCreatorExperiences`; decoded into [`Event::CreatorExperiences`].
pub const CAP_GET_CREATOR_EXPERIENCES: &str = "GetCreatorExperiences";

/// The HTTP capability for the experiences a group owns (`GroupExperiences`): a
/// GET of `…?<group_id>` returning `{ experience_ids }`. Driven by
/// `RequestGroupExperiences`; the runtime tags the reply with the queried group
/// to build [`Event::GroupExperiences`].
pub const CAP_GROUP_EXPERIENCES: &str = "GroupExperiences";

/// The HTTP capability for the agent's per-experience preferences
/// (`ExperiencePreferences`): an `Allow`/`Block` PUT of `{ "<id>": { permission }
/// }`, or a `Forget` DELETE of `…?<id>`; both reply `{ experiences, blocked }`.
/// Driven by `SetExperiencePermission`; decoded into [`Event::ExperiencePermissions`].
pub const CAP_EXPERIENCE_PREFERENCES: &str = "ExperiencePreferences";

/// The HTTP capability testing whether the agent is an admin of an experience
/// (`IsExperienceAdmin`): a GET of `…?experience_id=<id>` returning `{ status }`.
/// Driven by `RequestExperienceAdmin`; the runtime tags the reply with the queried
/// experience to build [`Event::ExperienceAdminStatus`].
pub const CAP_IS_EXPERIENCE_ADMIN: &str = "IsExperienceAdmin";

/// The HTTP capability testing whether the agent contributes to an experience
/// (`IsExperienceContributor`): a GET of `…?experience_id=<id>` returning `{
/// status }`. Driven by `RequestExperienceContributor`; the runtime tags the
/// reply with the queried experience to build [`Event::ExperienceContributorStatus`].
pub const CAP_IS_EXPERIENCE_CONTRIBUTOR: &str = "IsExperienceContributor";

/// The HTTP capability for editing an experience's metadata (`UpdateExperience`):
/// a POST of the editable fields returning the updated experience. Driven by
/// `UpdateExperience`; decoded into [`Event::ExperienceUpdated`].
pub const CAP_UPDATE_EXPERIENCE: &str = "UpdateExperience";

/// The HTTP capability for the region's experience allow/block/trust lists
/// (`RegionExperiences`): a GET to read, or a POST of `{ allowed, blocked,
/// trusted }` to update (estate-gated); both reply with the three lists. Driven by
/// `RequestRegionExperiences` / `SetRegionExperiences`; decoded into
/// [`Event::RegionExperiences`].
pub const CAP_REGION_EXPERIENCES: &str = "RegionExperiences";

/// Completing the IM surface (#28): the modern Second Life capability that
/// returns the agent's stored offline instant messages as an LLSD array (the
/// legacy path is the UDP `RetrieveInstantMessages` trigger). GET; decoded into
/// one [`Event::InstantMessageReceived`] per stored message.
pub const CAP_READ_OFFLINE_MSGS: &str = "ReadOfflineMsgs";

/// Completing the IM surface (#28): the modern Second Life capability for
/// accepting or declining a chat-session invitation. A POST of `{ "method":
/// "accept invitation" | "decline invitation", "session-id": <uuid> }`; the
/// `"accept invitation"` reply body is the session's current agent roster (the
/// modern equivalent of replaying the UDP `SessionAdd` stream). OpenSim stubs this
/// cap (returns `<llsd>true</llsd>`), so the UDP `SessionLeave` fallback is what
/// the local grid exercises for a decline. Decoded by the runtimes' chat-invite
/// commands; the roster reply seeds the session participants.
pub const CAP_CHAT_SESSION_REQUEST: &str = "ChatSessionRequest";

/// The `ChatSessionRequest` method that accepts (joins) a chat-session invitation,
/// for both text and voice channels (the voice-join signalling is layered on top
/// — see chat task B8). The reply carries the session's current agent roster.
pub const CHAT_SESSION_ACCEPT: &str = "accept invitation";

/// The `ChatSessionRequest` method that declines (refuses) a multi-agent
/// chat-session invitation, text or voice.
pub const CHAT_SESSION_DECLINE: &str = "decline invitation";

/// The `ChatSessionRequest` method that declines / leaves a **1:1 P2P** voice
/// call (a `Direct` session's voice channel), as distinct from the multi-agent
/// [`CHAT_SESSION_DECLINE`] (Firestorm `llimview.cpp` voice-call teardown).
pub const CHAT_SESSION_DECLINE_P2P_VOICE: &str = "decline p2p voice";

/// Inventory mutation (#30): the modern Second Life **AIS3** REST inventory
/// capability (`InventoryAPIv3`). Folder/item create/update/move/remove are HTTP
/// verbs against path suffixes under this base URL (see `sl_wire::inventory`).
/// Served only by Second Life; stock OpenSim ships no AIS3 cap. Replies are
/// decoded by [`Session::handle_caps_event`] into [`Event::InventoryBulkUpdate`].
pub const CAP_INVENTORY_API_V3: &str = "InventoryAPIv3";

/// Inventory mutation (#30): the AIS3 capability for the *library* inventory
/// (read-only, same REST shape as [`CAP_INVENTORY_API_V3`]). Second-Life only.
pub const CAP_LIBRARY_API_V3: &str = "LibraryAPIv3";

/// Inventory mutation (#30): the `CreateInventoryCategory` capability — a folder
/// create that returns a synchronous `{ folder_id, name, parent_id, type }`
/// reply (unlike the no-reply UDP `CreateInventoryFolder`). Served by **both**
/// OpenSim and Second Life. Decoded into [`Event::InventoryBulkUpdate`].
pub const CAP_CREATE_INVENTORY_CATEGORY: &str = "CreateInventoryCategory";

/// The HTTP capability for the Extended Environment (EEP): an HTTP `GET` of
/// `?parcelid=<id>` (or `-1` for the region) returns the region/parcel sky,
/// water, and day-cycle settings as LLSD. Decoded into [`Event::Environment`].
pub const CAP_EXT_ENVIRONMENT: &str = "ExtEnvironment";

/// The HTTP capability for batch **display-name** resolution (`GetDisplayNames`):
/// a GET of `…?ids=<id>&ids=<id>&…` returning `{ agents, bad_ids }`. Driven by the
/// runtimes' `RequestDisplayNames` command; the reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::DisplayNames`]. Complements the
/// always-present UDP legacy-name lookup ([`Session::request_avatar_names`]);
/// stock OpenSim serves it only when its user-management component is present.
pub const CAP_GET_DISPLAY_NAMES: &str = "GetDisplayNames";

/// The HTTP capability that resolves a region location to a grid-wide **parcel
/// id** (`RemoteParcelRequest`): a POST of `{ location, region_id | region_handle
/// }` returning `{ parcel_id }`. Driven by the runtimes' `RequestRemoteParcelId`
/// command; the reply is decoded by [`Session::handle_caps_event`] into
/// [`Event::RemoteParcelId`]. The resolved id then feeds a UDP `ParcelInfoRequest`
/// ([`Session::request_parcel_info`]) for the parcel's listing.
pub const CAP_REMOTE_PARCEL_REQUEST: &str = "RemoteParcelRequest";

/// The HTTP capability for the region's **feature flags** (`SimulatorFeatures`):
/// a GET returning the simulator's mesh/physics/attachment/GLTF switches and
/// limits (plus, on OpenSim, a nested `OpenSimExtras` map). The runtimes GET it
/// automatically once the capability map is known (at login and on each region
/// change) and also on demand via the `RequestSimulatorFeatures` command; the
/// reply is decoded by [`Session::handle_caps_event`] into
/// [`Event::SimulatorFeatures`].
pub const CAP_SIMULATOR_FEATURES: &str = "SimulatorFeatures";

/// The HTTP capability for the agent's **server-stored preferences**
/// (`AgentPreferences`): a POST of the fields to change (hover height, default
/// object permission masks, maturity-access ceiling, UI language) returning the
/// full stored set. Driven by the runtimes' `SetAgentPreferences` /
/// `RequestAgentPreferences` commands; the reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::AgentPreferences`].
pub const CAP_AGENT_PREFERENCES: &str = "AgentPreferences";

/// The HTTP capability for an object's **land-impact / physics costs**
/// (`GetObjectCost`): a POST of `{ object_ids }` returning the per-object resource
/// and physics costs. Driven by the runtimes' `RequestObjectCost` command; the
/// reply is decoded by [`Session::handle_caps_event`] into [`Event::ObjectCosts`].
pub const CAP_GET_OBJECT_COST: &str = "GetObjectCost";

/// The HTTP capability for the **current selection's summed costs**
/// (`ResourceCostSelected`): a POST of `{ selected_roots | selected_prims }`
/// returning `{ selected: { physics, streaming, simulation } }`. Driven by the
/// runtimes' `RequestSelectedCost` command; the reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::SelectedResourceCost`].
pub const CAP_RESOURCE_COST_SELECTED: &str = "ResourceCostSelected";

/// The HTTP capability for an object's **physics-material parameters**
/// (`GetObjectPhysicsData`): a POST of `{ object_ids }` returning each object's
/// physics shape type, density, friction, restitution, and gravity multiplier.
/// Driven by the runtimes' `RequestObjectPhysicsData` command; the reply is
/// decoded by [`Session::handle_caps_event`] into [`Event::ObjectPhysicsData`].
/// The simulator also pushes the same data unsolicited via the `ObjectPhysicsProperties`
/// event-queue event ([`Event::ObjectPhysicsProperties`]).
pub const CAP_GET_OBJECT_PHYSICS_DATA: &str = "GetObjectPhysicsData";

/// The HTTP capability for the agent's **attachment resource report**
/// (`AttachmentResources`): a GET returning the agent's scripted attachments
/// grouped by attachment point, with a resource summary. Driven by the runtimes'
/// `RequestAttachmentResources` command; the reply is decoded by
/// [`Session::handle_caps_event`] into [`Event::AttachmentResources`].
pub const CAP_ATTACHMENT_RESOURCES: &str = "AttachmentResources";

/// The HTTP capability for a parcel's **script resource report** (`LandResources`):
/// a POST of `{ parcel_id }` returning follow-up `ScriptResourceSummary` /
/// `ScriptResourceDetails` capability URLs. Driven by the runtimes'
/// `RequestLandResources` command; the URL hand-off is decoded by
/// [`Session::handle_caps_event`] into [`Event::LandResourcesUrls`], and the
/// runtimes then GET the follow-up URLs, surfacing
/// [`Event::LandResourceSummary`] / [`Event::LandResourceDetail`].
pub const CAP_LAND_RESOURCES: &str = "LandResources";

/// The tag the runtimes attach to a `LandResources` *summary* follow-up GET when
/// forwarding its body to [`Session::handle_caps_event`]. It is the LLSD key the
/// `LandResources` POST returns the follow-up URL under (`ScriptResourceSummary`),
/// not a seed capability — the URL is transient, minted per request.
pub const LAND_RESOURCE_SUMMARY_TAG: &str = "ScriptResourceSummary";

/// The tag the runtimes attach to a `LandResources` *detail* follow-up GET when
/// forwarding its body to [`Session::handle_caps_event`] (`ScriptResourceDetails`).
/// Like [`LAND_RESOURCE_SUMMARY_TAG`], this is a transient per-request URL key,
/// not a seed capability.
pub const LAND_RESOURCE_DETAIL_TAG: &str = "ScriptResourceDetails";

/// The HTTP capability for filing an **abuse / bug report** (`SendUserReport`):
/// a fire-and-forget POST of the report's LLSD body (built by
/// [`build_send_user_report`](sl_wire::build_send_user_report)), the modern
/// equivalent of the legacy `UserReport` UDP message. Driven by the runtimes'
/// `SendAbuseReportViaCaps` command; the simulator returns only an HTTP status,
/// so there is no reply event. Second Life serves it (the
/// `SendUserReportWithScreenshot` variant adds a snapshot —
/// [`CAP_SEND_USER_REPORT_WITH_SCREENSHOT`]); OpenSim implements only the UDP
/// path.
pub const CAP_SEND_USER_REPORT: &str = "SendUserReport";

/// The HTTP capability for filing an **abuse / bug report with a snapshot**
/// (`SendUserReportWithScreenshot`): the screenshot-bearing sibling of
/// [`CAP_SEND_USER_REPORT`]. It is the modern two-step asset uploader (the same
/// `{ state, uploader, … }` flow as [`CAP_NEW_FILE_AGENT_INVENTORY`]) — the
/// runtimes POST the report's LLSD body (with
/// [`screenshot_id`](sl_wire::AbuseReport::screenshot_id) set to a fresh texture
/// asset id) to obtain an `uploader` URL, then PUT the snapshot's JPEG-2000
/// bytes there. Driven by the runtimes' `SendAbuseReportViaCaps` command when a
/// screenshot is supplied; like the no-screenshot path the simulator returns
/// only an HTTP status, so there is no reply event. Second Life only; OpenSim
/// has no abuse-report cap at all. Cross-checked against the Firestorm viewer's
/// `llfloaterreporter.cpp` `sendReportViaCaps` / `LLARScreenShotUploader`.
pub const CAP_SEND_USER_REPORT_WITH_SCREENSHOT: &str = "SendUserReportWithScreenshot";

/// The capability names the client requests from the region seed. A driver POSTs
/// these to the seed URL to obtain the capability map, then uses `EventQueueGet`
/// for the event-queue long-poll, [`CAP_FETCH_INVENTORY`] for inventory fetches,
/// [`CAP_GROUP_MEMBER_DATA`] for group rosters, the asset/texture/mesh caps
/// ([`CAP_GET_TEXTURE`], [`CAP_GET_MESH`], [`CAP_GET_MESH2`], [`CAP_VIEWER_ASSET`])
/// for the HTTP asset-fetch pipeline, and the upload caps
/// ([`CAP_NEW_FILE_AGENT_INVENTORY`], [`CAP_UPLOAD_BAKED_TEXTURE`], and the
/// `Update*AgentInventory` family) for the HTTP asset-upload pipeline.
pub const REQUESTED_CAPABILITIES: &[&str] = &[
    "EventQueueGet",
    CAP_FETCH_INVENTORY,
    CAP_FETCH_LIBRARY,
    CAP_GROUP_MEMBER_DATA,
    CAP_GET_TEXTURE,
    CAP_GET_MESH,
    CAP_GET_MESH2,
    CAP_VIEWER_ASSET,
    CAP_UPDATE_AVATAR_APPEARANCE,
    CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SCRIPT_TASK,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_OBJECT_ANIMATION,
    CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE,
    CAP_RENDER_MATERIALS,
    CAP_MODIFY_MATERIAL_PARAMS,
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_PROVISION_VOICE_ACCOUNT,
    CAP_PARCEL_VOICE_INFO,
    CAP_VOICE_SIGNALING,
    CAP_GET_EXPERIENCE_INFO,
    CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_EXPERIENCES,
    CAP_AGENT_EXPERIENCES,
    CAP_GET_ADMIN_EXPERIENCES,
    CAP_GET_CREATOR_EXPERIENCES,
    CAP_GROUP_EXPERIENCES,
    CAP_EXPERIENCE_PREFERENCES,
    CAP_IS_EXPERIENCE_ADMIN,
    CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_UPDATE_EXPERIENCE,
    CAP_REGION_EXPERIENCES,
    CAP_READ_OFFLINE_MSGS,
    CAP_CHAT_SESSION_REQUEST,
    CAP_INVENTORY_API_V3,
    CAP_LIBRARY_API_V3,
    CAP_CREATE_INVENTORY_CATEGORY,
    CAP_EXT_ENVIRONMENT,
    CAP_GET_DISPLAY_NAMES,
    CAP_REMOTE_PARCEL_REQUEST,
    CAP_SIMULATOR_FEATURES,
    CAP_AGENT_PREFERENCES,
    CAP_GET_OBJECT_COST,
    CAP_RESOURCE_COST_SELECTED,
    CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_ATTACHMENT_RESOURCES,
    CAP_LAND_RESOURCES,
    CAP_SEND_USER_REPORT,
    CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
];

/// The maximum UDP datagram size an I/O driver should be prepared to receive.
///
/// Sized at the theoretical IPv4/UDP payload maximum (64 KiB) so a driver's
/// receive buffer never truncates an inbound datagram.
pub const RECV_BUFFER_SIZE: usize = 0x1_0000;

/// A sensible default bound on the number of inventory folder-contents fetches a
/// background crawler keeps in flight at once, passed by the runtime shells to
/// [`Session::next_inventory_fetch_batch`]. Matches Firestorm's legacy
/// `max_concurrent_fetches = 12` (`LLInventoryModelBackgroundFetch`).
pub const INVENTORY_FETCH_MAX_IN_FLIGHT: usize = 12;

/// Computes `now + duration`, saturating at `now` on (impossible) overflow.
fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

/// Updates `earliest` to the minimum of itself and `candidate`.
fn merge_deadline(earliest: &mut Option<Instant>, candidate: Option<Instant>) {
    if let Some(candidate) = candidate {
        *earliest = Some(match *earliest {
            Some(current) => current.min(candidate),
            None => candidate,
        });
    }
}

/// A reliable packet awaiting acknowledgement.
#[derive(Debug, Clone)]
struct UnackedPacket {
    /// The fully encoded datagram, ready to resend.
    datagram: Vec<u8>,
    /// When the packet was last sent.
    sent_at: Instant,
    /// How many times the packet has been sent so far.
    attempts: u32,
    /// The message name, used to label a [`Diagnostic::ExpectedReplyMissing`]
    /// when the packet exhausts its retransmission budget (`None` for an
    /// unrecognised id).
    ///
    /// [`Diagnostic::ExpectedReplyMissing`]: crate::Diagnostic::ExpectedReplyMissing
    name: Option<&'static str>,
}

/// A bounded set of recently seen inbound reliable sequence numbers, used to
/// suppress duplicate processing of retransmitted reliable packets.
#[derive(Debug, Default)]
struct SeenWindow {
    /// Membership set for O(1) lookup.
    set: HashSet<SequenceNumber>,
    /// Insertion order, for evicting the oldest entries.
    order: VecDeque<SequenceNumber>,
}

impl SeenWindow {
    /// Records `sequence`; returns `true` if it was not seen before.
    fn insert(&mut self, sequence: SequenceNumber) -> bool {
        if !self.set.insert(sequence) {
            return false;
        }
        self.order.push_back(sequence);
        if self.order.len() > SEEN_CAPACITY
            && let Some(evicted) = self.order.pop_front()
        {
            self.set.remove(&evicted);
        }
        true
    }
}

/// The per-connection timers, expressed as absolute deadlines.
#[derive(Debug)]
struct Timers {
    /// When the link is declared dead for lack of inbound traffic.
    inactivity: Instant,
    /// When to flush owed acknowledgements, if any are pending.
    ack_flush: Option<Instant>,
    /// When to send the next `AgentUpdate`, once the session is active.
    agent_update: Option<Instant>,
    /// When to send the next keep-alive `StartPingCheck`, once the session is
    /// active. Armed on the root circuit at region arrival.
    ping: Option<Instant>,
    /// When to give up waiting for a `LogoutReply`, once logging out.
    logout: Option<Instant>,
    /// When to give up waiting for a `TeleportFinish`, once teleporting.
    teleport: Option<Instant>,
    /// When to give up waiting for an `AvatarSitResponse`, once a sit was
    /// requested.
    sit: Option<Instant>,
}

/// An in-flight legacy UDP texture download (`RequestImage` →
/// `ImageData`/`ImagePacket`). The first packet (`ImageData`) carries the codec,
/// total size and packet count plus packet 0's data; subsequent `ImagePacket`s
/// carry packets `1..`. Packets are buffered by index so an out-of-order arrival
/// still reassembles correctly.
#[derive(Debug)]
struct TextureDownload {
    /// The codec reported by the `ImageData` header.
    codec: ImageCodec,
    /// The total number of packets, from the `ImageData` header.
    packets: u16,
    /// The received packet payloads, keyed by packet index (0 = `ImageData`).
    chunks: BTreeMap<u16, Vec<u8>>,
}

impl TextureDownload {
    /// Whether every packet `0..packets` has been received.
    fn is_complete(&self) -> bool {
        usize::from(self.packets) == self.chunks.len()
    }

    /// Concatenates the buffered packets in index order into the full encoded
    /// image bytes.
    fn assemble(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for chunk in self.chunks.values() {
            data.extend_from_slice(chunk);
        }
        data
    }
}

/// What a completed inbound `Xfer` file download should become — the routing
/// tag stored alongside each in-flight download in
/// [`Session::xfer_downloads`](Session::xfer_downloads). Every consumer of the
/// shared download machinery (mute list, task inventory, and the generic
/// [`Session::request_xfer`](Session::request_xfer) path) registers its purpose
/// so the single `SendXferPacket` handler can route the assembled bytes to the
/// right typed event.
#[derive(Debug, Clone)]
enum XferPurpose {
    /// A mute-list file: parse it into [`Event::MuteList`](crate::Event::MuteList).
    MuteList,
    /// A task-inventory listing for the given object (`task`) at contents
    /// `serial`: parse it into
    /// [`Event::TaskInventoryContents`](crate::Event::TaskInventoryContents).
    TaskInventory {
        /// The in-world object whose task inventory this listing describes.
        task: ObjectKey,
        /// The contents serial reported by the `ReplyTaskInventory` that named
        /// the file, echoed back on the parsed event.
        serial: i16,
    },
    /// A caller-initiated raw download: surface the bytes verbatim as
    /// [`Event::XferDownloaded`](crate::Event::XferDownloaded).
    Generic,
    /// A file the *simulator* offered via `InitiateDownload` (today only the
    /// region terrain RAW download): surface the assembled bytes as
    /// [`Event::ServerFileDownloaded`](crate::Event::ServerFileDownloaded),
    /// tagged with the viewer filename we asked for (echoed back in the
    /// `InitiateDownload`).
    ServerInitiated {
        /// The viewer-side filename we named in the download request, echoed by
        /// the simulator's `InitiateDownload` and returned on the event so a
        /// caller can correlate the bytes to the file it requested.
        viewer_filename: String,
    },
}

/// An in-flight inbound `Xfer` file download: the accumulated bytes and what to
/// do with them once the final packet arrives. Started by a `MuteListUpdate`, an
/// auto-fetched `ReplyTaskInventory`, or [`Session::request_xfer`], and keyed by
/// [`XferId`] in [`Session::xfer_downloads`](Session::xfer_downloads).
#[derive(Debug)]
struct XferDownload {
    /// What the assembled file should be routed to on completion.
    purpose: XferPurpose,
    /// The file bytes accumulated so far (the seq-0 length prefix already
    /// stripped).
    buffer: Vec<u8>,
}

/// The UDP circuit to a single simulator.
#[derive(Debug)]
struct Circuit {
    /// This circuit instance's client-side identity, minted when the circuit is
    /// established (and preserved when a child is promoted to root). Used to
    /// scope region-local ids ([`CircuitId`]) so a stale id fails to resolve.
    id: CircuitId,
    /// The simulator's UDP address.
    sim_addr: SocketAddr,
    /// The agent/avatar id.
    agent_id: AgentKey,
    /// The session id.
    session_id: Uuid,
    /// The circuit code.
    code: CircuitCode,
    /// The next outgoing sequence number.
    next_sequence: SequenceNumber,
    /// The monotonically increasing serial number shared by `AgentPause` and
    /// `AgentResume`; the simulator ignores non-increasing values.
    pause_serial_num: u32,
    /// The next outgoing keep-alive ping id (mirrors the reference viewer's
    /// `LLCircuitData::mLastPingID`); a wrapping `u8` the matching
    /// `CompletePingCheck` echoes back.
    next_ping_id: PingId,
    /// The in-flight keep-alive ping awaiting its `CompletePingCheck`, paired
    /// with the instant it was sent so the round-trip time can be measured.
    /// `None` when no ping is outstanding.
    outstanding_ping: Option<(PingId, Instant)>,
    /// Inbound reliable sequence numbers we still owe acknowledgements for.
    pending_acks: Vec<SequenceNumber>,
    /// Outgoing reliable packets awaiting acknowledgement, keyed by sequence.
    unacked: BTreeMap<SequenceNumber, UnackedPacket>,
    /// Recently seen inbound reliable sequence numbers.
    seen: SeenWindow,
    /// Datagrams ready to be transmitted.
    out: VecDeque<Vec<u8>>,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: Distance,
    /// The connection timers.
    timers: Timers,
}

/// The lifecycle state of a [`Session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    /// Constructed; the login request is available but not yet answered.
    New,
    /// Login succeeded; bootstrap packets sent, awaiting the region handshake.
    AwaitingHandshake,
    /// The region handshake completed; keep-alives are flowing.
    Active,
    /// A `TeleportLocationRequest` was sent; awaiting the `TeleportFinish`.
    Teleporting,
    /// A `LogoutRequest` was sent; awaiting the `LogoutReply`.
    LoggingOut,
    /// The session is finished.
    Closed,
}

/// Where the session is in a teleport / region-handover sequence.
///
/// Collapses what were two correlated `Option` fields (the in-flight teleport's
/// destination and the post-arrival handover bookkeeping) into one value, so the
/// illegal "both set at once" combination is unrepresentable: a request and a
/// pending handover are mutually exclusive phases of the same sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeleportPhase {
    /// No teleport or region handover is in progress.
    Idle,
    /// A `TeleportLocationRequest` / `TeleportLureRequest` was sent; awaiting the
    /// `TeleportFinish`. `target` is the best-effort destination region handle
    /// (a cross-region lure's authoritative handle arrives with the finish).
    Requested {
        /// The destination region handle the teleport was aimed at.
        target: RegionHandle,
    },
    /// A teleport finished or a region border was crossed: the new root circuit
    /// is up and the next `RegionHandshake` should surface a
    /// [`Event::RegionChanged`] for `region_handle` (rather than the login-time
    /// `RegionHandshakeComplete`).
    Handover {
        /// The destination region handle reported by `TeleportFinish` / the
        /// region crossing.
        region_handle: RegionHandle,
    },
}

/// Where the agent is in an object-sit sequence.
///
/// Replaces a bare `sit_requested: bool` with the three distinct phases the
/// flag conflated, so the seat object learned from the `AvatarSitResponse` is
/// carried by the type rather than dropped: a request that has not been
/// answered cannot be confused with being seated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SitState {
    /// Not seated on an object and no sit request outstanding. Whether the
    /// agent is standing, walking, or ground-sitting is an avatar *animation*
    /// concern, not an object sit, and is not tracked here.
    NotSitting,
    /// An `AgentRequestSit` was sent; awaiting the `AvatarSitResponse`.
    AwaitingResponse,
    /// Seated on an object: the `AvatarSitResponse` arrived and the session
    /// answered with an `AgentSit`.
    Seated {
        /// The object the agent is seated on.
        on: ObjectKey,
    },
}

/// The key of a script-permission grant: the script that holds it, uniquely a
/// `(holding object, inventory item within it)` pair (one object may run several
/// scripts, each with its own grant). Both halves come straight off the
/// `ScriptQuestion` / [`Event::ScriptPermissionRequest`](crate::Event::ScriptPermissionRequest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ScriptHolder {
    /// The task (object) id holding the script.
    task_id: ObjectKey,
    /// The script item id within the object.
    item_id: InventoryKey,
}

/// Whether the script holding a grant lives in one of *this* agent's
/// attachments (the grant crosses regions with the avatar) or in an in-world
/// object (region/circuit scoped, left behind on a real teleport).
///
/// Detection failure falls back to [`InWorld`](Self::InWorld) — the
/// conservative direction (an unrecognised holder is cleared on the next
/// teleport rather than kept forever; losing a mirror entry is cheap, the
/// simulator still enforces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HolderKind {
    /// The script lives in an attachment worn by this agent; the grant crosses
    /// regions with the avatar (kept on teleport, cleared on detach).
    Attachment,
    /// The script lives in an in-world object (or another avatar's attachment,
    /// which is in-world from our frame); the grant is region/circuit scoped and
    /// dropped when the agent leaves it.
    InWorld,
}

/// The agent's recorded answer to a script-permission request: an explicit deny
/// (answered with no permissions) or a granted, non-empty subset. The third
/// state — *never asked* — is the absence of a registry entry, so it has no
/// variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrantStatus {
    /// The agent answered with no permissions (an explicit deny). Distinct from
    /// a never-asked holder, which has no registry entry at all, so the driver's
    /// prompt UI can tell "already refused this" from "not yet seen".
    Denied,
    /// The agent granted this subset, stored wholesale as the raw bitfield (the
    /// record-only flags need no handler, the cooperation flags reuse existing
    /// event surfaces). Never empty — an empty answer is [`Denied`](Self::Denied).
    Granted(ScriptPermissions),
}

/// One recorded answer to a script-permission request — the value half of the
/// grant registry. Records both grants and explicit denials (the `status`
/// distinguishes them); a never-asked holder is simply absent from the map.
///
/// The simulator stays authoritative; this is an API-convenience mirror of what
/// the agent answered, never a security boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScriptGrant {
    /// Whether the agent denied this script outright or granted it a non-empty
    /// permission subset. A denial still carries the `kind` / `circuit` below so
    /// the region-leave resets treat it identically to a grant.
    status: GrantStatus,
    /// Whether the holder is one of our attachments or an in-world object; drives
    /// the region-leave reset (attachments cross with the avatar, in-world
    /// objects are left behind).
    kind: HolderKind,
    /// The circuit the holder was last seen on, for scoping the
    /// `DisableSimulator` / circuit-retired reset. `None` when the holder was not
    /// in the object cache at grant time (the in-world fallback).
    circuit: Option<CircuitId>,
    /// The experience the grant was made under, copied from the request; `None`
    /// outside an experience.
    experience_id: Option<ExperienceKey>,
}

/// The session-global *taken-controls* tracker: which movement controls scripts
/// are currently holding, fed by the inbound `ScriptControlChange` and cleared by
/// [`Session::release_script_controls`].
///
/// `ScriptControlChange` carries no object/holder id (only a `Data` array of
/// `{ TakeControls, Controls, PassToAgent }`), so taken controls cannot be
/// attributed to a holder and do not live in the per-script grant registry; they
/// are agent-global. Like the viewer (`LLAgent::mControlsTakenCount` /
/// `mControlsTakenPassedOnCount`) this is a **per-control-bit count** split by
/// `PassToAgent`: two scripts may take the same bit, and one releasing it must
/// not clear it for the other — a single union would lose that.
///
/// The simulator stays authoritative; this is an API-convenience mirror.
#[derive(Debug)]
struct TakenControls {
    /// Per-control-bit take count for controls the script *consumes*
    /// (`PassToAgent` clear; the avatar does not move from the input). Keyed by
    /// the single-bit mask, the entry removed when the count reaches zero, so a
    /// present key ≡ a currently-held control (a sparse map, deterministic order).
    consumed: BTreeMap<u32, u32>,
    /// Per-control-bit take count for controls *also* passed to the agent
    /// (`PassToAgent` set). Same single-bit-mask keying and remove-at-zero rule.
    passed_on: BTreeMap<u32, u32>,
}

/// A single agent session: login bookkeeping plus one simulator circuit.
///
/// This is a pure state machine. Feed it bytes and the current [`Instant`] via
/// the `handle_*` methods; drain datagrams, timeouts, and events via the
/// `poll_*` methods. It performs no I/O and never reads a clock.
#[derive(Debug)]
pub struct Session {
    /// The login parameters.
    login: LoginParams,
    /// The current lifecycle state.
    state: SessionState,
    /// The active (root) circuit, once login has succeeded.
    circuit: Option<Circuit>,
    /// Child-agent circuits to neighbouring regions, keyed by simulator address.
    /// Opened from `EnableSimulator` so a neighbour already holds the agent's
    /// presence when the avatar crosses the border (promoted to root on
    /// `CrossedRegion`).
    children: BTreeMap<SocketAddr, Circuit>,
    /// The capability-seed URL for each child region (from the CAPS
    /// `EstablishAgentCommunication` event), keyed by simulator address; used as
    /// the new seed when a child is promoted to root.
    child_seeds: BTreeMap<SocketAddr, url::Url>,
    /// A monotonic counter for minting a fresh [`CircuitId`] each time a circuit
    /// instance is established (never zero — zero is the "no circuit" sentinel).
    next_circuit_id: u64,
    /// The draw distance (metres) advertised in keep-alive `AgentUpdate`s.
    draw_distance: Distance,
    /// The agent control flags advertised in keep-alive `AgentUpdate`s; the
    /// simulator moves the agent accordingly.
    controls: ControlFlags,
    /// The desired bandwidth throttle (`AgentThrottle`), once the application
    /// has set one. Persisted so it can be re-sent on every region change (a new
    /// root circuit starts with the simulator's defaults until re-told).
    throttle: Option<Throttle>,
    /// The agent's body rotation (facing) sent in `AgentUpdate`s.
    body_rotation: Rotation,
    /// The agent's head rotation sent in `AgentUpdate`s.
    head_rotation: Rotation,
    /// The agent's camera viewpoint advertised in keep-alive `AgentUpdate`s on
    /// the root *and* every child circuit. Drives the simulator's interest list,
    /// so it follows where the agent looks rather than the region origin.
    /// Defaults to [`Camera::region_center`] until a client calls
    /// [`Session::set_camera`].
    camera: Camera,
    /// Where the agent is in an object-sit sequence (request → response →
    /// seated). Drives whether an incoming `AvatarSitResponse` is one the
    /// session asked for, and records the seat object once sat.
    sit: SitState,
    /// Where the session is in a teleport / region-handover sequence (between
    /// sending a `TeleportLocationRequest` and the next region's
    /// `RegionHandshake`).
    teleport: TeleportPhase,
    /// The script-permission registry: what the agent has answered each script
    /// (a grant or an explicit deny), keyed by the holding `(object, item)`
    /// pair; a never-asked script is simply absent. Written by
    /// [`Session::answer_script_permissions`], cleared on revoke and the
    /// region-leave signals (a real teleport drops in-world entries, a circuit
    /// retiring drops that circuit's, an object going away drops its). The
    /// simulator stays authoritative; this is an API-convenience mirror, not a
    /// security boundary. Read via [`Session::granted_permissions`] /
    /// [`Session::script_permission_status`] / [`Session::script_grants`].
    script_grants: BTreeMap<ScriptHolder, ScriptGrant>,
    /// The session-global taken-controls tracker: which movement controls scripts
    /// are currently holding (per-bit counts split by `PassToAgent`). Folded from
    /// the inbound `ScriptControlChange` (a `Take` increments, a `Release`
    /// decrements), cleared wholesale by [`Session::release_script_controls`].
    /// Not attributable to a holder (`ScriptControlChange` carries no object id)
    /// and not reset on a region change — the viewer keeps it across teleport.
    /// Read via [`Session::script_controls`]. The simulator stays authoritative;
    /// this is an API-convenience mirror.
    taken_controls: TakenControls,
    /// The buddy-list cache: every current friend keyed by id, with the
    /// friendship rights in both directions. Seeded from the login buddy list
    /// (`FriendList`) and kept live — a friendship formed mid-session is added
    /// the moment it forms (we accepted via [`Session::accept_friendship`], or
    /// they accepted our offer via an inbound `FriendshipAccepted` IM), a
    /// `ChangeUserRights` updates the cached rights, and a terminated friendship
    /// drops the entry. Grid-level: it survives teleport, cleared only by a
    /// relogin through the constructor. The simulator stays authoritative; this
    /// is an API-convenience read model. Read via [`Session::friends`] /
    /// [`Session::friend`].
    friends: BTreeMap<FriendKey, Friend>,
    /// The set of friends currently known to be online. The **sole** source of
    /// presence truth (a friend is online iff present here), fed only by the
    /// authoritative `OnlineNotification` / `OfflineNotification` signals (and a
    /// terminated friendship removal) — never inferred from buddy-list or IM
    /// traffic, so an IM just after a peer goes offline cannot re-mark them
    /// online. Starts empty at login (the buddy list carries rights, not
    /// presence) and is grid-level like [`friends`](Self::friends). Read via
    /// [`Session::is_online`] / [`Session::online_friends`]. Absence is "offline
    /// or not visible", never provably offline (a friend who does not grant us
    /// `CAN_SEE_ONLINE` never generates a notification).
    online: BTreeSet<FriendKey>,
    /// The chat-session registry: one entry per open IM session (1:1 direct,
    /// group, or ad-hoc conference), keyed by the typed [`ChatSessionKind`] (which
    /// *is* the canonical session id, keeping the three id spaces disjoint). Each
    /// value mirrors that session's mutable state. Opened lazily on the first
    /// inbound *or* outbound traffic for a session and removed on an explicit
    /// `SessionLeave` (1:1 has no leave, so it persists to logout). Grid-level
    /// like the buddy cache: it survives teleport / region handover, cleared only
    /// by a relogin through the constructor. The simulator stays authoritative;
    /// this is an API-convenience read model. Read via [`Session::chat_sessions`].
    chat_sessions: BTreeMap<ChatSessionKind, ChatSession>,
    /// The current region's capability-seed URL (from login or a teleport), for
    /// the driver to fetch the CAPS map and event queue.
    seed_capability: Option<url::Url>,
    /// Whether the initial-login `CompleteAgentMovement` has been **deferred** until
    /// the region's capabilities are fetched. `true` from login until
    /// [`Session::notify_capabilities_ready`] releases it; `false` otherwise (once
    /// sent, or when it was not deferred — a teleport handover sends it immediately).
    ///
    /// The simulator gates object streaming — including an animesh's one-shot
    /// `ObjectAnimation` — on `CompleteAgentMovement`, and only streams
    /// `ObjectAnimation` to a viewer that advertised the `ObjectAnimation`
    /// capability. Sending `CompleteAgentMovement` only after the seed-caps request
    /// (which advertises it) is processed ensures the sim knows we support animesh
    /// before it streams the scene. The driver releases it after its caps fetch
    /// settles — on success *and* failure (a failed fetch proceeds into a degraded,
    /// capless session, the pre-existing behaviour), the fetch itself bounded by the
    /// driver's HTTP timeout — so no session-side timer is needed.
    pending_complete_movement: bool,
    /// The agent-appearance (server-side "Sunshine" bake) service base URL, from
    /// the `agent_appearance_service` login field. Server-baked avatar textures are
    /// fetched from here (`<url>texture/<avatar>/<slot>/<uuid>`), not by UUID from
    /// the `GetTexture` CDN. `None` on a grid without central baking (OpenSim).
    agent_appearance_service: Option<url::Url>,
    /// Account-level facts from the login response (home, maturity, group limit,
    /// Library roots), or `None` before login.
    login_account: Option<LoginAccount>,
    /// In-flight inbound `Xfer` file downloads, keyed by the client-chosen
    /// [`XferId`], each carrying the accumulated bytes and a routing
    /// [`XferPurpose`]. Started by a `MuteListUpdate`, an auto-fetched
    /// `ReplyTaskInventory`, or [`Session::request_xfer`]; the single
    /// `SendXferPacket` handler drains and routes them.
    xfer_downloads: BTreeMap<XferId, XferDownload>,
    /// A monotonic counter for generating `Xfer` ids (never zero).
    next_xfer_id: XferId,
    /// Objects whose task inventory a [`Session::fetch_task_inventory`] asked
    /// for, keyed by their full [`ObjectKey`] (resolved from the object cache at
    /// request time). When the matching `ReplyTaskInventory` arrives its `Xfer`
    /// listing is auto-downloaded and parsed into
    /// [`Event::TaskInventoryContents`](crate::Event::TaskInventoryContents)
    /// rather than surfaced only as a serial/filename.
    pending_task_inventory: BTreeSet<ObjectKey>,
    /// A FIFO fallback for `fetch_task_inventory` calls whose target object was
    /// not yet in the cache (so its full id could not be resolved to key
    /// [`pending_task_inventory`](Self::pending_task_inventory)). Each entry
    /// auto-fetches the next otherwise-unmatched `ReplyTaskInventory`; it cannot
    /// disambiguate concurrent uncached fetches.
    pending_task_inventory_unresolved: VecDeque<()>,
    /// In-flight legacy UDP texture downloads, keyed by the texture's asset id
    /// (echoed in every `ImageData`/`ImagePacket`). Started by
    /// [`Session::request_texture`].
    texture_downloads: BTreeMap<Uuid, TextureDownload>,
    /// The scene-graph object cache, keyed by the circuit instance the objects
    /// belong to (the root region *and* every child/neighbour circuit), then by
    /// region-local id. Region-local ids are only unique within a circuit, so
    /// the cache is partitioned per [`CircuitId`] — a reconnect to the same
    /// address mints a fresh circuit, so its objects never alias the old ones. A
    /// circuit's objects are dropped when it goes away (`DisableSimulator`,
    /// teleport handover, relogin).
    objects: BTreeMap<CircuitId, BTreeMap<RegionLocalObjectId, Object>>,
    /// The decoded terrain cache, keyed by the circuit instance the patches
    /// belong to (the root region *and* every neighbour streamed over a child
    /// circuit), then by `(layer code, patch x, patch y)` so each layer's
    /// patches are kept side by side. Dropped with the rest of a circuit's state
    /// when it goes away. See [`Session::terrain_patches`] and
    /// [`Session::terrain_height`].
    terrain: BTreeMap<CircuitId, BTreeMap<(u8, u32, u32), TerrainPatch>>,
    /// The region handle most recently learned for each circuit instance (from
    /// object updates, which carry it, and from `EnableSimulator`). Used to
    /// label terrain patches, which the `LayerData` message does not itself tag
    /// with a region handle.
    regions: BTreeMap<CircuitId, RegionHandle>,
    /// The most recent raw `RegionData.TimeDilation` (a `u16`) seen for each
    /// circuit instance, used to de-duplicate [`Event::TimeDilation`] so it is
    /// emitted only when the region's frame time-dilation actually changes
    /// (every object-update message carries the field). See
    /// [`Session::note_time_dilation`].
    time_dilation: BTreeMap<CircuitId, u16>,
    /// The region-local id of the agent's **own** avatar object on each circuit
    /// instance, learned the first time that avatar's `ObjectUpdate` is cached
    /// (or read back from the cache at `AgentMovementComplete`). Used to
    /// recognise an object parented to our own avatar — one of our attachments —
    /// when classifying a script-permission holder (the permission system's
    /// `HolderKind`). Per circuit because a region-local id is unique only within
    /// one simulator and the avatar is assigned a fresh one in each region;
    /// absent until the avatar object is first observed on that circuit, and set
    /// once (the id is stable for the life of the circuit). Dropped with the rest
    /// of a circuit's state in [`Session::forget_sim_objects`]. Surfaced via
    /// [`Session::own_avatar_id`].
    own_avatar: BTreeMap<CircuitId, RegionLocalObjectId>,
    /// The held inventory model: the agent's own inventory tree and the read-only
    /// Library tree, each owning its folder/item stores, per-folder fetch state,
    /// and a parent→children index, plus the inventory roots and the async
    /// `CallbackID` counter. Seeded from the login skeleton
    /// ([`Event::InventorySkeleton`]), grown by folder-contents fetches
    /// ([`Event::InventoryDescendents`], both UDP and CAPS) and the simulator's
    /// [`Event::InventoryBulkUpdate`] pushes, and kept current by the agent's own
    /// mutations. See [`Session::inventory_folder`].
    inventory: Inventory,
    /// Whether the automatic background inventory crawl is enabled. Off by
    /// default so a consumer that never reads inventory (e.g. a survey bot) issues
    /// no folder fetches; the explicit pull paths work regardless. Toggled by
    /// [`Session::set_background_inventory_fetch`] and consulted by
    /// [`Session::next_inventory_fetch_batch`].
    background_inventory_fetch: bool,
    /// Pending high-level events for the driver.
    events: VecDeque<Event>,
    /// Whether protocol diagnostics are collected. Off by default so the
    /// silent-drop sites cost nothing (no raw-byte capture, no queueing) on the
    /// normal path. Toggled by [`Session::set_diagnostics`].
    diagnostics_enabled: bool,
    /// Pending [`Diagnostic`]s for the driver, populated only while
    /// `diagnostics_enabled`. Drained by [`Session::poll_diagnostic`].
    diagnostics: VecDeque<Diagnostic>,
}

mod chat_session;
mod circuit;
mod conversions;
mod inventory;
mod inventory_cache;
mod methods;

use self::chat_session::{ChatSession, TYPING_TIMEOUT};
use self::inventory::Inventory;
pub use chat_session::{
    ChatLifecycleView, ChatSessionInfo, ChatSessionKind, ChatSessionLifecycle, FriendPresence,
    InviteChannel, MessageCursor, PendingInvite, SessionMessage, VoiceChannelInfo,
    VoiceChannelState,
};
pub use inventory::{FolderState, InventoryOwner};
pub use inventory_cache::INVENTORY_CACHE_VERSION;

pub(crate) use conversions::{
    ZERO_VECTOR, instant_message, region_handshake_message, shape_from_object_shape_block,
};
pub use conversions::{
    agent_drop_group_to_llsd, agent_state_update_to_llsd, ais_inventory_update_to_llsd,
    build_map_block_reply, build_map_item_reply, build_map_layer_reply,
    bulk_update_inventory_to_llsd, chat_session_request_body, chatterbox_invitation_to_llsd,
    created_category_to_llsd, crossed_region_to_caps_llsd, display_name_update_to_llsd,
    enable_simulator_to_caps_llsd, environment_to_llsd, establish_agent_communication_to_llsd,
    group_members_to_caps_llsd, group_memberships_to_caps_llsd, inventory_descendents_to_llsd,
    nav_mesh_status_to_llsd, offline_messages_to_llsd, open_region_info_to_llsd,
    parcel_info_to_llsd, required_voice_version_to_llsd, server_appearance_update_to_llsd,
    set_display_name_reply_to_llsd, sim_console_response_to_llsd, teleport_finish_to_llsd,
    windlight_refresh_to_llsd,
};
