//! The sans-I/O **simulator-side** CAPS surface: the seed-capability grant
//! and the per-capability dispatch registry.
//!
//! A [`SimCaps`] is the server-direction counterpart of the client's
//! capability handling: where the client POSTs
//! [`REQUESTED_CAPABILITIES`](crate::REQUESTED_CAPABILITIES) to the seed URL
//! (via [`build_seed_request`](crate::build_seed_request)) and parses the
//! granted name→URL map, a [`SimCaps`] parses that seed request, grants one
//! unguessable `…/cap/<uuid>` URL per *supported* capability, and routes
//! subsequent requests on those URLs to their handlers.
//!
//! It is a **sibling** of [`SimSession`], not a part of it: the (future) HTTP
//! glue owns both and passes the session into [`SimCaps::dispatch`] per
//! request. That split keeps the borrow story simple and, more importantly,
//! keeps `SimCaps` free of any login state — the seed URL is a plain value
//! ([`SimCaps::seed_url`]) that a login server, possibly in a **different
//! process**, embeds in its `LoginSuccess::seed_capability`. Nothing else
//! crosses the login↔simulator boundary.
//!
//! Like the rest of `sl-proto` this module performs no I/O: requests come in
//! as a transport-agnostic [`CapsRequest`], responses go out as a
//! [`CapsResponse`] (or the [`CapsDispatch::EventQueueWouldBlock`] marker the
//! runtime turns into a held long-poll). The runtime decides how long to
//! hold an empty `EventQueueGet` poll (~30 s in the reference stack) before
//! answering [`SimCaps::event_queue_timeout`].

use std::collections::{BTreeMap, HashMap, HashSet};

use sl_types::key::{
    AgentKey, ExperienceKey, GroupKey, InventoryFolderKey, InventoryKey, ObjectKey,
};
use sl_wire::{
    AisUpdate, AssetUploadResponse, DisplayName, ExperiencePermission, InterestListReply,
    LandResourcesUrls, Llsd, ObjectMediaRequest, ObjectMediaResponse, Permissions,
    UploadGrantedPermissions, build_agent_preferences_response, build_asset_upload_response,
    build_attachment_resources_response, build_avatar_picker_search_response,
    build_create_inventory_category_response, build_display_names_response,
    build_experience_ids_response, build_experience_infos_response,
    build_experience_permissions_response, build_experience_query_response,
    build_experience_search_response, build_experience_status_response,
    build_get_object_cost_response, build_get_object_physics_data_response,
    build_interest_list_reply, build_land_resource_detail_response,
    build_land_resource_summary_response, build_land_resources_response, build_lsl_syntax_document,
    build_modify_material_params_response, build_parcel_voice_info_response,
    build_provision_voice_account_response, build_region_experiences_response,
    build_remote_parcel_response, build_render_materials_response,
    build_resource_cost_selected_response, build_seed_response, build_simulator_features_response,
    find_experience_query, is_ais_current_outfit_links_url, is_ais_orphans_url,
    parse_agent_preferences, parse_ais_category_children_fetch_url,
    parse_ais_category_children_subset, parse_ais_category_children_url,
    parse_ais_category_links_url, parse_ais_category_url, parse_ais_create_category_body,
    parse_ais_create_category_url, parse_ais_create_link_body, parse_ais_item_url,
    parse_ais_move_body, parse_ais_rename_category_body, parse_ais_slam_body,
    parse_ais_update_item_body, parse_avatar_picker_search_query,
    parse_create_inventory_category_request, parse_display_names_query, parse_event_queue_request,
    parse_experience_id_query, parse_experience_info_query, parse_experience_query,
    parse_fetch_inventory_items_request, parse_fetch_inventory_request,
    parse_find_experience_query, parse_forget_experience_query, parse_get_object_cost_request,
    parse_get_object_physics_data_request, parse_group_experiences_query,
    parse_interest_list_request, parse_land_resources_request, parse_llsd_xml,
    parse_modify_material_params_request, parse_new_file_agent_inventory_request,
    parse_object_media_navigate_request, parse_object_media_request,
    parse_provision_voice_account_request, parse_region_experiences_request,
    parse_remote_parcel_request, parse_render_materials_put_request,
    parse_render_materials_request, parse_resource_cost_selected_request, parse_seed_request,
    parse_send_user_report, parse_set_experience_permission_request,
    parse_update_avatar_appearance_request, parse_update_experience_request,
    parse_update_item_asset_request, parse_update_script_agent_request,
    parse_update_script_task_request, parse_update_task_item_asset_request,
    parse_voice_signaling_request,
};
use url::Url;
use uuid::Uuid;

use crate::asset_caps::AssetCaps;
use crate::bookkeeping_ids::ImSessionId;
use crate::session::{
    ais_category_children_reply_to_llsd, ais_category_links_reply_to_llsd,
    ais_inventory_update_to_llsd, ais_item_reply_to_llsd, chat_session_agent_params_from_llsd,
    chat_session_request_from_llsd, chat_session_roster_to_llsd, cof_version_increment_to_llsd,
    environment_to_llsd, environment_update_from_llsd, fetch_inventory_items_to_llsd,
    inventory_descendents_to_llsd, llsd_map, parse_copy_inventory_from_notecard,
    session_history_to_llsd,
};
use crate::sim_inventory::SimInventoryError;
use crate::sim_new_file::{self, FirstStep, NewFilePolicy, SideUpload};
use crate::sim_session::{CapsUploadMetadata, SimSession};
use crate::task_caps::{
    TaskInventoryCapQuery, build_get_metadata_reply, build_task_inventory_cap_reply,
    parse_get_metadata_request, parse_task_inventory_cap_query,
};
use crate::types::TaskInventoryItem;
use crate::types::{GroupBan, GroupBanAction, GroupMember};
use crate::{
    CAP_ACCEPT_FRIENDSHIP, CAP_ACCEPT_GROUP_INVITE, CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES,
    CAP_ATTACHMENT_RESOURCES, CAP_AVATAR_PICKER_SEARCH, CAP_AVATAR_RENDER_INFO,
    CAP_CHAT_SESSION_REQUEST, CAP_COPY_INVENTORY_FROM_NOTECARD, CAP_CREATE_INVENTORY_CATEGORY,
    CAP_DECLINE_FRIENDSHIP, CAP_DECLINE_GROUP_INVITE, CAP_DIRECT_DELIVERY,
    CAP_DISPATCH_REGION_INFO, CAP_ESTATE_ACCESS, CAP_EXPERIENCE_PREFERENCES, CAP_EXPERIENCE_QUERY,
    CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FETCH_INVENTORY_ITEM, CAP_FETCH_LIBRARY,
    CAP_FETCH_LIBRARY_ITEM, CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES,
    CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES, CAP_GET_EXPERIENCE_INFO,
    CAP_GET_EXPERIENCES, CAP_GET_METADATA, CAP_GET_OBJECT_COST, CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_GROUP_API_V1, CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA, CAP_HOME_LOCATION,
    CAP_INCREMENT_COF_VERSION, CAP_INTEREST_LIST, CAP_INVENTORY_API_V3,
    CAP_INVENTORY_THUMBNAIL_UPLOAD, CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_LAND_RESOURCES, CAP_LIBRARY_API_V3, CAP_LSL_SYNTAX, CAP_MESH_UPLOAD_FLAG,
    CAP_MODIFY_MATERIAL_PARAMS, CAP_MODIFY_REGION, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_PROPERTIES_UPDATE, CAP_PARCEL_VOICE_INFO,
    CAP_PRODUCT_INFO_REQUEST, CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS,
    CAP_REGION_EXPERIENCES, CAP_REGION_SCHEDULE, CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS,
    CAP_REQUEST_TASK_INVENTORY, CAP_RESOURCE_COST_SELECTED, CAP_SEARCH_STAT_REQUEST,
    CAP_SEND_POSTCARD, CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    CAP_SET_DISPLAY_NAME, CAP_SIMULATOR_FEATURES, CAP_UNTRUSTED_SIMULATOR_MESSAGE,
    CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_GESTURE_TASK_INVENTORY, CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_MATERIAL_TASK_INVENTORY, CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_TASK_INVENTORY, CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPDATE_SETTINGS_TASK_INVENTORY,
    CAP_UPLOAD_AGENT_PROFILE_IMAGE, CAP_UPLOAD_BAKED_TEXTURE, CAP_VOICE_SIGNALING,
    CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE, CHAT_SESSION_DECLINE_P2P_VOICE,
    CHAT_SESSION_FETCH_HISTORY, CHAT_SESSION_INVITE, CHAT_SESSION_START_CONFERENCE, Event,
    InventoryItem, InventoryListing, ServerEvent, VoiceProvisionRefusal, offline_messages_to_llsd,
};
use sl_wire::{
    AvatarRenderInfo, AvatarRenderReport, EstateAccessLists, EstateAccessStyle,
    build_avatar_render_info_response, build_avatar_render_report_response,
    build_estate_access_response, parse_avatar_render_report_request,
};

/// The LLSD-XML media type CAPS bodies use.
pub const LLSD_XML_CONTENT_TYPE: &str = "application/llsd+xml";

/// The media type of body-less error responses.
const TEXT_PLAIN_CONTENT_TYPE: &str = "text/plain";

/// The capability-name key of the event-queue long-poll.
const EVENT_QUEUE_GET: &str = "EventQueueGet";

/// The LLSD-XML body of a data-less `200` ack (`<llsd><undef /></llsd>`).
const UNDEF_LLSD_BODY: &str = "<llsd><undef /></llsd>";

/// The sub-path under the `SendUserReportWithScreenshot` cap URL that step 1
/// mints as its uploader URL and step 2 POSTs the screenshot bytes to.
const SCREENSHOT_SUB_PATH: &str = "screenshot";

/// The sub-path under a two-stage-upload cap URL that step 1 mints as its
/// uploader URL and step 2 POSTs the raw asset bytes to. The generalisation of
/// [`SCREENSHOT_SUB_PATH`] across the whole `NewFile*`/`Update*` family.
const UPLOAD_SUB_PATH: &str = "upload";

/// The sub-path under the `LandResources` cap URL the POST mints as its
/// `ScriptResourceSummary` follow-up URL.
const LAND_RESOURCES_SUMMARY_SUB_PATH: &str = "summary";

/// The sub-path under the `LandResources` cap URL the POST mints as its
/// `ScriptResourceDetails` follow-up URL.
const LAND_RESOURCES_DETAIL_SUB_PATH: &str = "detail";

/// The two-stage asset-upload capabilities routed to
/// [`CapHandler::AssetUpload`]: the shared server-side uploader state machine
/// serves them all, branching on the cap name only to pick the step-1 metadata
/// parser. `NewFileAgentInventory` creates a new asset + inventory item;
/// `UploadBakedTexture` a temporary asset with no item; the `Update*` caps
/// replace an existing item's asset (the two `Update*Script*` caps additionally
/// compile).
const UPLOAD_CAPABILITIES: &[&str] = &[
    CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_TASK_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SCRIPT_TASK,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_UPDATE_SETTINGS_TASK_INVENTORY,
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_GESTURE_TASK_INVENTORY,
    CAP_UPDATE_MATERIAL_TASK_INVENTORY,
];

/// The capability names this simulator can serve — the registry keys.
///
/// [`SimCaps::handler_for`] maps each entry to its [`CapHandler`]; the
/// `protocol-sim-caps-*` cluster tasks grow both together (plus the pinned
/// coverage table below, which tracks this list against
/// [`REQUESTED_CAPABILITIES`](crate::REQUESTED_CAPABILITIES)).
const SERVED_CAPABILITIES: &[&str] = &[
    EVENT_QUEUE_GET,
    CAP_CHAT_SESSION_REQUEST,
    CAP_READ_OFFLINE_MSGS,
    CAP_ACCEPT_FRIENDSHIP,
    CAP_DECLINE_FRIENDSHIP,
    CAP_ACCEPT_GROUP_INVITE,
    CAP_DECLINE_GROUP_INVITE,
    CAP_GROUP_MEMBER_DATA,
    CAP_GROUP_API_V1,
    CAP_GET_DISPLAY_NAMES,
    CAP_SET_DISPLAY_NAME,
    CAP_SEARCH_STAT_REQUEST,
    CAP_PRODUCT_INFO_REQUEST,
    CAP_AVATAR_PICKER_SEARCH,
    CAP_AGENT_PREFERENCES,
    CAP_INTEREST_LIST,
    CAP_SEND_USER_REPORT,
    CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    // The smaller upload capabilities.
    CAP_MESH_UPLOAD_FLAG,
    CAP_INVENTORY_THUMBNAIL_UPLOAD,
    CAP_SEND_POSTCARD,
    CAP_UPLOAD_AGENT_PROFILE_IMAGE,
    // The content upload/update, materials and MOAP cluster.
    CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_TASK_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SCRIPT_TASK,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_UPDATE_SETTINGS_TASK_INVENTORY,
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_GESTURE_TASK_INVENTORY,
    CAP_UPDATE_MATERIAL_TASK_INVENTORY,
    // A prim's contents.
    CAP_REQUEST_TASK_INVENTORY,
    CAP_GET_METADATA,
    CAP_UPDATE_AVATAR_APPEARANCE,
    CAP_INCREMENT_COF_VERSION,
    CAP_COPY_INVENTORY_FROM_NOTECARD,
    CAP_RENDER_MATERIALS,
    CAP_MODIFY_MATERIAL_PARAMS,
    CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE,
    // The inventory cluster: AISv3 + the legacy folder/per-item fetch caps.
    CAP_FETCH_INVENTORY,
    CAP_FETCH_LIBRARY,
    CAP_FETCH_INVENTORY_ITEM,
    CAP_FETCH_LIBRARY_ITEM,
    CAP_INVENTORY_API_V3,
    CAP_LIBRARY_API_V3,
    CAP_CREATE_INVENTORY_CATEGORY,
    // The region/object-information cluster.
    CAP_SIMULATOR_FEATURES,
    CAP_LSL_SYNTAX,
    CAP_EXT_ENVIRONMENT,
    CAP_REMOTE_PARCEL_REQUEST,
    CAP_PARCEL_PROPERTIES_UPDATE,
    CAP_GET_OBJECT_COST,
    CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_RESOURCE_COST_SELECTED,
    CAP_ATTACHMENT_RESOURCES,
    CAP_AVATAR_RENDER_INFO,
    CAP_LAND_RESOURCES,
    // The experience cluster.
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
    CAP_EXPERIENCE_QUERY,
    // The estate's access lists.
    CAP_ESTATE_ACCESS,
    // The region settings write and the restart schedule.
    CAP_DISPATCH_REGION_INFO,
    CAP_REGION_SCHEDULE,
    // The region's PBR terrain overrides.
    CAP_MODIFY_REGION,
    // The Marketplace's listing service, as it answers an avatar with no store.
    CAP_DIRECT_DELIVERY,
    // "Set Home to Here".
    CAP_HOME_LOCATION,
    // A template message posted as LLSD (the god kick).
    CAP_UNTRUSTED_SIMULATOR_MESSAGE,
    // The voice signalling cluster.
    CAP_PROVISION_VOICE_ACCOUNT,
    CAP_PARCEL_VOICE_INFO,
    CAP_VOICE_SIGNALING,
];

/// How the simulator serves one capability name.
///
/// The registry maps capability names to variants of this enum; the
/// `protocol-sim-caps-*` cluster tasks extend it one variant per served
/// capability family, each dispatched to the typed sl-wire
/// `parse_*_request`/`build_*_response` inverse pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CapHandler {
    /// The `EventQueueGet` long-poll, wired to [`SimSession`]'s event queue
    /// ([`SimSession::enqueue_caps_event`] /
    /// [`SimSession::take_event_queue_response`]).
    EventQueue,
    /// The `ChatSessionRequest` chat-session lifecycle (start conference /
    /// invite / accept / decline / decline p2p voice / fetch history), served
    /// from [`SimSession`]'s chat-session registry.
    ChatSession,
    /// The deliver-once `ReadOfflineMsgs` fetch of messages stored while the
    /// agent was offline ([`SimSession::take_offline_messages`]).
    OfflineMessages,
    /// The `AcceptFriendship` POST / `DeclineFriendship` DELETE answering a
    /// kept friendship offer by its offerer (`?from=`), handed to the driver
    /// as [`ServerEvent::KeptFriendshipAnswered`].
    KeptFriendshipAnswer,
    /// The `AcceptGroupInvite` / `DeclineGroupInvite` POST of `{group}`
    /// answering a kept group invitation, handed to the driver as
    /// [`ServerEvent::KeptGroupInvitationAnswered`].
    KeptGroupInvitationAnswer,
    /// The `GroupMemberData` POST of `{group_id}`: a group's roster, which is
    /// the driver's to know ([`CapsDispatch::GroupMembers`]).
    GroupMemberData,
    /// The `GroupAPIv1` GET / POST of a group's bans (`?group_id=`), which are
    /// the driver's ([`CapsDispatch::GroupBans`]).
    GroupBans,
    /// The `AvatarPickerSearch` name search over [`SimSession`]'s display-name
    /// store — the modern replacement for the UDP `AvatarPickerRequest`.
    AvatarPickerSearch,
    /// The `GetDisplayNames` people-service lookup, served from
    /// [`SimSession`]'s display-name store ([`SimSession::display_name`]).
    DisplayNames,
    /// The `AgentPreferences` merge-and-echo of the agent's server-stored
    /// preferences ([`SimSession::agent_preferences`]).
    AgentPreferences,
    /// The `InterestList` mode switch ([`SimSession::interest_list_mode`]).
    InterestList,
    /// The `RequestTaskInventory` GET of a prim's contents. The prims are the
    /// driver's, so the request is handed to it
    /// ([`CapsDispatch::TaskInventory`]).
    TaskInventory,
    /// The `GetMetadata` POST asking a script's experience, handed to the
    /// driver likewise ([`CapsDispatch::ScriptMetadata`]).
    ScriptMetadata,
    /// The one-step `SendUserReport` abuse-report POST.
    UserReport,
    /// The two-step `SendUserReportWithScreenshot` uploader: the report POST
    /// answers with an uploader URL (a sub-path of the cap's own URL), the
    /// raw screenshot bytes complete it.
    UserReportScreenshot,
    /// The `MeshUploadFlag` GET: whether the agent may upload mesh.
    MeshUploadFlag,
    /// The two-step `InventoryThumbnailUpload`: what the thumbnail is for,
    /// then the image.
    ThumbnailUpload,
    /// The two-step `SendPostcard`: the card, then the snapshot.
    Postcard,
    /// The two-step `UploadAgentProfileImage`: which profile field, then the
    /// image.
    ProfileImageUpload,
    /// The `SetDisplayName` POST, answered over the event queue from and
    /// into [`SimSession`]'s display-name store.
    SetDisplayName,
    /// The `SearchStatRequest` POST of a classified's click counts.
    SearchStat,
    /// The `ProductInfoRequest` GET of the land products.
    ProductInfo,
    /// The `EstateAccess` GET of the estate's four lists. The estate is the
    /// driver's, so the request is handed to it ([`CapsDispatch::EstateAccess`]).
    EstateAccess,
    /// The `DispatchRegionInfo` POST of the region's settings. The region is
    /// the driver's, so the request is handed to it
    /// ([`CapsDispatch::RegionInfoWrite`]).
    DispatchRegionInfo,
    /// The `RegionSchedule` GET and POST of the restart schedule, handed to
    /// the driver ([`CapsDispatch::RegionSchedule`]).
    RegionSchedule,
    /// The `ModifyRegion` GET and POST of the region's PBR terrain overrides,
    /// handed to the driver ([`CapsDispatch::ModifyRegion`]).
    ModifyRegion,
    /// The Second Life Marketplace's `DirectDelivery` listing service, as it
    /// answers an avatar with no merchant store: `404` on every route.
    DirectDelivery,
    /// "Set Home to Here" (`HomeLocation`), handed to the driver
    /// ([`CapsDispatch::HomeLocation`]).
    HomeLocation,
    /// The `UntrustedSimulatorMessage` POST of a template message as LLSD,
    /// handed to the driver ([`CapsDispatch::UntrustedMessage`]).
    UntrustedSimulatorMessage,
    /// The shared two-stage asset-upload state machine serving every
    /// `UPLOAD_CAPABILITIES` entry (`NewFileAgentInventory`,
    /// `UploadBakedTexture`, and the `Update*{Agent,Task}Inventory` family):
    /// step 1 parks the parsed metadata and answers an uploader URL, step 2
    /// (the raw-bytes POST) completes it into
    /// [`ServerEvent::CapsAssetUploaded`](crate::ServerEvent::CapsAssetUploaded).
    AssetUpload,
    /// The single-POST `UpdateAvatarAppearance` server-side-bake trigger.
    AvatarAppearance,
    /// The single-GET `IncrementCOFVersion` Current Outfit Folder version bump.
    IncrementCofVersion,
    /// The one-way `CopyInventoryFromNotecard` POST (no reply body).
    CopyInventoryFromNotecard,
    /// The legacy `RenderMaterials` materials surface (POST query / PUT set /
    /// GET all), served from the session's material store.
    RenderMaterials,
    /// The `ModifyMaterialParams` GLTF-material set POST.
    ModifyMaterialParams,
    /// The `ObjectMedia` media-on-a-prim read/write POST (GET / UPDATE verbs).
    ObjectMedia,
    /// The `ObjectMediaNavigate` media-navigation POST.
    ObjectMediaNavigate,
    /// The `FetchInventoryDescendents2` / `FetchLibDescendents2` folder-listing
    /// POST, served from the session's agent / Library inventory tree
    /// ([`SimSession::agent_inventory`] / [`SimSession::library_inventory`]).
    FetchDescendents,
    /// The `FetchInventory2` / `FetchLib2` per-item fetch POST, served from
    /// the same trees.
    FetchItems,
    /// The `InventoryAPIv3` / `LibraryAPIv3` REST surface (verb × sub-path
    /// routing under the cap URL); `LibraryAPIv3` is GET-only.
    Ais3,
    /// The plain `CreateInventoryCategory` POST (client-chosen folder id).
    CreateInventoryCategory,
    /// The bodyless `SimulatorFeatures` GET, served from the session's
    /// feature document ([`SimSession::set_simulator_features`]).
    SimulatorFeatures,
    /// The bodyless `LSLSyntax` GET, served from the session's syntax
    /// document ([`SimSession::set_lsl_syntax`]).
    LslSyntax,
    /// The `ExtEnvironment` EEP surface (GET with a `?parcelid=` query, PUT
    /// publishing an update), served from the session's environment store
    /// ([`SimSession::set_environment`]).
    Environment,
    /// The `RemoteParcelRequest` location→parcel-id lookup POST, resolved
    /// against the session's parcel-cover store ([`SimSession::add_parcel`]).
    RemoteParcel,
    /// The `ParcelPropertiesUpdate` parcel-edit POST, routed to the driver as
    /// the same [`ServerEvent::ParcelPropertiesUpdated`](crate::ServerEvent::ParcelPropertiesUpdated)
    /// the UDP message raises.
    ParcelUpdate,
    /// The `GetObjectCost` per-object cost POST
    /// ([`SimSession::set_object_cost`]).
    ObjectCost,
    /// The `GetObjectPhysicsData` per-object physics POST
    /// ([`SimSession::set_object_physics`]).
    ObjectPhysics,
    /// The `ResourceCostSelected` selection-sum POST
    /// ([`SimSession::set_selection_cost`]).
    ResourceCostSelected,
    /// The bodyless `AttachmentResources` GET
    /// ([`SimSession::set_attachment_resources`]).
    AttachmentResources,
    /// The `AvatarRenderInfo` statement GET and report POST: what the viewers
    /// of a region report is the region's, so both go to the driver
    /// ([`CapsDispatch::AvatarRenderInfoRead`] /
    /// [`CapsDispatch::AvatarRenderInfoReport`]).
    AvatarRenderInfo,
    /// The two-stage `LandResources` surface: the POST answers the
    /// summary/detail follow-up URLs (sub-paths of the cap's own URL), the
    /// follow-up GETs serve the stored reports
    /// ([`SimSession::set_land_resource_summary`] /
    /// [`SimSession::set_land_resource_details`]).
    LandResources,
    /// The `GetExperienceInfo` record lookup GET (a `/id/?public_id=…`
    /// sub-path + query), served from the session's experience store
    /// ([`SimSession::experiences`]).
    ExperienceInfo,
    /// The `FindExperienceByName` search GET (`?page=…&query=…`), served
    /// from the same store's public records.
    ExperienceSearch,
    /// The bodyless `GetExperiences` GET: the agent's allowed / blocked
    /// preference lists.
    ExperiencePermissions,
    /// The `ExperiencePreferences` mutation surface (PUT `Allow`/`Block`
    /// body, DELETE `?<id>` forget), echoing the full permission lists.
    ExperiencePreferences,
    /// The experience id-list GETs (`AgentExperiences`,
    /// `GetAdminExperiences`, `GetCreatorExperiences`, `GroupExperiences`),
    /// name-routed to the store's owned / admin / creator / per-group
    /// lists; `GroupExperiences` requires its `?<group_id>` query.
    ExperienceIdList,
    /// The `IsExperienceAdmin` / `IsExperienceContributor` status GETs
    /// (`?experience_id=…`), name-routed to the store's admin / creator
    /// membership.
    ExperienceStatus,
    /// The `UpdateExperience` metadata-edit POST, applied to the same
    /// store's record (`SimSession::apply_experience_update`).
    UpdateExperience,
    /// The `RegionExperiences` surface: GET serves the region's
    /// allowed / blocked / trusted lists, POST replaces them wholesale
    /// (`SimSession::apply_region_experiences`).
    RegionExperiences,
    /// The `ExperienceQuery` GET (`?parcelid=…&experiences=…`), answering
    /// per experience whether that parcel admits it, from the store's
    /// per-parcel table (`SimExperiences::parcel_experiences`).
    ExperienceQuery,
    /// `ProvisionVoiceAccountRequest`: a WebRTC offer → JSEP answer (or a
    /// logout), or the Vivox account fixture, served from the voice stub
    /// ([`SimSession::voice`], `SimSession::provision_voice`).
    ProvisionVoiceAccount,
    /// `ParcelVoiceInfoRequest`: the agent's parcel voice channel from the
    /// stub's parcel table (`SimSession::parcel_voice_info`).
    ParcelVoiceInfo,
    /// `VoiceSignalingRequest`: the WebRTC ICE trickle, recorded on its
    /// connection (`SimSession::record_voice_signaling`).
    VoiceSignaling,
}

/// A transport-agnostic CAPS HTTP request, borrowed from the server glue.
///
/// `method`, `path`, `body`, `query` and `range` are all consumed: the
/// agent-communication caps read `query`/`body`, and the asset-delivery caps
/// read `query` (the `?<class>_id=<uuid>` selector) and `range` (the byte
/// range). The AIS inventory (sub-path routing) cluster task may extend this
/// type further instead of redesigning it — more fields may grow here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapsRequest<'a> {
    /// The HTTP method, uppercase (`"GET"`, `"POST"`, …).
    pub method: &'a str,
    /// The absolute URL path (e.g. `/cap/<uuid>` plus any sub-path).
    pub path: &'a str,
    /// The raw query string without the `?`, if any.
    pub query: Option<&'a str>,
    /// The raw `Range` header value without the header name (e.g.
    /// `bytes=0-1023`), if the client sent one. Consumed only by the
    /// asset-delivery caps' partial-content path.
    pub range: Option<&'a str>,
    /// The raw `If-Modified-Since` header value, if the client sent one.
    /// Consumed only by the asset-delivery caps, where the policy has them
    /// state a date to send back.
    pub if_modified_since: Option<&'a str>,
    /// The raw `Destination` header value, if the client sent one: the folder
    /// an AIS3 `COPY` copies into.
    pub destination: Option<&'a str>,
    /// The request body (LLSD-XML for seed and event-queue requests).
    pub body: &'a [u8],
}

/// A transport-agnostic CAPS HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The `Content-Type` header value.
    pub content_type: &'static str,
    /// The `Content-Range` header value without the header name (e.g.
    /// `bytes 0-1023/4096`), set on the asset-delivery caps' `206` responses and
    /// `None` otherwise.
    pub content_range: Option<String>,
    /// Further headers, by lowercase name: what the asset-delivery caps state
    /// for a cache (`accept-ranges`, `etag`, `last-modified`). Empty
    /// otherwise.
    pub headers: Vec<(&'static str, String)>,
    /// The response body.
    pub body: Vec<u8>,
}

impl CapsResponse {
    /// A `200 OK` response carrying an LLSD-XML body.
    const fn llsd_xml(body: String) -> Self {
        Self {
            status: 200,
            content_type: LLSD_XML_CONTENT_TYPE,
            content_range: None,
            headers: Vec::new(),
            body: body.into_bytes(),
        }
    }

    /// A response of `status` carrying an LLSD-XML body.
    pub(crate) const fn llsd_xml_status(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: LLSD_XML_CONTENT_TYPE,
            content_range: None,
            headers: Vec::new(),
            body: body.into_bytes(),
        }
    }

    /// A response of `status` carrying an HTML page.
    pub(crate) fn html(status: u16, page: &str) -> Self {
        Self {
            status,
            content_type: "text/html",
            content_range: None,
            headers: Vec::new(),
            body: page.as_bytes().to_vec(),
        }
    }

    /// A response of `status` carrying `words` and stating no media type.
    pub(crate) fn words(status: u16, words: &str) -> Self {
        Self {
            status,
            content_type: "",
            content_range: None,
            headers: Vec::new(),
            body: words.as_bytes().to_vec(),
        }
    }

    /// A body-less response with the given status code.
    const fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: TEXT_PLAIN_CONTENT_TYPE,
            content_range: None,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// `404 Not Found` — an unknown capability URL, an event-queue poll after
    /// teardown (the client's "stop polling" signal), or a missing asset.
    pub(crate) const fn not_found() -> Self {
        Self::empty(404)
    }

    /// `405 Method Not Allowed` — a known capability URL hit with the wrong
    /// HTTP method.
    pub(crate) const fn method_not_allowed() -> Self {
        Self::empty(405)
    }

    /// `400 Bad Request` — a body that is not UTF-8 or not well-formed
    /// LLSD-XML.
    const fn bad_request() -> Self {
        Self::empty(400)
    }

    /// `401 Unauthorized` — a voice provision whose channel credentials do
    /// not match (the viewer reports it as "channel locked").
    const fn unauthorized() -> Self {
        Self::empty(401)
    }

    /// `502 Bad Gateway` — the "nothing yet, re-poll" answer to a held
    /// event-queue poll whose hold expired.
    const fn bad_gateway() -> Self {
        Self::empty(502)
    }

    /// `500 Internal Server Error` — the serving fixture holds a value the
    /// wire shape cannot carry (an out-of-range L$ sale price). A server-data
    /// fault, deliberately not disguised as a client error.
    const fn internal_error() -> Self {
        Self::empty(500)
    }

    /// `200 OK` carrying a whole asset of the given content type — the answer
    /// to an asset fetch with no (or an unparsable) `Range` header.
    pub(crate) const fn asset_whole(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type,
            content_range: None,
            headers: Vec::new(),
            body,
        }
    }

    /// `206 Partial Content` carrying `body` (the asset bytes `start..=last`)
    /// with `Content-Range: bytes {start}-{last}/{total}` (inclusive `last`,
    /// per the client's byte-range contract).
    pub(crate) fn asset_partial(
        content_type: &'static str,
        body: Vec<u8>,
        start: usize,
        last: usize,
        total: usize,
    ) -> Self {
        Self {
            status: 206,
            content_type,
            content_range: Some(format!("bytes {start}-{last}/{total}")),
            headers: Vec::new(),
            body,
        }
    }

    /// `206 Partial Content` carrying several ranges as one multipart `body`
    /// — under the asset's own media type and with no `Content-Range` of its
    /// own, which is how Second Life's asset hosts answer several.
    pub(crate) const fn asset_multipart(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: 206,
            content_type,
            content_range: None,
            headers: Vec::new(),
            body,
        }
    }

    /// A body-less response with `status`, for the asset-delivery caps'
    /// policy-chosen answers.
    pub(crate) const fn status(status: u16) -> Self {
        Self::empty(status)
    }

    /// The same response stating `headers` too.
    #[must_use]
    pub(crate) fn with_headers(mut self, headers: Vec<(&'static str, String)>) -> Self {
        self.headers = headers;
        self
    }
}

/// How a grid answers a `CopyInventoryFromNotecard`
/// ([`SimCaps::copy_from_notecard_response`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFromNotecardAnswer {
    /// `200` and an undefined LLSD body: Second Life's answer, whether or not
    /// it copied anything.
    Undefined,
    /// `200` and no body: OpenSim's answer to a copy it made.
    Empty,
    /// `400` and no body: OpenSim's answer where it found nothing to copy.
    Refused,
}

/// The outcome of dispatching one CAPS request.
#[derive(Debug, Clone, PartialEq)]
pub enum CapsDispatch {
    /// Respond immediately.
    Response(CapsResponse),
    /// An `EventQueueGet` poll with nothing queued: hold the request open.
    /// The runtime re-dispatches when [`SimSession::has_caps_events`] turns
    /// true, or answers [`SimCaps::event_queue_timeout`] (`502` — the
    /// "nothing yet, re-poll" signal the reference viewer expects) when its
    /// hold (~30 s) expires.
    EventQueueWouldBlock,
    /// A `RequestTaskInventory` GET. What a prim holds is the driver's to
    /// know: it looks the prim up and answers with
    /// [`SimCaps::task_inventory_response`].
    TaskInventory(TaskInventoryCapQuery),
    /// A `GetMetadata` POST asking which experience a script was compiled
    /// under. The driver answers with [`SimCaps::script_metadata_response`].
    ScriptMetadata {
        /// The prim holding the script.
        task: ObjectKey,
        /// The script.
        item: InventoryKey,
    },
    /// A `GroupMemberData` POST: who is in a group is the driver's to know.
    /// It answers with [`SimCaps::group_member_data_response`].
    GroupMembers {
        /// The group asked about.
        group_id: GroupKey,
    },
    /// A `GroupAPIv1` request: a read of a group's bans, or a change of them.
    /// The driver answers a read with [`SimCaps::group_bans_response`] and a
    /// change it made with [`SimCaps::group_bans_changed_response`].
    GroupBans {
        /// The group.
        group_id: GroupKey,
        /// What the POST does and to whom; `None` for a GET.
        change: Option<(GroupBanAction, Vec<AgentKey>)>,
    },
    /// An `EstateAccess` GET: the estate and who may read it are the driver's
    /// to know. It answers with [`SimCaps::estate_access_response`].
    EstateAccess,
    /// A `CopyInventoryFromNotecard` POST: the notecard and what is embedded
    /// in it are the driver's to read. It copies what it finds and answers
    /// with [`SimCaps::copy_from_notecard_response`].
    CopyFromNotecard {
        /// The notecard holding the embedded item.
        notecard_id: InventoryKey,
        /// The in-world object holding the notecard, or `None` for an
        /// agent-inventory notecard.
        object_id: Option<ObjectKey>,
        /// The embedded item to copy.
        item_id: InventoryKey,
        /// The destination folder, or `None` to let the grid pick the system
        /// folder for the item's type.
        folder_id: Option<InventoryFolderKey>,
    },
    /// An `AvatarRenderInfo` GET: the region's statement is made of every
    /// viewer's reports, which are the driver's to hold. It answers with
    /// [`SimCaps::avatar_render_info_response`].
    AvatarRenderInfoRead {
        /// The agent asking, whose counts the statement carries.
        reader: AgentKey,
    },
    /// An `AvatarRenderInfo` POST: a viewer's report, for the driver to file.
    /// It answers with [`SimCaps::avatar_render_report_response`].
    AvatarRenderInfoReport {
        /// The agent reporting.
        reporter: AgentKey,
        /// What it reported.
        reports: Vec<AvatarRenderReport>,
    },
    /// A `DispatchRegionInfo` POST: whether the writer may change the region
    /// is the driver's to know. It answers with
    /// [`SimCaps::region_info_write_response`].
    RegionInfoWrite(crate::RegionInfoUpdate),
    /// A `RegionSchedule` request: a read (`change` `None`) or a write of the
    /// schedule (`Some(None)` clears it). The driver answers with
    /// [`SimCaps::region_schedule_response`].
    RegionSchedule {
        /// The schedule a POST writes; `None` for a GET.
        change: Option<Option<sl_wire::RegionSchedule>>,
    },
    /// A `ModifyRegion` request: a read (`change` `None`) or a write of the
    /// region's PBR terrain overrides. The driver answers with
    /// [`SimCaps::modify_region_response`].
    ModifyRegion {
        /// The overrides a POST writes; `None` for a GET.
        change: Option<sl_wire::TerrainMaterialOverrides>,
    },
    /// A message posted to `UntrustedSimulatorMessage`. The driver acts on it
    /// and answers with [`SimCaps::untrusted_message_response`].
    UntrustedMessage(sl_wire::UntrustedMessage),
    /// A "Set Home to Here" posted to `HomeLocation`: the location asked for,
    /// or `None` for a body that names none, which a grid refuses rather than
    /// rejects. The driver applies its set-home rule and answers with
    /// [`SimCaps::home_location_response`].
    HomeLocation(Option<sl_wire::HomeLocationRequest>),
    /// A script upload, stored: what the item now holds is `source`, and the
    /// answer carries whether it compiled. The compiler is the grid's, so the
    /// driver compiles it, applies what a compile does to the script, and
    /// answers with [`SimCaps::script_upload_response`].
    ScriptUpload(UploadedScript),
}

/// A script upload the session has stored, for the driver to compile
/// ([`CapsDispatch::ScriptUpload`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedScript {
    /// The script item the upload was for, and what the request said.
    pub item: UploadedScriptItem,
    /// The source, as uploaded.
    pub source: Vec<u8>,
    /// The asset the source was stored as.
    pub new_asset: crate::AssetKey,
    /// The item the completion names, by the session's
    /// [`set_update_completion_names_item`](SimSession::set_update_completion_names_item).
    pub new_inventory_item: Option<InventoryKey>,
}

impl UploadedScript {
    /// The answer a grid with no compiler would give — `compiled`, no
    /// errors, the stored asset — for a driver that compiles nothing.
    #[must_use]
    pub const fn compiled_unchecked(&self) -> ScriptUploadAnswer {
        ScriptUploadAnswer {
            new_asset: self.new_asset.uuid(),
            new_inventory_item: self.new_inventory_item,
            compiled: true,
            errors: Vec::new(),
            inventory_flags: None,
        }
    }
}

/// Which script an [`UploadedScript`] is, and the request that named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadedScriptItem {
    /// A script in a prim (`UpdateScriptTask`).
    Task(sl_wire::UpdateScriptTaskRequest),
    /// A script in the agent's inventory (`UpdateScriptAgent`).
    Agent(sl_wire::UpdateScriptAgentRequest),
}

impl UploadedScriptItem {
    /// The compile target the request named (`mono`, `lsl2`, `luau`, …).
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::Task(request) => &request.target,
            Self::Agent(request) => &request.target,
        }
    }

    /// The script item.
    #[must_use]
    pub const fn item_id(&self) -> InventoryKey {
        match self {
            Self::Task(request) => request.item_id,
            Self::Agent(request) => request.item_id,
        }
    }
}

/// What a driver answers a [`CapsDispatch::ScriptUpload`] with
/// ([`SimCaps::script_upload_response`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptUploadAnswer {
    /// The id the answer names as `new_asset`: the stored asset on Second
    /// Life, the item's own id for a task upload on OpenSim.
    pub new_asset: Uuid,
    /// The item the answer names, if any.
    pub new_inventory_item: Option<InventoryKey>,
    /// Whether the source compiled.
    pub compiled: bool,
    /// The compile errors, each as the grid words it.
    pub errors: Vec<String>,
    /// The item's inventory flags, when the upload changed them (a Luau
    /// script's subtype).
    pub inventory_flags: Option<u32>,
}

/// What a driver answers a [`CapsDispatch::UntrustedMessage`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntrustedMessageOutcome {
    /// A message the grid knows, taken — whether or not it then acts on it.
    Taken,
    /// A god kick whose god session is not the god's presence (OpenSim's
    /// check; Second Life was not seen to make it).
    WrongSession,
    /// A message the grid does not know.
    Unknown,
}

/// How a grid words its answers to `UntrustedSimulatorMessage`, measured by
/// the `god-tools` census on 2026-10-18.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntrustedMessageStyle {
    /// OpenSim's `GodsModule`: `200`, `401` and `400`, none with a body.
    Bare,
    /// Second Life: `200` with an `undef` LLSD body for any god kick, and
    /// `404` "Not Found" for a message it does not know.
    Llsd,
}

/// The media type OpenSim's HTTP server states for a handler that sets none.
const OPENSIM_DEFAULT_CONTENT_TYPE: &str = "text/html;charset=UTF-8";

/// How a grid labels an LLSD answer whose handler may state no media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlsdLabel {
    /// `application/llsd+xml`, as Second Life labels it.
    #[default]
    Llsd,
    /// The HTTP server's default, `text/html;charset=UTF-8`: OpenSim's, for a
    /// handler that states none.
    ServerDefault,
}

/// How a grid words its answers to `HomeLocation`, measured by the
/// `landmarks-home` census on 2026-10-19.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeLocationStyle {
    /// OpenSim's `BunchOfCaps`: `success` as a string, the position only when
    /// stored, `text/html` as the media type.
    OpenSim,
    /// Second Life: `success` as a string, the position asked for echoed
    /// whether stored or not, LLSD as the media type.
    SecondLife,
}

/// What a driver answers a [`CapsDispatch::HomeLocation`] with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HomeLocationOutcome {
    /// The location was stored, at this position.
    Stored(sl_types::map::RegionCoordinates),
    /// The location asked for, at this position, was refused.
    Refused(sl_types::map::RegionCoordinates),
    /// The body named no location.
    NoLocation,
}

/// What a driver answers a [`CapsDispatch::ModifyRegion`] with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModifyRegionOutcome<'overrides> {
    /// A read: the region's overrides.
    Read(&'overrides sl_wire::TerrainMaterialOverrides),
    /// A write the region took.
    Written,
    /// A write by an agent who manages no estate of the region's — refused
    /// naming the region, by its id.
    Refused(uuid::Uuid),
}

/// What a request path resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedPath {
    /// The seed capability itself.
    Seed,
    /// A granted capability, by registered name.
    Capability(&'static str),
    /// No granted capability matches.
    Unknown,
}

/// The server-side CAPS surface of one simulator session: the seed grant and
/// the per-capability dispatch registry.
///
/// Sans-I/O and login-free: construct it with the region's public base URL
/// and caller-supplied token randomness, hand [`SimCaps::seed_url`] to
/// whatever builds the login response (possibly another process), and route
/// every request under the base URL through [`SimCaps::dispatch`].
///
/// All capability tokens are minted up front in [`SimCaps::new`], so
/// [`SimCaps::grant`] is a pure read: a retried seed POST (the reference
/// viewer retries up to 30×) is answered with a byte-identical grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimCaps {
    /// The base under which capability URLs are minted
    /// (`{base}/cap/{token}`). Must be an HTTP(S) URL.
    base_url: Url,
    /// The seed capability's own unguessable URL token.
    seed_token: Uuid,
    /// One pre-minted URL token per served capability
    /// ([`SERVED_CAPABILITIES`]).
    tokens: BTreeMap<&'static str, Uuid>,
    /// The composed asset-delivery surface, held so a single seed grant
    /// advertises the asset caps alongside the sim caps. Its dispatch path is
    /// independent (it needs an [`AssetSource`](crate::AssetSource), not a
    /// [`SimSession`]) and may serve from a different process.
    assets: AssetCaps,
    /// Set once the client has polled with `done`; later polls answer `404`.
    event_queue_done: bool,
    /// Which grid's `NewFileAgentInventory` this surface serves.
    new_file_policy: NewFilePolicy,
    /// Which grid's people service `GetDisplayNames` answers as.
    display_name_style: DisplayNameStyle,
    /// The land products `ProductInfoRequest` lists.
    land_products: Vec<crate::LandProduct>,
}

/// Which grid's people service a `GetDisplayNames` answer imitates, for a
/// record that still has the default display name (`profiles`, 2026-10-15,
/// `book/src/gridspec/profiles.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayNameStyle {
    /// The records as they were stored, every id unknown to the store listed
    /// under `bad_ids`.
    #[default]
    Plain,
    /// Second Life: a Resident's default display name is its first name
    /// alone, the record has no `display_name_expires`, and an unknown id is
    /// listed under `bad_ids`.
    SecondLife,
    /// OpenSim: the user name is the legacy name, space and capitals and all,
    /// and an unknown id is left out of the answer altogether.
    OpenSim,
}

impl DisplayNameStyle {
    /// `record` as this grid answers it; `None` for an unknown id the grid
    /// leaves out.
    fn answer(self, record: DisplayName) -> Option<DisplayName> {
        if record.missing {
            return (self != Self::OpenSim).then_some(record);
        }
        if !record.is_display_name_default {
            return Some(record);
        }
        let legacy = format!("{} {}", record.legacy_first_name, record.legacy_last_name);
        Some(match self {
            Self::Plain => record,
            Self::SecondLife => DisplayName {
                display_name: if record.legacy_last_name.eq_ignore_ascii_case("Resident") {
                    record.legacy_first_name.clone()
                } else {
                    legacy
                },
                display_name_expires: String::new(),
                ..record
            },
            Self::OpenSim => DisplayName {
                username: legacy.clone(),
                display_name: legacy,
                ..record
            },
        })
    }
}

impl SimCaps {
    /// Creates the CAPS surface for one agent's presence on a region, with the
    /// asset caps served from the **same** base URL as the sim caps (the
    /// OpenSim-style co-located layout).
    ///
    /// `base_url` is the public HTTP(S) base the capability URLs are minted
    /// under; `seed_token` is the seed capability's own URL token (the login
    /// side embeds the resulting [`SimCaps::seed_url`] in its login
    /// response); `mint_token` supplies the randomness for the per-capability
    /// tokens — sans-I/O purity means the caller owns randomness (a runtime
    /// passes `Uuid::new_v4`, tests pass a deterministic counter). The asset
    /// tokens come from the same `mint_token` stream, so one call seeds every
    /// capability.
    pub fn new(base_url: Url, seed_token: Uuid, mut mint_token: impl FnMut() -> Uuid) -> Self {
        let tokens = SERVED_CAPABILITIES
            .iter()
            .map(|name| (*name, mint_token()))
            .collect::<BTreeMap<&'static str, Uuid>>();
        let assets = AssetCaps::new(base_url.clone(), &mut mint_token);
        Self {
            base_url,
            seed_token,
            tokens,
            assets,
            event_queue_done: false,
            new_file_policy: NewFilePolicy::Plain,
            display_name_style: DisplayNameStyle::Plain,
            land_products: Vec::new(),
        }
    }

    /// The same surface granting `assets` as its asset-delivery capabilities
    /// instead of the ones it minted — a surface another process serves, or
    /// one laid out [at the root](AssetCaps::at_the_root) of an asset host.
    #[must_use]
    pub fn with_asset_surface(mut self, assets: AssetCaps) -> Self {
        self.assets = assets;
        self
    }

    /// The same surface with its `NewFileAgentInventory` answering as
    /// `policy` says — which live grid's uploader it is.
    #[must_use]
    pub const fn with_new_file_policy(mut self, policy: NewFilePolicy) -> Self {
        self.new_file_policy = policy;
        self
    }

    /// The same surface with its `GetDisplayNames` answering as `style`
    /// says — which live grid's people service it is.
    #[must_use]
    pub const fn with_display_name_style(mut self, style: DisplayNameStyle) -> Self {
        self.display_name_style = style;
        self
    }

    /// The same surface with its `ProductInfoRequest` listing `products` —
    /// the land types a land row's `ProductSKU` is named by. Empty unless set.
    #[must_use]
    pub fn with_land_products(mut self, products: Vec<crate::LandProduct>) -> Self {
        self.land_products = products;
        self
    }

    /// The same surface with its asset-delivery capabilities answering as
    /// `policy` says — which live grid's HTTP they speak.
    #[must_use]
    pub fn with_asset_http_policy(mut self, policy: crate::AssetHttpPolicy) -> Self {
        self.assets = self.assets.with_http_policy(policy);
        self
    }

    /// Creates the CAPS surface with the asset caps minted under a **separate**
    /// base URL — a content delivery network on a different host from the
    /// simulator, as Second Life serves them.
    ///
    /// The sim caps and seed are minted under `sim_base`; the four asset caps
    /// under `asset_base`. Both sets of URLs are advertised by the one seed
    /// grant, but the asset URLs point at the CDN host; a CDN process rebuilds
    /// the asset surface with [`AssetCaps::from_tokens`] from
    /// [`SimCaps::assets`]'s [`tokens`](AssetCaps::tokens).
    pub fn new_split(
        sim_base: Url,
        asset_base: Url,
        seed_token: Uuid,
        mut mint_token: impl FnMut() -> Uuid,
    ) -> Self {
        let tokens = SERVED_CAPABILITIES
            .iter()
            .map(|name| (*name, mint_token()))
            .collect::<BTreeMap<&'static str, Uuid>>();
        let assets = AssetCaps::new(asset_base, &mut mint_token);
        Self {
            base_url: sim_base,
            seed_token,
            tokens,
            assets,
            event_queue_done: false,
            new_file_policy: NewFilePolicy::Plain,
            display_name_style: DisplayNameStyle::Plain,
            land_products: Vec::new(),
        }
    }

    /// The composed asset-delivery surface. The in-process HTTP glue routes an
    /// asset request here — `if caps.assets().handles_path(path) {
    /// caps.assets().dispatch(source, &req) }` — because that path needs the
    /// [`AssetSource`](crate::AssetSource), not the [`SimSession`], and
    /// everything else through [`SimCaps::dispatch`].
    #[must_use]
    pub const fn assets(&self) -> &AssetCaps {
        &self.assets
    }

    /// The handler for a served capability name — the dispatch registry.
    ///
    /// Every [`SERVED_CAPABILITIES`] entry maps to a [`CapHandler`] variant
    /// here; the `protocol-sim-caps-*` cluster tasks grow both together.
    fn handler_for(name: &str) -> Option<CapHandler> {
        match name {
            EVENT_QUEUE_GET => Some(CapHandler::EventQueue),
            CAP_CHAT_SESSION_REQUEST => Some(CapHandler::ChatSession),
            CAP_READ_OFFLINE_MSGS => Some(CapHandler::OfflineMessages),
            CAP_ACCEPT_FRIENDSHIP | CAP_DECLINE_FRIENDSHIP => {
                Some(CapHandler::KeptFriendshipAnswer)
            }
            CAP_ACCEPT_GROUP_INVITE | CAP_DECLINE_GROUP_INVITE => {
                Some(CapHandler::KeptGroupInvitationAnswer)
            }
            CAP_GROUP_MEMBER_DATA => Some(CapHandler::GroupMemberData),
            CAP_GROUP_API_V1 => Some(CapHandler::GroupBans),
            CAP_GET_DISPLAY_NAMES => Some(CapHandler::DisplayNames),
            CAP_AVATAR_PICKER_SEARCH => Some(CapHandler::AvatarPickerSearch),
            CAP_AGENT_PREFERENCES => Some(CapHandler::AgentPreferences),
            CAP_INTEREST_LIST => Some(CapHandler::InterestList),
            CAP_REQUEST_TASK_INVENTORY => Some(CapHandler::TaskInventory),
            CAP_GET_METADATA => Some(CapHandler::ScriptMetadata),
            CAP_SEND_USER_REPORT => Some(CapHandler::UserReport),
            CAP_SEND_USER_REPORT_WITH_SCREENSHOT => Some(CapHandler::UserReportScreenshot),
            CAP_MESH_UPLOAD_FLAG => Some(CapHandler::MeshUploadFlag),
            CAP_INVENTORY_THUMBNAIL_UPLOAD => Some(CapHandler::ThumbnailUpload),
            CAP_SEND_POSTCARD => Some(CapHandler::Postcard),
            CAP_UPLOAD_AGENT_PROFILE_IMAGE => Some(CapHandler::ProfileImageUpload),
            CAP_SET_DISPLAY_NAME => Some(CapHandler::SetDisplayName),
            CAP_SEARCH_STAT_REQUEST => Some(CapHandler::SearchStat),
            CAP_PRODUCT_INFO_REQUEST => Some(CapHandler::ProductInfo),
            CAP_UPDATE_AVATAR_APPEARANCE => Some(CapHandler::AvatarAppearance),
            CAP_INCREMENT_COF_VERSION => Some(CapHandler::IncrementCofVersion),
            CAP_COPY_INVENTORY_FROM_NOTECARD => Some(CapHandler::CopyInventoryFromNotecard),
            CAP_RENDER_MATERIALS => Some(CapHandler::RenderMaterials),
            CAP_MODIFY_MATERIAL_PARAMS => Some(CapHandler::ModifyMaterialParams),
            CAP_OBJECT_MEDIA => Some(CapHandler::ObjectMedia),
            CAP_OBJECT_MEDIA_NAVIGATE => Some(CapHandler::ObjectMediaNavigate),
            CAP_FETCH_INVENTORY | CAP_FETCH_LIBRARY => Some(CapHandler::FetchDescendents),
            CAP_FETCH_INVENTORY_ITEM | CAP_FETCH_LIBRARY_ITEM => Some(CapHandler::FetchItems),
            CAP_INVENTORY_API_V3 | CAP_LIBRARY_API_V3 => Some(CapHandler::Ais3),
            CAP_CREATE_INVENTORY_CATEGORY => Some(CapHandler::CreateInventoryCategory),
            CAP_SIMULATOR_FEATURES => Some(CapHandler::SimulatorFeatures),
            CAP_LSL_SYNTAX => Some(CapHandler::LslSyntax),
            CAP_EXT_ENVIRONMENT => Some(CapHandler::Environment),
            CAP_REMOTE_PARCEL_REQUEST => Some(CapHandler::RemoteParcel),
            CAP_PARCEL_PROPERTIES_UPDATE => Some(CapHandler::ParcelUpdate),
            CAP_GET_OBJECT_COST => Some(CapHandler::ObjectCost),
            CAP_GET_OBJECT_PHYSICS_DATA => Some(CapHandler::ObjectPhysics),
            CAP_RESOURCE_COST_SELECTED => Some(CapHandler::ResourceCostSelected),
            CAP_ATTACHMENT_RESOURCES => Some(CapHandler::AttachmentResources),
            CAP_AVATAR_RENDER_INFO => Some(CapHandler::AvatarRenderInfo),
            CAP_LAND_RESOURCES => Some(CapHandler::LandResources),
            CAP_GET_EXPERIENCE_INFO => Some(CapHandler::ExperienceInfo),
            CAP_FIND_EXPERIENCE_BY_NAME => Some(CapHandler::ExperienceSearch),
            CAP_GET_EXPERIENCES => Some(CapHandler::ExperiencePermissions),
            CAP_EXPERIENCE_PREFERENCES => Some(CapHandler::ExperiencePreferences),
            CAP_AGENT_EXPERIENCES
            | CAP_GET_ADMIN_EXPERIENCES
            | CAP_GET_CREATOR_EXPERIENCES
            | CAP_GROUP_EXPERIENCES => Some(CapHandler::ExperienceIdList),
            CAP_IS_EXPERIENCE_ADMIN | CAP_IS_EXPERIENCE_CONTRIBUTOR => {
                Some(CapHandler::ExperienceStatus)
            }
            CAP_UPDATE_EXPERIENCE => Some(CapHandler::UpdateExperience),
            CAP_REGION_EXPERIENCES => Some(CapHandler::RegionExperiences),
            CAP_ESTATE_ACCESS => Some(CapHandler::EstateAccess),
            CAP_DISPATCH_REGION_INFO => Some(CapHandler::DispatchRegionInfo),
            CAP_REGION_SCHEDULE => Some(CapHandler::RegionSchedule),
            CAP_MODIFY_REGION => Some(CapHandler::ModifyRegion),
            CAP_DIRECT_DELIVERY => Some(CapHandler::DirectDelivery),
            CAP_HOME_LOCATION => Some(CapHandler::HomeLocation),
            CAP_UNTRUSTED_SIMULATOR_MESSAGE => Some(CapHandler::UntrustedSimulatorMessage),
            CAP_EXPERIENCE_QUERY => Some(CapHandler::ExperienceQuery),
            CAP_PROVISION_VOICE_ACCOUNT => Some(CapHandler::ProvisionVoiceAccount),
            CAP_PARCEL_VOICE_INFO => Some(CapHandler::ParcelVoiceInfo),
            CAP_VOICE_SIGNALING => Some(CapHandler::VoiceSignaling),
            // Every two-stage upload cap shares one handler; the cap name only
            // picks the step-1 metadata parser inside it.
            name if UPLOAD_CAPABILITIES.contains(&name) => Some(CapHandler::AssetUpload),
            _ => None,
        }
    }

    /// The seed capability URL — the value the login response's
    /// `seed_capability` field carries to the client.
    #[must_use]
    pub fn seed_url(&self) -> Url {
        self.cap_url(self.seed_token)
    }

    /// Whether this simulator can serve the named capability.
    #[must_use]
    pub fn supports(&self, name: &str) -> bool {
        self.tokens.contains_key(name)
    }

    /// Withdraws a capability this surface would otherwise serve, so the seed
    /// grant never advertises it and its URL never resolves. Answers whether
    /// there was one to withdraw.
    ///
    /// The served-capability table is what a simulator *can* serve, not what
    /// every simulator does: capability negotiation is how a grid tells a client
    /// which of two protocols it speaks, and a client that finds a capability
    /// missing is expected to take the other road. `UpdateAvatarAppearance` is
    /// the worked example — a grid that does not central-bake avatars offers no
    /// bake trigger, and a viewer that finds none bakes the avatar itself.
    ///
    /// Withholding is per-agent-presence, because that is the granularity a
    /// [`SimCaps`] has; a grid-wide policy withholds on every session it mints.
    pub fn withhold(&mut self, name: &str) -> bool {
        self.tokens.remove(name).is_some()
    }

    /// Grants capability URLs for the requested names — the server side of
    /// the seed round-trip. Merges the sim-cap grant with the composed
    /// [`AssetCaps`]'s grant so one response advertises every capability, sim
    /// and asset alike (the asset URLs may point at a different host).
    /// Unsupported names are silently omitted (the protocol's feature
    /// negotiation); requested order is irrelevant since the response is a
    /// map. Pure and stable: equal requests yield equal grants, which
    /// [`build_seed_response`] serializes byte-identically.
    #[must_use]
    pub fn grant(&self, requested: &[String]) -> HashMap<String, String> {
        let mut granted = requested
            .iter()
            .filter_map(|name| {
                self.tokens
                    .get(name.as_str())
                    .map(|token| (name.clone(), self.cap_url(*token).to_string()))
            })
            .collect::<HashMap<String, String>>();
        granted.extend(self.assets.grant(requested));
        granted
    }

    /// Routes one CAPS request to its handler.
    ///
    /// Outcomes: an unknown URL answers `404`; the seed URL answers the
    /// grant (`POST` only); the `EventQueueGet` URL implements the long-poll
    /// contract — events now (`200 { id, events }`), nothing queued
    /// ([`CapsDispatch::EventQueueWouldBlock`]; the runtime holds and
    /// eventually answers [`SimCaps::event_queue_timeout`]), `done=true`
    /// teardown (`200`, then `404` for every later poll), and `404` once the
    /// session is closed. The agent-communication capabilities
    /// (`ChatSessionRequest`, `ReadOfflineMsgs`, `GetDisplayNames`,
    /// `AgentPreferences`, `SendUserReport`,
    /// `SendUserReportWithScreenshot`) answer immediately from and into the
    /// [`SimSession`]'s state; each handler documents its own status
    /// contract.
    ///
    /// The `ack` field is parsed but deliberately fire-and-forget, exactly
    /// like OpenSim's event-queue module: a batch is dropped when
    /// [`SimSession::take_event_queue_response`] serializes it, so a response
    /// lost in transit loses that batch. The batch id may also wrap negative
    /// after `i32::MAX` polls — harmless, since nothing keys on `ack`. A
    /// future task can add ack-keyed retention if a conformance case ever
    /// demands it.
    pub fn dispatch(&mut self, sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsDispatch {
        match self.resolve(request.path) {
            ResolvedPath::Unknown => CapsDispatch::Response(CapsResponse::not_found()),
            ResolvedPath::Seed => CapsDispatch::Response(self.respond_seed(request)),
            ResolvedPath::Capability(name) => {
                match Self::handler_for(name) {
                    Some(CapHandler::EventQueue) => self.dispatch_event_queue(sim, request),
                    Some(CapHandler::ChatSession) => {
                        CapsDispatch::Response(Self::dispatch_chat_session(sim, request))
                    }
                    Some(CapHandler::OfflineMessages) => {
                        CapsDispatch::Response(Self::dispatch_offline_messages(sim, request))
                    }
                    Some(CapHandler::KeptFriendshipAnswer) => CapsDispatch::Response(
                        Self::dispatch_kept_friendship_answer(sim, request, name),
                    ),
                    Some(CapHandler::KeptGroupInvitationAnswer) => CapsDispatch::Response(
                        Self::dispatch_kept_group_invitation_answer(sim, request, name),
                    ),
                    Some(CapHandler::GroupMemberData) => Self::dispatch_group_member_data(request),
                    Some(CapHandler::GroupBans) => Self::dispatch_group_bans(request),
                    Some(CapHandler::EstateAccess) => {
                        if request.method == "GET" {
                            CapsDispatch::EstateAccess
                        } else {
                            CapsDispatch::Response(CapsResponse::method_not_allowed())
                        }
                    }
                    Some(CapHandler::DispatchRegionInfo) => Self::dispatch_region_info(request),
                    Some(CapHandler::RegionSchedule) => Self::dispatch_region_schedule(request),
                    Some(CapHandler::ModifyRegion) => Self::dispatch_modify_region(request),
                    Some(CapHandler::DirectDelivery) => {
                        CapsDispatch::Response(Self::dispatch_direct_delivery())
                    }
                    Some(CapHandler::HomeLocation) => Self::dispatch_home_location(request),
                    Some(CapHandler::UntrustedSimulatorMessage) => {
                        Self::dispatch_untrusted_message(request)
                    }
                    Some(CapHandler::DisplayNames) => {
                        CapsDispatch::Response(self.dispatch_display_names(sim, request))
                    }
                    Some(CapHandler::AvatarPickerSearch) => {
                        CapsDispatch::Response(Self::dispatch_avatar_picker_search(sim, request))
                    }
                    Some(CapHandler::AgentPreferences) => {
                        CapsDispatch::Response(Self::dispatch_agent_preferences(sim, request))
                    }
                    Some(CapHandler::InterestList) => {
                        CapsDispatch::Response(Self::dispatch_interest_list(sim, request))
                    }
                    Some(CapHandler::TaskInventory) => Self::dispatch_task_inventory(request),
                    Some(CapHandler::ScriptMetadata) => Self::dispatch_script_metadata(request),
                    Some(CapHandler::UserReport) => {
                        CapsDispatch::Response(Self::dispatch_user_report(sim, request))
                    }
                    Some(CapHandler::UserReportScreenshot) => {
                        CapsDispatch::Response(self.dispatch_user_report_screenshot(sim, request))
                    }
                    Some(CapHandler::MeshUploadFlag) => {
                        CapsDispatch::Response(self.dispatch_mesh_upload_flag(sim, request))
                    }
                    Some(CapHandler::ThumbnailUpload) => CapsDispatch::Response(
                        self.dispatch_side_upload(sim, request, CAP_INVENTORY_THUMBNAIL_UPLOAD),
                    ),
                    Some(CapHandler::Postcard) => CapsDispatch::Response(
                        self.dispatch_side_upload(sim, request, CAP_SEND_POSTCARD),
                    ),
                    Some(CapHandler::ProfileImageUpload) => CapsDispatch::Response(
                        self.dispatch_side_upload(sim, request, CAP_UPLOAD_AGENT_PROFILE_IMAGE),
                    ),
                    Some(CapHandler::SetDisplayName) => {
                        CapsDispatch::Response(Self::dispatch_set_display_name(sim, request))
                    }
                    Some(CapHandler::SearchStat) => {
                        CapsDispatch::Response(Self::dispatch_search_stat(request))
                    }
                    Some(CapHandler::ProductInfo) => {
                        CapsDispatch::Response(self.dispatch_product_info(request))
                    }
                    Some(CapHandler::AssetUpload) => self.dispatch_caps_upload(sim, request, name),
                    Some(CapHandler::AvatarAppearance) => CapsDispatch::Response(
                        Self::dispatch_update_avatar_appearance(sim, request),
                    ),
                    Some(CapHandler::IncrementCofVersion) => {
                        CapsDispatch::Response(Self::dispatch_increment_cof_version(sim, request))
                    }
                    Some(CapHandler::CopyInventoryFromNotecard) => {
                        Self::dispatch_copy_inventory_from_notecard(request)
                    }
                    Some(CapHandler::RenderMaterials) => {
                        CapsDispatch::Response(Self::dispatch_render_materials(sim, request))
                    }
                    Some(CapHandler::ModifyMaterialParams) => {
                        CapsDispatch::Response(Self::dispatch_modify_material_params(sim, request))
                    }
                    Some(CapHandler::ObjectMedia) => {
                        CapsDispatch::Response(Self::dispatch_object_media(sim, request))
                    }
                    Some(CapHandler::ObjectMediaNavigate) => {
                        CapsDispatch::Response(Self::dispatch_object_media_navigate(sim, request))
                    }
                    Some(CapHandler::FetchDescendents) => {
                        CapsDispatch::Response(Self::dispatch_fetch_descendents(sim, request, name))
                    }
                    Some(CapHandler::FetchItems) => {
                        CapsDispatch::Response(Self::dispatch_fetch_items(sim, request, name))
                    }
                    Some(CapHandler::Ais3) => {
                        CapsDispatch::Response(Self::dispatch_ais3(sim, request, name))
                    }
                    Some(CapHandler::CreateInventoryCategory) => CapsDispatch::Response(
                        Self::dispatch_create_inventory_category(sim, request),
                    ),
                    Some(CapHandler::SimulatorFeatures) => {
                        CapsDispatch::Response(Self::dispatch_simulator_features(sim, request))
                    }
                    Some(CapHandler::LslSyntax) => {
                        CapsDispatch::Response(Self::dispatch_lsl_syntax(sim, request))
                    }
                    Some(CapHandler::Environment) => {
                        CapsDispatch::Response(Self::dispatch_environment(sim, request))
                    }
                    Some(CapHandler::RemoteParcel) => {
                        CapsDispatch::Response(Self::dispatch_remote_parcel(sim, request))
                    }
                    Some(CapHandler::ParcelUpdate) => {
                        CapsDispatch::Response(Self::dispatch_parcel_update(sim, request))
                    }
                    Some(CapHandler::ObjectCost) => {
                        CapsDispatch::Response(Self::dispatch_object_cost(sim, request))
                    }
                    Some(CapHandler::ObjectPhysics) => {
                        CapsDispatch::Response(Self::dispatch_object_physics(sim, request))
                    }
                    Some(CapHandler::ResourceCostSelected) => {
                        CapsDispatch::Response(Self::dispatch_resource_cost_selected(sim, request))
                    }
                    Some(CapHandler::AttachmentResources) => {
                        CapsDispatch::Response(Self::dispatch_attachment_resources(sim, request))
                    }
                    Some(CapHandler::AvatarRenderInfo) => {
                        Self::dispatch_avatar_render_info(sim, request)
                    }
                    Some(CapHandler::LandResources) => {
                        CapsDispatch::Response(self.dispatch_land_resources(sim, request))
                    }
                    Some(CapHandler::ExperienceInfo) => {
                        CapsDispatch::Response(Self::dispatch_experience_info(sim, request))
                    }
                    Some(CapHandler::ExperienceSearch) => {
                        CapsDispatch::Response(Self::dispatch_experience_search(sim, request))
                    }
                    Some(CapHandler::ExperiencePermissions) => {
                        CapsDispatch::Response(Self::dispatch_experience_permissions(sim, request))
                    }
                    Some(CapHandler::ExperiencePreferences) => {
                        CapsDispatch::Response(Self::dispatch_experience_preferences(sim, request))
                    }
                    Some(CapHandler::ExperienceIdList) => CapsDispatch::Response(
                        Self::dispatch_experience_id_list(sim, request, name),
                    ),
                    Some(CapHandler::ExperienceStatus) => {
                        CapsDispatch::Response(Self::dispatch_experience_status(sim, request, name))
                    }
                    Some(CapHandler::UpdateExperience) => {
                        CapsDispatch::Response(Self::dispatch_update_experience(sim, request))
                    }
                    Some(CapHandler::RegionExperiences) => {
                        CapsDispatch::Response(Self::dispatch_region_experiences(sim, request))
                    }
                    Some(CapHandler::ExperienceQuery) => {
                        CapsDispatch::Response(Self::dispatch_experience_query(sim, request))
                    }
                    Some(CapHandler::ProvisionVoiceAccount) => {
                        CapsDispatch::Response(Self::dispatch_provision_voice_account(sim, request))
                    }
                    Some(CapHandler::ParcelVoiceInfo) => {
                        CapsDispatch::Response(Self::dispatch_parcel_voice_info(sim, request))
                    }
                    Some(CapHandler::VoiceSignaling) => {
                        CapsDispatch::Response(Self::dispatch_voice_signaling(sim, request))
                    }
                    // Tokens are only minted for served capabilities, so a
                    // resolved name always has a handler; answer 404 rather than
                    // panic if that invariant is ever broken.
                    None => CapsDispatch::Response(CapsResponse::not_found()),
                }
            }
        }
    }

    /// The response a held empty event-queue poll receives once the
    /// runtime's hold (~30 s in the reference stack) expires: `502`, which
    /// the client treats as "nothing yet" and re-polls.
    #[must_use]
    pub const fn event_queue_timeout(&self) -> CapsResponse {
        CapsResponse::bad_gateway()
    }

    /// Serves the seed capability: parse the requested names, answer the
    /// grant. Idempotent by construction, so the reference viewer's seed
    /// retries all receive identical bodies.
    fn respond_seed(&self, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(body) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(names) = parse_seed_request(body) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_seed_response(&self.grant(&names)))
    }

    /// Serves one `EventQueueGet` long-poll request against the session's
    /// event buffer.
    fn dispatch_event_queue(
        &mut self,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsDispatch {
        if sim.is_closed() || self.event_queue_done {
            return CapsDispatch::Response(CapsResponse::not_found());
        }
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        let Ok(body) = std::str::from_utf8(request.body) else {
            return CapsDispatch::Response(CapsResponse::bad_request());
        };
        let Ok(poll) = parse_event_queue_request(body) else {
            return CapsDispatch::Response(CapsResponse::bad_request());
        };
        if poll.done {
            self.event_queue_done = true;
            return CapsDispatch::Response(CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned()));
        }
        match sim.take_event_queue_response() {
            Some(xml) => CapsDispatch::Response(CapsResponse::llsd_xml(xml)),
            None => CapsDispatch::EventQueueWouldBlock,
        }
    }

    /// Serves one `ChatSessionRequest` POST: routes on the body's `method`
    /// member. `"start conference"` registers an ad-hoc conference of the
    /// body's `params` invitees and answers its roster (`400` when the body
    /// names none); `"invite"` adds those invitees to a session that already
    /// exists, answering its grown roster (`400` for an unknown session);
    /// `"accept invitation"` answers the
    /// session's roster (an empty
    /// `agent_info` map for an unknown session — tolerant, mirroring
    /// OpenSim's stubbed cap); `"decline invitation"` drops this agent from
    /// the roster and acks with an undefined body; `"decline p2p voice"` is a
    /// pure ack; `"fetch history"` answers the session's server-side backlog
    /// (an empty array for an unknown session). Any other method — or a
    /// method-less / malformed body — answers `400`.
    fn dispatch_chat_session(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Some((method, session_uuid)) = chat_session_request_from_llsd(&body) else {
            return CapsResponse::bad_request();
        };
        let session_id = ImSessionId::from(session_uuid);
        match method.as_str() {
            CHAT_SESSION_ACCEPT => {
                let roster = sim.chat_session_accept(session_id).unwrap_or_default();
                CapsResponse::llsd_xml(chat_session_roster_to_llsd(&roster).to_llsd_xml())
            }
            CHAT_SESSION_DECLINE => {
                sim.chat_session_decline(session_id);
                CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
            }
            CHAT_SESSION_INVITE => {
                // Adding to a session that exists: the roster grows and the
                // driver relays the invitations, but no session is minted, so
                // no start reply follows.
                let invitees = chat_session_agent_params_from_llsd(&body);
                if invitees.is_empty() {
                    return CapsResponse::bad_request();
                }
                match sim.chat_session_invite(session_id, &invitees) {
                    Some(roster) => {
                        CapsResponse::llsd_xml(chat_session_roster_to_llsd(&roster).to_llsd_xml())
                    }
                    None => CapsResponse::bad_request(),
                }
            }
            CHAT_SESSION_START_CONFERENCE => {
                // The modern conference start: register the session with its
                // invitees and answer the roster. The real session id — which
                // a simulator is free to mint itself — is told to the client
                // afterwards over the event queue
                // ([`SimSession::enqueue_chatterbox_session_start_reply`]),
                // which is the driver's call, not the cap's.
                let invitees = chat_session_agent_params_from_llsd(&body);
                if invitees.is_empty() {
                    // A conference of nobody: the body named no `params`, or
                    // none of them was a uuid.
                    return CapsResponse::bad_request();
                }
                let roster = sim.chat_session_start_conference(session_id, &invitees);
                CapsResponse::llsd_xml(chat_session_roster_to_llsd(&roster).to_llsd_xml())
            }
            CHAT_SESSION_DECLINE_P2P_VOICE => CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned()),
            CHAT_SESSION_FETCH_HISTORY => {
                let history = sim.chat_session(session_id).map_or_else(
                    || Llsd::Array(Vec::new()),
                    |chat_session| session_history_to_llsd(&chat_session.history),
                );
                CapsResponse::llsd_xml(history.to_llsd_xml())
            }
            _ => CapsResponse::bad_request(),
        }
    }

    /// Serves one `ReadOfflineMsgs` GET: the messages stored while the agent
    /// was offline, serialized as the capability's array body. Deliver-once
    /// (OpenSim's delete-on-fetch): the fetch drains the store, so a repeated
    /// GET answers an empty array. The client sends no body, so none is
    /// parsed.
    fn dispatch_offline_messages(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let records: Vec<_> = sim
            .take_offline_messages()
            .into_iter()
            .map(|kept| kept.record)
            .collect();
        CapsResponse::llsd_xml(offline_messages_to_llsd(&records).to_llsd_xml())
    }

    /// Serves one answer to a kept friendship offer: an `AcceptFriendship`
    /// POST or a `DeclineFriendship` DELETE of `?from=<offerer>` (the accept
    /// also names the accepter, `&agent_name=`, which nothing here needs). The
    /// answer goes to the driver ([`ServerEvent::KeptFriendshipAnswered`]) and
    /// the request is answered `{success: true}`, as Second Life answers a
    /// decline of a kept offer (measured on aditi, 2026-10-14 —
    /// `book/src/gridspec/friends.md`). The wrong method → `405`; no `from`,
    /// or one that is not an id → `400`.
    fn dispatch_kept_friendship_answer(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
        name: &str,
    ) -> CapsResponse {
        let accept = name == CAP_ACCEPT_FRIENDSHIP;
        let method = if accept { "POST" } else { "DELETE" };
        if request.method != method {
            return CapsResponse::method_not_allowed();
        }
        let Some(from) = query_param(request.query, "from")
            .and_then(|from| Uuid::parse_str(from.trim_matches('"')).ok())
        else {
            return CapsResponse::bad_request();
        };
        sim.push_kept_friendship_answer(AgentKey::from(from), accept);
        CapsResponse::llsd_xml(
            Llsd::Map(HashMap::from([("success".to_owned(), Llsd::Boolean(true))])).to_llsd_xml(),
        )
    }

    /// Serves one answer to a kept group invitation: an `AcceptGroupInvite` or
    /// `DeclineGroupInvite` POST of `{group}`. The answer goes to the driver
    /// ([`ServerEvent::KeptGroupInvitationAnswered`]) and the request is
    /// answered `{success: true}`, as Second Life answers a decline of a kept
    /// invitation (measured on aditi, 2026-10-14 — `book/src/gridspec/groups.md`).
    /// The wrong method → `405`; a body that names no group → `400`.
    fn dispatch_kept_group_invitation_answer(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
        name: &str,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(group_id) = parse_llsd_body(request.body)
            .as_ref()
            .and_then(crate::group_invite_answer_from_llsd)
        else {
            return CapsResponse::bad_request();
        };
        sim.push_kept_group_invitation_answer(group_id, name == CAP_ACCEPT_GROUP_INVITE);
        CapsResponse::llsd_xml(
            Llsd::Map(HashMap::from([("success".to_owned(), Llsd::Boolean(true))])).to_llsd_xml(),
        )
    }

    /// Reads one `GroupMemberData` POST and hands it to the driver: the wrong
    /// method → `405`, a body that names no group → `400`.
    fn dispatch_group_member_data(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        match parse_llsd_body(request.body)
            .as_ref()
            .and_then(crate::group_member_data_request_from_llsd)
        {
            Some(group_id) => CapsDispatch::GroupMembers { group_id },
            None => CapsDispatch::Response(CapsResponse::bad_request()),
        }
    }

    /// Answers a `GroupMemberData` request ([`CapsDispatch::GroupMembers`])
    /// with `members`, the roster the driver holds for `group_id`.
    #[must_use]
    pub fn group_member_data_response(group_id: GroupKey, members: &[GroupMember]) -> CapsResponse {
        let event = Event::GroupMembers {
            group_id,
            request_id: Uuid::nil(),
            member_count: i32::try_from(members.len()).unwrap_or(i32::MAX),
            members: members.to_vec(),
        };
        match crate::group_members_to_caps_llsd(&event) {
            Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
            Err(_out_of_range) => CapsResponse::internal_error(),
        }
    }

    /// Reads one `GroupAPIv1` request and hands it to the driver: a GET of
    /// `?group_id=` reads the bans, a POST of a ban request changes them. The
    /// wrong method → `405`; no group, or a POST that is no ban request →
    /// `400`.
    fn dispatch_group_bans(request: &CapsRequest<'_>) -> CapsDispatch {
        let Some(group_id) = query_param(request.query, "group_id")
            .and_then(|group| Uuid::parse_str(group).ok())
            .map(GroupKey::from)
        else {
            return CapsDispatch::Response(CapsResponse::bad_request());
        };
        match request.method {
            "GET" => CapsDispatch::GroupBans {
                group_id,
                change: None,
            },
            "POST" => match parse_llsd_body(request.body)
                .as_ref()
                .and_then(crate::group_ban_request_from_llsd)
            {
                Some(change) => CapsDispatch::GroupBans {
                    group_id,
                    change: Some(change),
                },
                None => CapsDispatch::Response(CapsResponse::bad_request()),
            },
            _ => CapsDispatch::Response(CapsResponse::method_not_allowed()),
        }
    }

    /// Resolves a `DispatchRegionInfo` request: a POST of the settings, read
    /// into the update the UDP `setregioninfo` would carry plus its two extra
    /// fields.
    fn dispatch_region_info(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        match parse_llsd_body(request.body)
            .as_ref()
            .map(sl_wire::parse_dispatch_region_info_request)
        {
            Some(Ok(dispatch)) => {
                CapsDispatch::RegionInfoWrite(crate::RegionInfoUpdate::from_dispatch(&dispatch))
            }
            Some(Err(_)) | None => CapsDispatch::Response(CapsResponse::bad_request()),
        }
    }

    /// Resolves a `RegionSchedule` request: a GET reads, a POST writes.
    fn dispatch_region_schedule(request: &CapsRequest<'_>) -> CapsDispatch {
        match request.method {
            "GET" => CapsDispatch::RegionSchedule { change: None },
            "POST" => match parse_llsd_body(request.body)
                .as_ref()
                .map(sl_wire::parse_region_schedule)
            {
                Some(Ok(schedule)) => CapsDispatch::RegionSchedule {
                    change: Some(schedule),
                },
                Some(Err(_)) | None => CapsDispatch::Response(CapsResponse::bad_request()),
            },
            _ => CapsDispatch::Response(CapsResponse::method_not_allowed()),
        }
    }

    /// Resolves a `ModifyRegion` request: a GET reads, a POST writes.
    fn dispatch_modify_region(request: &CapsRequest<'_>) -> CapsDispatch {
        match request.method {
            "GET" => CapsDispatch::ModifyRegion { change: None },
            "POST" => match parse_llsd_body(request.body)
                .as_ref()
                .map(sl_wire::parse_modify_region_request)
            {
                Some(Ok(overrides)) => CapsDispatch::ModifyRegion {
                    change: Some(overrides),
                },
                Some(Err(_)) | None => CapsDispatch::Response(CapsResponse::bad_request()),
            },
            _ => CapsDispatch::Response(CapsResponse::method_not_allowed()),
        }
    }

    /// Resolves a `HomeLocation` POST into the location it asks for. A body
    /// that is no LLSD is a bad request; one that names no location is still
    /// asked, and refused by the driver.
    fn dispatch_home_location(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        match parse_llsd_body(request.body)
            .as_ref()
            .map(sl_wire::parse_home_location_request)
        {
            Some(Ok(asked)) => CapsDispatch::HomeLocation(asked),
            Some(Err(_)) | None => CapsDispatch::Response(CapsResponse::bad_request()),
        }
    }

    /// The answer to a [`CapsDispatch::HomeLocation`], in a grid's `style`
    /// (`landmarks-home`, 2026-10-19).
    #[must_use]
    pub fn home_location_response(
        style: HomeLocationStyle,
        outcome: HomeLocationOutcome,
    ) -> CapsResponse {
        use sl_wire::{HomeLocationSuccessForm as Form, build_home_location_answer as build};
        let answer = |success, position| sl_wire::HomeLocationAnswer { success, position };
        match (style, outcome) {
            // OpenSim's `BunchOfCaps.HomeLocation`: the string `success`, the
            // stored position only on success, and no media type of its own
            // (the HTTP server's default).
            (HomeLocationStyle::OpenSim, HomeLocationOutcome::Stored(position)) => CapsResponse {
                content_type: OPENSIM_DEFAULT_CONTENT_TYPE,
                ..CapsResponse::llsd_xml(build(&answer(true, Some(position)), Form::String))
            },
            (
                HomeLocationStyle::OpenSim,
                HomeLocationOutcome::Refused(_) | HomeLocationOutcome::NoLocation,
            ) => CapsResponse {
                content_type: OPENSIM_DEFAULT_CONTENT_TYPE,
                ..CapsResponse::llsd_xml(build(&answer(false, None), Form::String))
            },
            // Second Life: the string `success` and the position it was asked,
            // stored or not.
            (HomeLocationStyle::SecondLife, HomeLocationOutcome::Stored(position)) => {
                CapsResponse::llsd_xml(build(&answer(true, Some(position)), Form::String))
            }
            (HomeLocationStyle::SecondLife, HomeLocationOutcome::Refused(position)) => {
                CapsResponse::llsd_xml(build(&answer(false, Some(position)), Form::String))
            }
            // A body that names no location: Second Life answers `success`
            // `true` with an undefined position, and says nothing else.
            (HomeLocationStyle::SecondLife, HomeLocationOutcome::NoLocation) => {
                CapsResponse::llsd_xml(
                    Llsd::Map(HashMap::from([
                        ("success".to_owned(), Llsd::String("true".to_owned())),
                        (
                            "HomeLocation".to_owned(),
                            Llsd::Map(HashMap::from([("LocationPos".to_owned(), Llsd::Undef)])),
                        ),
                    ]))
                    .to_llsd_xml(),
                )
            }
        }
    }

    /// Resolves an `UntrustedSimulatorMessage` POST into the message it
    /// carries.
    fn dispatch_untrusted_message(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        match parse_llsd_body(request.body)
            .as_ref()
            .map(sl_wire::parse_untrusted_message)
        {
            Some(Ok(message)) => CapsDispatch::UntrustedMessage(message),
            Some(Err(_)) | None => CapsDispatch::Response(CapsResponse::bad_request()),
        }
    }

    /// The answer to a [`CapsDispatch::UntrustedMessage`] in a grid's
    /// `style`.
    #[must_use]
    pub fn untrusted_message_response(
        style: UntrustedMessageStyle,
        outcome: UntrustedMessageOutcome,
    ) -> CapsResponse {
        match (style, outcome) {
            (UntrustedMessageStyle::Bare, UntrustedMessageOutcome::Taken) => {
                CapsResponse::empty(200)
            }
            (UntrustedMessageStyle::Bare, UntrustedMessageOutcome::WrongSession) => {
                CapsResponse::unauthorized()
            }
            (UntrustedMessageStyle::Bare, UntrustedMessageOutcome::Unknown) => {
                CapsResponse::bad_request()
            }
            (
                UntrustedMessageStyle::Llsd,
                UntrustedMessageOutcome::Taken | UntrustedMessageOutcome::WrongSession,
            ) => CapsResponse::llsd_xml(Llsd::Undef.to_llsd_xml()),
            (UntrustedMessageStyle::Llsd, UntrustedMessageOutcome::Unknown) => {
                CapsResponse::words(404, "Not Found")
            }
        }
    }

    /// The answer to a [`CapsDispatch::ModifyRegion`], as Second Life gives
    /// it (`terrain-editing`, 2026-10-18): a read `{success, overrides}`, a
    /// taken write `{success}`, a refused one `403` and an LLSD answer naming
    /// the region and why.
    #[must_use]
    pub fn modify_region_response(outcome: ModifyRegionOutcome<'_>) -> CapsResponse {
        match outcome {
            ModifyRegionOutcome::Read(overrides) => CapsResponse::llsd_xml(
                sl_wire::build_modify_region_answer(&sl_wire::ModifyRegionAnswer {
                    success: true,
                    message: None,
                    overrides: Some(overrides.clone()),
                }),
            ),
            ModifyRegionOutcome::Written => CapsResponse::llsd_xml(
                sl_wire::build_modify_region_answer(&sl_wire::ModifyRegionAnswer {
                    success: true,
                    message: None,
                    overrides: None,
                }),
            ),
            ModifyRegionOutcome::Refused(region) => CapsResponse::llsd_xml_status(
                403,
                sl_wire::build_modify_region_answer(&sl_wire::ModifyRegionAnswer {
                    success: false,
                    message: Some(format!(
                        "Unable to modify region {region}: Not an estate owner or estate manager"
                    )),
                    overrides: None,
                }),
            ),
        }
    }

    /// The answer to a [`CapsDispatch::RegionInfoWrite`]: whether the driver
    /// `applied` it. Second Life refuses a resident `403` with the word
    /// `Forbidden` and no LLSD (`region-info`, 2026-10-17).
    #[must_use]
    pub fn region_info_write_response(applied: bool) -> CapsResponse {
        if applied {
            CapsResponse::llsd_xml(Llsd::Undef.to_llsd_xml())
        } else {
            CapsResponse::words(403, "Forbidden")
        }
    }

    /// The answer to a [`CapsDispatch::RegionSchedule`]: the region's
    /// `schedule` after the request, or `None` for a writer the grid refuses —
    /// `403` and a sentence, Second Life's answer to a resident
    /// (`region-info`, 2026-10-17).
    #[must_use]
    pub fn region_schedule_response(
        schedule: Option<Option<&sl_wire::RegionSchedule>>,
    ) -> CapsResponse {
        match schedule {
            Some(schedule) => {
                CapsResponse::llsd_xml(sl_wire::build_region_schedule(schedule, false))
            }
            None => CapsResponse::words(
                403,
                "Forbidden: You don't have permission to make this change.",
            ),
        }
    }

    /// The answer to a [`CapsDispatch::EstateAccess`]: the estate's `lists`
    /// written in `style`, or `None` for a reader the grid refuses — `401`,
    /// OpenSim's answer to a resident without estate rights.
    #[must_use]
    pub fn estate_access_response(
        lists: Option<(&EstateAccessLists, EstateAccessStyle)>,
    ) -> CapsResponse {
        match lists {
            Some((lists, style)) => {
                CapsResponse::llsd_xml(build_estate_access_response(lists, style))
            }
            None => CapsResponse::unauthorized(),
        }
    }

    /// Answers a `GroupAPIv1` read ([`CapsDispatch::GroupBans`] with no
    /// change) with `bans`, the list the driver holds for `group_id`.
    #[must_use]
    pub fn group_bans_response(group_id: GroupKey, bans: &[GroupBan]) -> CapsResponse {
        CapsResponse::llsd_xml(crate::group_bans_to_llsd(group_id, bans).to_llsd_xml())
    }

    /// Answers a `GroupAPIv1` change the driver made, as Second Life does: a
    /// ban `201` with the text "Created", a lifted ban `200` with the text
    /// "OK", neither LLSD (measured on aditi, 2026-10-14 —
    /// `book/src/gridspec/groups.md`).
    #[must_use]
    pub fn group_bans_changed_response(action: GroupBanAction) -> CapsResponse {
        match action {
            GroupBanAction::Ban => CapsResponse::words(201, "Created"),
            GroupBanAction::Unban => CapsResponse::words(200, "OK"),
        }
    }

    /// Serves one `GetDisplayNames` GET: looks each `ids` query parameter up
    /// in the session's display-name store. Known agents answer as full
    /// `agents` records, unknown ids as `bad_ids` (the grid's "could not
    /// resolve" form). No ids — or no query at all — answers an empty
    /// `agents` array; an `ids` value that is not a UUID is a `400`, since
    /// "unreadable" and "nobody by that id" are different answers.
    fn dispatch_display_names(&self, sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let query = request.query.unwrap_or_default();
        let Ok(ids) = parse_display_names_query(&format!("?{query}")) else {
            return CapsResponse::bad_request();
        };
        let records = ids
            .into_iter()
            .map(|id| {
                let agent = AgentKey::from(id);
                sim.display_name(agent)
                    .cloned()
                    .unwrap_or_else(|| DisplayName {
                        id: agent,
                        missing: true,
                        ..DisplayName::default()
                    })
            })
            .filter_map(|record| self.display_name_style.answer(record))
            .collect::<Vec<DisplayName>>();
        CapsResponse::llsd_xml(build_display_names_response(&records))
    }

    /// Serves one `SetDisplayName` POST of `{ display_name: [<old>, <new>] }`,
    /// as the reference expects it answered: the HTTP answer says nothing,
    /// and the outcome arrives over the event queue as a `SetDisplayNameReply`
    /// — `409` where the old name is not the agent's current one (the
    /// reference then fetches the name again), `400` for a name that is empty
    /// or longer than the reference's dialog takes, and `200` with the new
    /// name, followed by a `DisplayNameUpdate`, where it is taken. The change
    /// is handed to the driver to tell the rest of the region
    /// ([`ServerEvent::DisplayNameChanged`]).
    ///
    /// Not measured: a change is refused on Second Life once a week, which
    /// is why the census never makes one; this answers every valid change.
    fn dispatch_set_display_name(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        /// The longest display name the reference's dialog takes, in
        /// characters (`floater_display_name.xml`).
        const LONGEST_DISPLAY_NAME: usize = 31;
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some((old, new)) = std::str::from_utf8(request.body)
            .ok()
            .and_then(crate::parse_set_display_name_body)
        else {
            return CapsResponse::bad_request();
        };
        let Some(current) = sim
            .agent_id()
            .and_then(|agent| sim.display_name(agent))
            .cloned()
        else {
            return CapsResponse::not_found();
        };
        let refusal = |status: i32, reason: &str| crate::SetDisplayNameReply {
            status,
            reason: reason.to_owned(),
            new_display_name: None,
            error_tag: None,
        };
        if old != current.display_name {
            sim.enqueue_set_display_name_reply(&refusal(409, "Conflict"));
        } else if new.trim().is_empty() || new.chars().count() > LONGEST_DISPLAY_NAME {
            sim.enqueue_set_display_name_reply(&refusal(400, "Bad Request"));
        } else {
            let name = DisplayName {
                display_name: new.clone(),
                is_display_name_default: false,
                ..current.clone()
            };
            sim.set_display_name(name.clone());
            sim.enqueue_set_display_name_reply(&crate::SetDisplayNameReply {
                status: 200,
                reason: "OK".to_owned(),
                new_display_name: Some(new),
                error_tag: None,
            });
            let update = crate::DisplayNameUpdate {
                old_display_name: current.display_name,
                name,
            };
            sim.enqueue_display_name_update(&update);
            sim.push_side_upload(ServerEvent::DisplayNameChanged(Box::new(update)));
        }
        CapsResponse::llsd_xml(Llsd::Map(HashMap::new()).to_llsd_xml())
    }

    /// Serves one request of the Marketplace's `DirectDelivery` listing
    /// service the way Second Life's answers an avatar with no merchant store:
    /// `404` with a plain-text `Not found` on every route and method — the
    /// merchant probe (which the client reads as "not a merchant"), the
    /// listings, a listing, a new listing and a route the service does not
    /// have alike (`marketplace-direct-delivery`, 2026-10-19). A fake grid
    /// keeps no stores.
    fn dispatch_direct_delivery() -> CapsResponse {
        CapsResponse {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            content_range: None,
            headers: Vec::new(),
            body: b"Not found".to_vec(),
        }
    }

    /// Serves one `SearchStatRequest` POST of `{ classified_id }` with the
    /// six counters at nought, which is what Second Life answered for a
    /// classified nobody had looked at (`profiles`, 2026-10-15). A fake grid
    /// counts no clicks.
    fn dispatch_search_stat(request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(classified_id) = std::str::from_utf8(request.body)
            .ok()
            .and_then(crate::parse_search_stat_request_body)
        else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(crate::search_stat_answer_body(
            &crate::ClassifiedClickStats {
                classified_id,
                teleport: 0,
                map: 0,
                profile: 0,
                search: None,
            },
        ))
    }

    /// Serves one `ProductInfoRequest` GET: the land products, as the array of
    /// `{ sku, name, description }` maps Second Life answers.
    fn dispatch_product_info(&self, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        CapsResponse::llsd_xml(crate::land_products_body(&self.land_products))
    }

    /// Serves one `AvatarPickerSearch` GET: the residents whose username,
    /// display name or legacy name contains the `names` query parameter, capped
    /// at its `page_size`, as the reply's `agents` array. A query with no
    /// `names` at all is a `400`; one that matches nobody is an empty (but
    /// successful) `agents` array — a search answers with what it found.
    fn dispatch_avatar_picker_search(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let query = request.query.unwrap_or_default();
        let Some((names, page_size)) = parse_avatar_picker_search_query(&format!("?{query}"))
        else {
            return CapsResponse::bad_request();
        };
        let matches = sim.search_display_names(&names, page_size);
        CapsResponse::llsd_xml(build_avatar_picker_search_response(&matches))
    }

    /// Serves one `AgentPreferences` POST: merges the request's `Some` fields
    /// into the session's stored preferences and echoes the full stored set —
    /// so an empty-body POST is the pure "get". A malformed body answers
    /// `400`; `god_level` in a request is ignored (reply-only).
    fn dispatch_agent_preferences(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(update) = parse_agent_preferences(&body) else {
            return CapsResponse::bad_request();
        };
        sim.merge_agent_preferences(&update);
        CapsResponse::llsd_xml(build_agent_preferences_response(sim.agent_preferences()))
    }

    /// Serves one `InterestList` POST: switches the session's interest-list
    /// mode and answers with the mode now in force and the one it replaced,
    /// as Second Life does. A body that names no mode, or one the simulator
    /// does not know, asks for `default`.
    fn dispatch_interest_list(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(mode) = parse_interest_list_request(&body) else {
            return CapsResponse::bad_request();
        };
        let previous_mode = sim.set_interest_list_mode(mode);
        CapsResponse::llsd_xml(build_interest_list_reply(&InterestListReply {
            mode,
            previous_mode,
        }))
    }

    /// Reads one `RequestTaskInventory` GET and hands it to the driver. A
    /// request that names no prim is answered here, as one that names a prim
    /// the region does not have is: `404`, which is what Second Life answers
    /// both with.
    fn dispatch_task_inventory(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "GET" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        match request.query.and_then(parse_task_inventory_cap_query) {
            Some(query) => CapsDispatch::TaskInventory(query),
            None => CapsDispatch::Response(CapsResponse::not_found()),
        }
    }

    /// The answer to a [`CapsDispatch::TaskInventory`]: the prim's `contents`
    /// (its serial and items), or `None` for a prim the region does not have.
    ///
    /// A request naming the serial the prim is at is answered `304` with no
    /// body, a prim the region does not have `404`, and anything else the
    /// listing.
    #[must_use]
    pub fn task_inventory_response(
        query: TaskInventoryCapQuery,
        contents: Option<(i16, &[TaskInventoryItem])>,
    ) -> CapsResponse {
        match contents {
            None => CapsResponse::not_found(),
            Some((serial, _items)) if query.held_serial == Some(serial) => CapsResponse::empty(304),
            Some((serial, items)) => {
                CapsResponse::llsd_xml(build_task_inventory_cap_reply(serial, items))
            }
        }
    }

    /// Reads one `GetMetadata` POST and hands it to the driver.
    fn dispatch_script_metadata(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsDispatch::Response(CapsResponse::bad_request());
        };
        match parse_get_metadata_request(&body) {
            Ok((task, item)) => CapsDispatch::ScriptMetadata { task, item },
            Err(_) => CapsDispatch::Response(CapsResponse::bad_request()),
        }
    }

    /// The answer to a [`CapsDispatch::ScriptMetadata`]: the experience of a
    /// script the prim holds — `Some(None)` for a script under none — or
    /// `None`, and with it `404`, for a script the prim does not hold.
    #[must_use]
    pub fn script_metadata_response(script: Option<Option<ExperienceKey>>) -> CapsResponse {
        match script {
            Some(experience) => CapsResponse::llsd_xml(build_get_metadata_reply(experience)),
            None => CapsResponse::not_found(),
        }
    }

    /// Serves one `SendUserReport` POST: parses the abuse report and routes
    /// it to the driver as
    /// [`ServerEvent::AbuseReportReceived`](crate::ServerEvent::AbuseReportReceived)
    /// (the same event as the legacy UDP `UserReport` path). The reply body
    /// is undefined — the client discards it.
    fn dispatch_user_report(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(report) = parse_send_user_report(&body) else {
            return CapsResponse::bad_request();
        };
        sim.push_abuse_report(report);
        CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
    }

    /// Serves the two-step `SendUserReportWithScreenshot` uploader. The cap
    /// URL itself (step 1) takes the report POST, parks it, and answers
    /// `{ state: "upload", uploader }` with an uploader URL minted as the
    /// cap's own `screenshot` sub-path. That sub-path (step 2) takes the raw
    /// screenshot bytes (not LLSD — the body is stored verbatim), joins them
    /// with the parked report into
    /// [`ServerEvent::AbuseReportWithScreenshotReceived`](crate::ServerEvent::AbuseReportWithScreenshotReceived),
    /// and answers `{ state: "complete" }`. A step-2 POST with no parked
    /// report answers `400`; a re-POST of step 1 replaces the parked report;
    /// any other sub-path answers `404`.
    fn dispatch_user_report_screenshot(
        &self,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        match cap_sub_path(request.path) {
            None => {
                let Some(body) = parse_llsd_body(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok(report) = parse_send_user_report(&body) else {
                    return CapsResponse::bad_request();
                };
                sim.set_pending_screenshot_report(report);
                let uploader = self.screenshot_uploader_url();
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "upload".to_owned(),
                    uploader: Some(uploader.to_string()),
                    ..AssetUploadResponse::default()
                }))
            }
            Some(SCREENSHOT_SUB_PATH) => {
                let Some(report) = sim.take_pending_screenshot_report() else {
                    return CapsResponse::bad_request();
                };
                sim.push_abuse_report_with_screenshot(report, request.body.to_vec());
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "complete".to_owned(),
                    ..AssetUploadResponse::default()
                }))
            }
            Some(_) => CapsResponse::not_found(),
        }
    }

    /// The uploader URL step 1 of `SendUserReportWithScreenshot` answers
    /// with: the cap's own URL plus the `screenshot` sub-path (which
    /// [`SimCaps::resolve`] tolerates and
    /// [`SimCaps::dispatch_user_report_screenshot`] routes on).
    fn screenshot_uploader_url(&self) -> Url {
        let token = self
            .tokens
            .get(CAP_SEND_USER_REPORT_WITH_SCREENSHOT)
            .copied()
            .unwrap_or_default();
        let mut url = self.cap_url(token);
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.push(SCREENSHOT_SUB_PATH);
        }
        url
    }

    /// Serves the shared two-stage asset uploader for one of the
    /// [`UPLOAD_CAPABILITIES`]. Step 1 (a POST to the cap URL) parses the
    /// cap-specific metadata, parks it under `cap_name` and a fresh ticket,
    /// and answers `{ state: "upload", uploader }` with the cap's own
    /// `upload/<ticket>` sub-path — one uploader URL per upload, so uploads in
    /// flight together through one cap stay apart. Step 2 (a POST to that
    /// sub-path) takes the parked metadata, has the session mint the stored ids
    /// and push
    /// [`ServerEvent::CapsAssetUploaded`](crate::ServerEvent::CapsAssetUploaded),
    /// and answers `{ state: "complete", new_asset, new_inventory_item? }` —
    /// except a script upload, whose answer carries the compile result: that
    /// is handed to the driver as [`CapsDispatch::ScriptUpload`], since the
    /// compiler is the grid's. A bytes-POST to a ticket with nothing parked
    /// (never issued, or already completed) answers `400`; any other sub-path
    /// answers `404`. Wrong method → `405`, an unparsable metadata body →
    /// `400`.
    fn dispatch_caps_upload(
        &self,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
        cap_name: &'static str,
    ) -> CapsDispatch {
        if cap_name == CAP_NEW_FILE_AGENT_INVENTORY && self.new_file_policy != NewFilePolicy::Plain
        {
            return CapsDispatch::Response(self.dispatch_new_file(sim, request));
        }
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        match cap_sub_path(request.path) {
            None => {
                let Some(metadata) = Self::parse_upload_metadata(cap_name, request.body) else {
                    return CapsDispatch::Response(CapsResponse::bad_request());
                };
                let ticket = sim.park_caps_upload(cap_name, metadata);
                let uploader = self.upload_uploader_url(cap_name, ticket);
                CapsDispatch::Response(CapsResponse::llsd_xml(build_asset_upload_response(
                    &AssetUploadResponse {
                        state: "upload".to_owned(),
                        uploader: Some(uploader.to_string()),
                        ..AssetUploadResponse::default()
                    },
                )))
            }
            Some(sub_path) if upload_ticket(sub_path).is_some() => {
                let Some(metadata) = upload_ticket(sub_path)
                    .and_then(|ticket| sim.take_caps_upload(cap_name, ticket))
                else {
                    return CapsDispatch::Response(CapsResponse::bad_request());
                };
                Self::complete_caps_upload(sim, metadata, request.body)
            }
            Some(_) => CapsDispatch::Response(CapsResponse::not_found()),
        }
    }

    /// Step 2 of [`dispatch_caps_upload`](Self::dispatch_caps_upload): stores
    /// `body` under the parked `metadata` and answers — or, for a script,
    /// hands the answer to the driver.
    fn complete_caps_upload(
        sim: &mut SimSession,
        metadata: CapsUploadMetadata,
        body: &[u8],
    ) -> CapsDispatch {
        let script = match &metadata {
            CapsUploadMetadata::UpdateScriptTask(request) => {
                Some(UploadedScriptItem::Task(request.clone()))
            }
            CapsUploadMetadata::UpdateScriptAgent(request) => {
                Some(UploadedScriptItem::Agent(request.clone()))
            }
            _other => None,
        };
        // An upload that *creates* an item reports the permissions it was
        // granted, as both real grids do (`BunchOfCaps` copies its three
        // `m_*Mask` fields into every completion). This grid grants what was
        // asked for; the fields are what stop a client from having to assume
        // that, since a real grid need not.
        let granted = match &metadata {
            CapsUploadMetadata::NewFileInventory(new_file) => Some(UploadGrantedPermissions {
                next_owner: Permissions::from_bits(new_file.next_owner_mask),
                group: Permissions::from_bits(new_file.group_mask),
                everyone: Permissions::from_bits(new_file.everyone_mask),
            }),
            _other => None,
        };
        let mut data = body.to_vec();
        if matches!(
            &metadata,
            CapsUploadMetadata::UpdateAgentItem { cap, .. }
                if cap == crate::UpdatableAssetType::Notecard.cap()
        ) && sim.inventory_dialect() == crate::InventoryDialect::SecondLife
        {
            data = match crate::sim_notecard::second_life_body(sim, data) {
                Ok(kept) => kept,
                Err(refusal) => return CapsDispatch::Response(refusal),
            };
        }
        let (new_asset, new_inventory_item) = sim.complete_caps_upload(metadata, data);
        if let Some(item) = script {
            return CapsDispatch::ScriptUpload(UploadedScript {
                item,
                source: body.to_vec(),
                new_asset,
                new_inventory_item,
            });
        }
        CapsDispatch::Response(CapsResponse::llsd_xml(build_asset_upload_response(
            &AssetUploadResponse {
                state: "complete".to_owned(),
                new_asset: Some(new_asset.uuid()),
                new_inventory_item: new_inventory_item.map(|item| item.uuid()),
                granted,
                ..AssetUploadResponse::default()
            },
        )))
    }

    /// The answer to a [`CapsDispatch::ScriptUpload`]: the completion with
    /// the compile result the driver's compiler gave.
    #[must_use]
    pub fn script_upload_response(answer: &ScriptUploadAnswer) -> CapsResponse {
        CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
            state: "complete".to_owned(),
            new_asset: Some(answer.new_asset),
            new_inventory_item: answer.new_inventory_item.map(|item| item.uuid()),
            compiled: Some(answer.compiled),
            errors: answer.errors.clone(),
            inventory_flags: answer.inventory_flags,
            ..AssetUploadResponse::default()
        }))
    }

    /// Serves `MeshUploadFlag`.
    ///
    /// OpenSim answers a GET with `{ mesh_upload_status: "valid" }` and
    /// nothing else, and has no handler for another method. Second Life
    /// answers with the agent's name record and, on a GET, the account's
    /// status beside it; a POST is answered with the record alone.
    fn dispatch_mesh_upload_flag(
        &self,
        sim: &SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        let status = (
            "mesh_upload_status".to_owned(),
            Llsd::String("valid".to_owned()),
        );
        if self.new_file_policy == NewFilePolicy::OpenSim {
            return if request.method == "GET" {
                CapsResponse::llsd_xml(Llsd::Map(HashMap::from([status])).to_llsd_xml())
            } else {
                CapsResponse::not_found()
            };
        }
        let mut body = match sim
            .agent_id()
            .and_then(|agent| sim.display_name(agent))
            .map(sl_wire::DisplayName::to_llsd)
        {
            Some(Llsd::Map(record)) => record,
            _other => HashMap::new(),
        };
        if request.method == "GET" {
            let _previous = body.insert(status.0, status.1);
        }
        CapsResponse::llsd_xml(Llsd::Map(body).to_llsd_xml())
    }

    /// Serves the two-step uploads that are not asset uploads —
    /// `InventoryThumbnailUpload` and `SendPostcard`, both Second Life's
    /// alone — as it was measured to: any first request is handed an
    /// uploader, whatever it names; the uploader serves once; and a request
    /// that is no POST is answered `501`.
    fn dispatch_side_upload(
        &self,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
        cap: &'static str,
    ) -> CapsResponse {
        let Some(sub_path) = cap_sub_path(request.path) else {
            if request.method != "POST" {
                return sim_new_file::not_implemented();
            }
            let body = std::str::from_utf8(request.body).unwrap_or_default();
            let parked = if cap == CAP_SEND_POSTCARD {
                SideUpload::Postcard(Box::new(
                    crate::parse_postcard_request(body).unwrap_or_default(),
                ))
            } else if cap == CAP_UPLOAD_AGENT_PROFILE_IMAGE {
                SideUpload::ProfileImage(crate::parse_profile_image_upload_body(body))
            } else {
                SideUpload::Thumbnail(crate::parse_thumbnail_upload_request(body).ok().flatten())
            };
            let ticket = sim.park_side_upload(parked);
            return CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                state: "upload".to_owned(),
                uploader: Some(self.upload_uploader_url(cap, ticket).to_string()),
                ..AssetUploadResponse::default()
            }));
        };
        let Some(ticket) = upload_ticket(sub_path) else {
            return CapsResponse::not_found();
        };
        let parked = sim.take_side_upload(ticket);
        if request.method != "POST" {
            return sim_new_file::not_implemented();
        }
        match parked {
            None => sim_new_file::spent_uploader(ticket),
            Some(SideUpload::Postcard(postcard)) => {
                sim.push_side_upload(ServerEvent::PostcardUploaded {
                    postcard,
                    image: request.body.to_vec(),
                });
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "complete".to_owned(),
                    ..AssetUploadResponse::default()
                }))
            }
            Some(SideUpload::ProfileImage(image)) => {
                // An upload that named no profile field has nowhere to go.
                let Some(image) = image else {
                    return sim_new_file::server_error();
                };
                let new_asset = sim.mint_id();
                sim.push_side_upload(ServerEvent::ProfileImageUploaded {
                    image,
                    new_asset: crate::AssetKey::from(new_asset),
                    data: request.body.to_vec(),
                });
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "complete".to_owned(),
                    new_asset: Some(new_asset),
                    ..AssetUploadResponse::default()
                }))
            }
            Some(SideUpload::Thumbnail(target)) => {
                if let Some(refusal) = sim_new_file::thumbnail_refusal(request.body) {
                    return sim_new_file::side_upload_failure(refusal);
                }
                // An image for nothing, or for an item the agent does not
                // hold, is a server error with nothing in it. What a prim
                // holds is not looked at: an item in a prim nobody has is
                // answered like any other.
                let held = match target {
                    None => false,
                    Some(crate::ThumbnailTarget::Item(item)) => {
                        sim.agent_inventory().item(item).is_some()
                    }
                    Some(
                        crate::ThumbnailTarget::Folder(_) | crate::ThumbnailTarget::TaskItem { .. },
                    ) => true,
                };
                let Some(target) = target.filter(|_target| held) else {
                    return sim_new_file::server_error();
                };
                let new_asset = sim.mint_id();
                sim.push_side_upload(ServerEvent::InventoryThumbnailUploaded {
                    target,
                    new_asset: crate::AssetKey::from(new_asset),
                    data: request.body.to_vec(),
                });
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "complete".to_owned(),
                    new_asset: Some(new_asset),
                    ..AssetUploadResponse::default()
                }))
            }
        }
    }

    /// Serves `NewFileAgentInventory` as the grid
    /// [`with_new_file_policy`](Self::with_new_file_policy) named does: its
    /// classes, its prices, its refusals and its words. See
    /// [`NewFilePolicy`].
    fn dispatch_new_file(&self, sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        let policy = self.new_file_policy;
        let cap = CAP_NEW_FILE_AGENT_INVENTORY;
        let Some(sub_path) = cap_sub_path(request.path) else {
            let step =
                sim_new_file::first_step(policy, request.method, request.body, sim.money_balance());
            // OpenSim's uploader, once stuck, says so to every request it
            // reads at all.
            if policy == NewFilePolicy::OpenSim
                && sim.uploader_stuck()
                && !matches!(&step, FirstStep::Refused(refused) if refused.status != 200)
            {
                return sim_new_file::open_sim_refusal(sim_new_file::OPENSIM_BUSY);
            }
            return match step {
                FirstStep::Refused(refused) => refused,
                FirstStep::Taken {
                    request,
                    expected,
                    upload_price,
                } => {
                    let ticket =
                        sim.park_caps_upload(cap, CapsUploadMetadata::NewFileInventory(request));
                    sim.note_upload_expectation(ticket, expected, upload_price);
                    sim_new_file::uploader_answer(
                        policy,
                        self.upload_uploader_url(cap, ticket).as_str(),
                        upload_price,
                        sim.money_balance(),
                    )
                }
            };
        };
        let Some(ticket) = upload_ticket(sub_path) else {
            return CapsResponse::not_found();
        };
        let second_life = matches!(policy, NewFilePolicy::SecondLife(_));
        if request.method != "POST" {
            // Second Life's uploader is spent by any request; OpenSim has no
            // handler for another method and keeps the uploader.
            if second_life {
                let _spent = sim.take_caps_upload(cap, ticket);
                let _spent = sim.take_upload_expectation(ticket);
                return sim_new_file::not_implemented();
            }
            return CapsResponse::html(404, sim_new_file::OPENSIM_NOT_FOUND);
        }
        let parked = sim.take_caps_upload(cap, ticket);
        let (expected, _quoted) = sim
            .take_upload_expectation(ticket)
            .unwrap_or((sim_new_file::Expected::Unstated, 0));
        let Some(CapsUploadMetadata::NewFileInventory(new_file)) = parked else {
            // An uploader serves once.
            return if second_life {
                CapsResponse::words(
                    404,
                    &format!("cap not found: '{}'", Uuid::from_u128(ticket)),
                )
            } else {
                CapsResponse::status(200)
            };
        };
        match policy {
            NewFilePolicy::SecondLife(prices) => {
                let upload_price = match sim_new_file::second_life_bytes(
                    prices,
                    &new_file,
                    expected,
                    request.body,
                    sim.money_balance(),
                ) {
                    Ok(upload_price) => upload_price,
                    Err(refused) => return refused,
                };
                sim.charge(upload_price);
                let transaction = sim.mint_id();
                let (new_asset, new_item) = sim.complete_caps_upload(
                    CapsUploadMetadata::NewFileInventory(new_file.clone()),
                    request.body.to_vec(),
                );
                sim_new_file::second_life_completion(
                    &new_file,
                    new_asset.uuid(),
                    new_item.map_or_else(Uuid::nil, |item| item.uuid()),
                    transaction,
                    upload_price,
                )
            }
            NewFilePolicy::OpenSim | NewFilePolicy::Plain => {
                match sim_new_file::open_sim_filed_as(&new_file) {
                    Some(filed) => {
                        let (new_asset, new_item) = sim.complete_caps_upload(
                            CapsUploadMetadata::NewFileInventory(filed),
                            request.body.to_vec(),
                        );
                        sim_new_file::open_sim_completion(
                            &new_file,
                            new_asset.uuid(),
                            new_item.map_or_else(Uuid::nil, |item| item.uuid()),
                        )
                    }
                    None => {
                        // Nothing is stored and nothing filed, the answer
                        // names both anyway, and the uploader is stuck.
                        sim.stick_uploader();
                        let new_asset = sim.mint_id();
                        let new_item = sim.mint_id();
                        sim_new_file::open_sim_completion(&new_file, new_asset, new_item)
                    }
                }
            }
        }
    }

    /// Parses the step-1 metadata of a two-stage upload for `cap_name` into the
    /// parked [`CapsUploadMetadata`], or `None` (→ `400`) when the body is not
    /// UTF-8 or not well-formed for the cap. The four agent-inventory `Update*`
    /// caps (gesture / notecard / settings / material) share the bare
    /// `{ item_id }` body and fall through to the catch-all arm; the two
    /// task-inventory ones (notecard / settings / gesture / material) share the
    /// `{ task_id, item_id }` body of the arm above it.
    fn parse_upload_metadata(cap_name: &str, body: &[u8]) -> Option<CapsUploadMetadata> {
        let text = std::str::from_utf8(body).ok()?;
        let metadata = match cap_name {
            CAP_NEW_FILE_AGENT_INVENTORY => CapsUploadMetadata::NewFileInventory(
                parse_new_file_agent_inventory_request(text).ok()?,
            ),
            CAP_UPLOAD_BAKED_TEXTURE => CapsUploadMetadata::BakedTexture,
            CAP_UPDATE_SCRIPT_AGENT => {
                CapsUploadMetadata::UpdateScriptAgent(parse_update_script_agent_request(text).ok()?)
            }
            CAP_UPDATE_SCRIPT_TASK => {
                CapsUploadMetadata::UpdateScriptTask(parse_update_script_task_request(text).ok()?)
            }
            CAP_UPDATE_NOTECARD_TASK_INVENTORY
            | CAP_UPDATE_SETTINGS_TASK_INVENTORY
            | CAP_UPDATE_GESTURE_TASK_INVENTORY
            | CAP_UPDATE_MATERIAL_TASK_INVENTORY => {
                let request = parse_update_task_item_asset_request(text).ok()?;
                CapsUploadMetadata::UpdateTaskItem {
                    cap: cap_name.to_owned(),
                    task_id: request.task_id,
                    item_id: request.item_id,
                }
            }
            _ => CapsUploadMetadata::UpdateAgentItem {
                cap: cap_name.to_owned(),
                item_id: parse_update_item_asset_request(text).ok()?,
            },
        };
        Some(metadata)
    }

    /// The uploader URL a two-stage upload's step 1 answers with: the cap's own
    /// URL plus [`UPLOAD_SUB_PATH`] and the upload's ticket (routed back to
    /// [`SimCaps::dispatch_caps_upload`]).
    fn upload_uploader_url(&self, cap_name: &str, ticket: u128) -> Url {
        let token = self.tokens.get(cap_name).copied().unwrap_or_default();
        let mut url = self.cap_url(token);
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.push(UPLOAD_SUB_PATH);
            segments.push(&format!("{ticket:x}"));
        }
        url
    }

    /// Serves one `UpdateAvatarAppearance` POST as Second Life does
    /// (`appearance-baking`, 2026-10-19, `book/src/gridspec/appearance.md`
    /// § Baking): a request for any version but the Current Outfit Folder's
    /// is refused `400` with `{ success: false, code: 1, error: "Cof Version
    /// Mismatch: <asked> != <current>", expected: <current>, observed:
    /// <asked>, cof_version: <asked>, agent_id }`; one for the folder's
    /// version is accepted `200` with `{ success: true, error: undef,
    /// cof_version, agent_id, warnings: [] }` and surfaced as
    /// [`ServerEvent::ServerAppearanceRequested`](crate::ServerEvent::ServerAppearanceRequested)
    /// — the baked result then goes out over UDP `AvatarAppearance`. An
    /// account with no Current Outfit Folder has no version to hold a request
    /// to, and every request is accepted. Where the server has baked the
    /// outfit ([`SimSession::set_baked_outfit`]) both answers carry the
    /// bake's payload as Second Life's do — the textures, the visual
    /// parameters and the scale, and on a refusal a dump of the outfit.
    /// Wrong method → `405`, a non-LLSD body → `400`.
    fn dispatch_update_avatar_appearance(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(text) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(cof_version) = parse_update_avatar_appearance_request(text) else {
            return CapsResponse::bad_request();
        };
        let agent = sim.agent_id().map_or_else(Uuid::nil, |agent| agent.uuid());
        let current = sim.agent_inventory().current_outfit_version();
        if let Some(current) = current.filter(|current| *current != cof_version) {
            let mut entries = vec![
                ("success", Llsd::Boolean(false)),
                ("code", Llsd::Integer(1)),
                (
                    "error",
                    Llsd::String(format!("Cof Version Mismatch: {cof_version} != {current}")),
                ),
                ("expected", Llsd::Integer(current)),
                ("observed", Llsd::Integer(cof_version)),
                ("cof_version", Llsd::Integer(cof_version)),
                ("agent_id", Llsd::Uuid(agent)),
            ];
            if let Some(outfit) = sim.baked_outfit() {
                entries.extend(outfit.refused_payload());
            }
            return CapsResponse::llsd_xml_status(400, llsd_map(entries).to_llsd_xml());
        }
        sim.push_content_event(ServerEvent::ServerAppearanceRequested { cof_version });
        let mut entries = vec![
            ("success", Llsd::Boolean(true)),
            ("error", Llsd::Undef),
            ("cof_version", Llsd::Integer(cof_version)),
            ("agent_id", Llsd::Uuid(agent)),
            ("warnings", Llsd::Array(Vec::new())),
        ];
        if let Some(outfit) = sim.baked_outfit() {
            entries.extend(outfit.accepted_payload());
        }
        CapsResponse::llsd_xml(llsd_map(entries).to_llsd_xml())
    }

    /// Serves one `IncrementCOFVersion` GET: bumps the version of the agent's
    /// Current Outfit Folder in the served inventory tree and answers the new
    /// version, `{ version }`, surfacing it as
    /// [`ServerEvent::CurrentOutfitVersionIncremented`]. Wrong method → `405`; an account with no Current
    /// Outfit Folder has no version to bump → `404`.
    fn dispatch_increment_cof_version(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        match sim.agent_inventory_mut().increment_current_outfit_version() {
            Some(version) => {
                sim.push_content_event(ServerEvent::CurrentOutfitVersionIncremented { version });
                CapsResponse::llsd_xml(cof_version_increment_to_llsd(version).to_llsd_xml())
            }
            None => CapsResponse::not_found(),
        }
    }

    /// Reads one `CopyInventoryFromNotecard` POST into a
    /// [`CapsDispatch::CopyFromNotecard`]: the notecard, the item embedded in
    /// it and where it is to go are the driver's to resolve, and how the grid
    /// answers — and what it files — is the imitated grid's (Second Life
    /// answers `200` and an undefined body whether or not it copied anything,
    /// OpenSim `200` and no body, or `400` and an alert where it found no
    /// item). Wrong method → `405`, a non-LLSD body → `400`.
    fn dispatch_copy_inventory_from_notecard(request: &CapsRequest<'_>) -> CapsDispatch {
        if request.method != "POST" {
            return CapsDispatch::Response(CapsResponse::method_not_allowed());
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsDispatch::Response(CapsResponse::bad_request());
        };
        let copy = parse_copy_inventory_from_notecard(&body);
        CapsDispatch::CopyFromNotecard {
            notecard_id: copy.notecard,
            object_id: copy.object,
            item_id: copy.item,
            folder_id: copy.folder,
        }
    }

    /// The answer to a [`CapsDispatch::CopyFromNotecard`]: an undefined body
    /// (Second Life's, copied or not), no body (OpenSim's copy), or OpenSim's
    /// refusal, `400` and no body.
    #[must_use]
    pub fn copy_from_notecard_response(answer: CopyFromNotecardAnswer) -> CapsResponse {
        match answer {
            CopyFromNotecardAnswer::Undefined => CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned()),
            CopyFromNotecardAnswer::Empty => CapsResponse::status(200),
            CopyFromNotecardAnswer::Refused => CapsResponse::status(400),
        }
    }

    /// Serves the legacy `RenderMaterials` surface, routed on HTTP method:
    /// `POST` (or `GET`) queries the session's material store — `POST` for the
    /// zipped id list, `GET` for every region material — and answers the
    /// matching materials; `PUT` sets legacy materials on object faces,
    /// surfacing them as
    /// [`ServerEvent::RenderMaterialsSet`](crate::ServerEvent::RenderMaterialsSet)
    /// and acking with an undefined body. Any other method → `405`.
    fn dispatch_render_materials(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        match request.method {
            "POST" => {
                let Ok(text) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok(ids) = parse_render_materials_request(text) else {
                    return CapsResponse::bad_request();
                };
                CapsResponse::llsd_xml(build_render_materials_response(&sim.region_materials(&ids)))
            }
            "GET" => {
                CapsResponse::llsd_xml(build_render_materials_response(&sim.region_materials(&[])))
            }
            "PUT" => {
                let Ok(text) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok(updates) = parse_render_materials_put_request(text) else {
                    return CapsResponse::bad_request();
                };
                sim.push_content_event(ServerEvent::RenderMaterialsSet { updates });
                CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
            }
            _ => CapsResponse::method_not_allowed(),
        }
    }

    /// Serves one `ModifyMaterialParams` POST: parses the per-face GLTF material
    /// assignments, surfaces them as
    /// [`ServerEvent::MaterialParamsModified`](crate::ServerEvent::MaterialParamsModified),
    /// and answers the `{ success: true, message: "" }` status. Wrong method →
    /// `405`, a non-LLSD body → `400`.
    fn dispatch_modify_material_params(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(text) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(updates) = parse_modify_material_params_request(text) else {
            return CapsResponse::bad_request();
        };
        sim.push_content_event(ServerEvent::MaterialParamsModified { updates });
        CapsResponse::llsd_xml(build_modify_material_params_response(true, ""))
    }

    /// Serves one `ObjectMedia` POST, routed on the body's `verb`. A `GET`
    /// answers the object's stored per-face media (an unknown object gets an
    /// empty media list, tolerant like the other read caps); an `UPDATE`
    /// records the new media (advancing the media version), surfaces it as
    /// [`ServerEvent::ObjectMediaSet`](crate::ServerEvent::ObjectMediaSet), and
    /// acks with an undefined body. Wrong method → `405`; a non-LLSD or
    /// unroutable body → `400`.
    fn dispatch_object_media(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(media_request) = parse_object_media_request(&body) else {
            return CapsResponse::bad_request();
        };
        match media_request {
            ObjectMediaRequest::Get { object_id } => {
                let response = sim.object_media(object_id).map_or_else(
                    || ObjectMediaResponse {
                        object_id,
                        version: String::new(),
                        faces: Vec::new(),
                    },
                    |state| ObjectMediaResponse {
                        object_id,
                        version: state.version.clone(),
                        faces: state.faces.clone(),
                    },
                );
                CapsResponse::llsd_xml(response.to_llsd())
            }
            ObjectMediaRequest::Update { object_id, faces } => {
                sim.set_object_media_update(object_id, faces);
                CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
            }
        }
    }

    /// Serves one `ObjectMediaNavigate` POST: advances the object's media
    /// version and surfaces the navigation as
    /// [`ServerEvent::ObjectMediaNavigated`](crate::ServerEvent::ObjectMediaNavigated),
    /// acking with an undefined body (the cap carries no media reply). Wrong
    /// method → `405`; a non-LLSD or unroutable body → `400`.
    fn dispatch_object_media_navigate(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(navigate) = parse_object_media_navigate_request(&body) else {
            return CapsResponse::bad_request();
        };
        sim.navigate_object_media(navigate.object_id, navigate.face, navigate.url);
        CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
    }

    /// Serves one `FetchInventoryDescendents2` / `FetchLibDescendents2` POST:
    /// parses the `folders` request array and answers one folder entry per
    /// known folder from the matching serving tree (`FetchLibDescendents2`
    /// reads the Library tree, everything else the agent tree). Unknown
    /// folders are skipped tolerantly, matching OpenSim's handler. Wrong
    /// method → `405`; a malformed body → `400`.
    fn dispatch_fetch_descendents(
        sim: &SimSession,
        request: &CapsRequest<'_>,
        name: &str,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(text) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(folders) = parse_fetch_inventory_request(text) else {
            return CapsResponse::bad_request();
        };
        let library = name == CAP_FETCH_LIBRARY;
        let agent = sim.agent_id().map_or_else(Uuid::nil, |agent| agent.uuid());
        let dialect = sim.inventory_dialect();
        // Each answered folder with the owner its entry names
        // (`book/src/gridspec/inventory.md` § Fetching). Through the agent's
        // own capability a folder the agent does not hold is still answered,
        // on both grids, with an empty entry the agent owns at version 0 —
        // except that OpenSim lists a Library folder there.
        let (events, owners): (Vec<Event>, Vec<Uuid>) = folders
            .iter()
            .filter_map(|folder| {
                let read = |tree: &crate::sim_inventory::SimInventoryTree| {
                    tree.descendents(
                        folder.folder_id,
                        folder.fetch_folders,
                        folder.fetch_items,
                        folder.sort_order,
                    )
                };
                if library {
                    return read(sim.library_inventory()).map(|event| (event, folder.owner_id));
                }
                if let Some(mut event) = read(sim.agent_inventory()) {
                    // Each item link's target, ahead of the folder's own items
                    // and outside its count — OpenSim's `ProcessLinks`, and what
                    // Second Life's listing of a folder of links names too
                    // (`book/src/gridspec/appearance.md`, `inventory.md`
                    // § Changing inventory).
                    if let Event::InventoryDescendents { items, .. } = &mut event {
                        let mut listed = sim.agent_inventory().link_targets(items);
                        listed.append(items);
                        *items = listed;
                    }
                    return Some((event, folder.owner_id));
                }
                if dialect == crate::InventoryDialect::OpenSim
                    && let Some(event) = read(sim.library_inventory())
                {
                    return Some((event, folder.owner_id));
                }
                Some((
                    Event::InventoryDescendents {
                        folder_id: folder.folder_id,
                        version: 0,
                        descendents: 0,
                        folders: Vec::new(),
                        items: Vec::new(),
                    },
                    agent,
                ))
            })
            .unzip();
        let Ok(mut body) = inventory_descendents_to_llsd(&events) else {
            return CapsResponse::internal_error();
        };
        if let Llsd::Map(map) = &mut body
            && let Some(Llsd::Array(entries)) = map.get_mut("folders")
        {
            for (entry, owner) in entries.iter_mut().zip(owners) {
                dialect.descendents_folder(entry, agent, owner);
            }
        }
        CapsResponse::llsd_xml(body.to_llsd_xml())
    }

    /// Serves one `FetchInventory2` / `FetchLib2` per-item fetch POST:
    /// looks each requested `item_id` up in the matching serving tree
    /// (`FetchLib2` reads the Library tree) and answers the found items;
    /// unknown ids are omitted, exactly as OpenSim's handler tolerates them
    /// (and the reply never carries an `error` member). Wrong method →
    /// `405`; a malformed body → `400`.
    fn dispatch_fetch_items(
        sim: &SimSession,
        request: &CapsRequest<'_>,
        name: &str,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(text) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(fetch) = parse_fetch_inventory_items_request(text) else {
            return CapsResponse::bad_request();
        };
        let tree = if name == CAP_FETCH_LIBRARY_ITEM {
            sim.library_inventory()
        } else {
            sim.agent_inventory()
        };
        let items: Vec<InventoryItem> = fetch
            .items
            .iter()
            .filter_map(|reference| tree.item(reference.item_id).cloned())
            .collect();
        let agent_id = sim.agent_id().map_or(fetch.agent_id, |agent| agent.uuid());
        let Ok(mut body) = fetch_inventory_items_to_llsd(agent_id, &items) else {
            return CapsResponse::internal_error();
        };
        let dialect = sim.inventory_dialect();
        if let Llsd::Map(map) = &mut body
            && let Some(Llsd::Array(entries)) = map.get_mut("items")
        {
            for entry in entries {
                dialect.fetched_item(entry, agent_id);
            }
        }
        CapsResponse::llsd_xml(body.to_llsd_xml())
    }

    /// Serves one plain `CreateInventoryCategory` POST (client-chosen folder
    /// id): applies the folder to the agent tree
    /// ([`SimSession::create_inventory_category`]) and echoes the request
    /// fields, the capability's synchronous reply shape. Wrong method →
    /// `405`; a malformed body → `400`; an unknown parent → `404`.
    fn dispatch_create_inventory_category(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(text) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(create) = parse_create_inventory_category_request(text) else {
            return CapsResponse::bad_request();
        };
        match sim.create_inventory_category(&create) {
            Ok(_update) => CapsResponse::llsd_xml(build_create_inventory_category_response(
                create.folder_id,
                create.parent_id,
                create.folder_type,
                &create.name,
            )),
            Err(error) => Self::inventory_error(error),
        }
    }

    /// Serves one `InventoryAPIv3` / `LibraryAPIv3` REST request, routing on
    /// HTTP verb × URL sub-path exactly as the client lays its URLs out
    /// (`llaisapi.cpp`): `POST /category/<parent>?tid=` creates a folder — or
    /// links, when the body carries a `links` array; `PATCH /category/<id>`
    /// renames (`{ name }`) or moves (`{ parent_id }`); `DELETE
    /// /category/<id>` removes a subtree and `DELETE /category/<id>/children`
    /// empties a folder; `GET /category/<id>/children?depth=` lists a
    /// subtree; `GET`/`PATCH`/`DELETE /item/<id>` fetch / update-or-move /
    /// remove an item. `LibraryAPIv3` is read-only: its `GET`s serve the
    /// Library tree and every mutating verb answers `405`.
    ///
    /// Status contract: unknown verb (or a mutation on the Library) → `405`;
    /// a malformed body or unroutable sub-path → `400`; an unknown target id
    /// → `404` ([`Self::inventory_error`] — the AIS REST convention, a
    /// deliberate exception to the tolerant-empty stance the batch fetch caps
    /// keep); a mutation the tree rejects (unknown / cycle-creating new
    /// parent) → `400`. Successful mutations answer the change-set meta with
    /// the affected objects under `_embedded`; deletes answer meta only.
    fn dispatch_ais3(sim: &mut SimSession, request: &CapsRequest<'_>, name: &str) -> CapsResponse {
        let suffix = ais_suffix(request);
        let read_only = name == CAP_LIBRARY_API_V3;
        let agent = sim.agent_id().map_or_else(Uuid::nil, |agent| agent.uuid());
        match request.method {
            "GET" => {
                let tree = if read_only {
                    sim.library_inventory()
                } else {
                    sim.agent_inventory()
                };
                // A children URL missing the `?depth=` query lists the folder
                // itself, which is also what `depth=0` asks for: the parameter
                // counts levels of recursion *below* the listing, not levels
                // of it.
                if let Some((folder_id, depth)) = parse_ais_category_children_fetch_url(&suffix)
                    .or_else(|| parse_ais_category_children_url(&suffix).map(|id| (id, 0)))
                {
                    // `&children=` narrows it to a subset fetch of named
                    // children; without it the folder is listed entire.
                    let subset = parse_ais_category_children_subset(&suffix);
                    let Some(listing) = tree.listing_to_depth(folder_id, depth, subset.as_deref())
                    else {
                        return category_not_found(folder_id);
                    };
                    return match ais_category_children_reply_to_llsd(&listing) {
                        Ok(mut body) => {
                            decorate_ais(&mut body, name, agent, AisTop::Children);
                            CapsResponse::llsd_xml(body.to_llsd_xml())
                        }
                        Err(_) => CapsResponse::internal_error(),
                    };
                }
                // `/links` answers a folder's link items only, at any depth the
                // body asks for: the depth there governs how far *embedded*
                // outfit folders expand, not whether the folder's own links
                // come back. `current` is the server-resolved alias for the
                // Current Outfit folder — the client cannot name its id.
                let links_folder = if is_ais_current_outfit_links_url(&suffix) {
                    match tree.folder_of_type(crate::FolderType::CurrentOutfit.to_code()) {
                        Some(folder) => Some(folder.folder_id),
                        // No Current Outfit folder means nothing is worn, not a
                        // malformed request.
                        None => return CapsResponse::not_found(),
                    }
                } else {
                    parse_ais_category_links_url(&suffix)
                };
                if let Some(folder_id) = links_folder {
                    let Some(folder) = tree.folder(folder_id).cloned() else {
                        return CapsResponse::not_found();
                    };
                    let Some(links) = tree.child_links(folder_id) else {
                        return CapsResponse::not_found();
                    };
                    return match ais_category_links_reply_to_llsd(&folder, &links) {
                        Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
                        Err(_) => CapsResponse::internal_error(),
                    };
                }
                // The folder alone, as Second Life answers `GET /category/<id>`.
                if let Some(folder_id) = parse_ais_category_url(&suffix) {
                    let Some(folder) = tree.folder(folder_id).cloned() else {
                        return category_not_found(folder_id);
                    };
                    return match ais_category_children_reply_to_llsd(&InventoryListing::unlisted(
                        folder,
                    )) {
                        Ok(mut body) => {
                            decorate_ais(&mut body, name, agent, AisTop::Folder);
                            CapsResponse::llsd_xml(body.to_llsd_xml())
                        }
                        Err(_) => CapsResponse::internal_error(),
                    };
                }
                if let Some(item_id) = parse_ais_item_url(&suffix) {
                    let Some(item) = tree.item(item_id) else {
                        return CapsResponse::not_found();
                    };
                    return match ais_item_reply_to_llsd(item) {
                        Ok(mut body) => {
                            decorate_ais(&mut body, name, agent, AisTop::Item);
                            CapsResponse::llsd_xml(body.to_llsd_xml())
                        }
                        Err(_) => CapsResponse::internal_error(),
                    };
                }
                if is_ais_orphans_url(&suffix) {
                    // The serving tree is built parent-first and its moves
                    // reject unknown parents, so it can hold no orphan. The
                    // viewer asks on every login regardless and reads any
                    // non-2xx as an inventory error, so answer the empty set.
                    return match ais_inventory_update_to_llsd(&[], &[]) {
                        Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
                        Err(_) => CapsResponse::internal_error(),
                    };
                }
                CapsResponse::bad_request()
            }
            "POST" if !read_only => {
                let Some((parent, _tid)) = parse_ais_create_category_url(&suffix) else {
                    return CapsResponse::bad_request();
                };
                let Ok(text) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Some(body) = parse_llsd_body(request.body) else {
                    return CapsResponse::bad_request();
                };
                if body.get("links").is_some() {
                    let Ok(links) = parse_ais_create_link_body(text) else {
                        return CapsResponse::bad_request();
                    };
                    return match sim.ais_create_links(parent, &links) {
                        Ok((update, items)) => {
                            let embedded = match ais_embedded(&[], &items, &["links"]) {
                                Ok(embedded) => embedded,
                                Err(response) => return response,
                            };
                            ais_answer(
                                AisAnswer::new(201, name, agent).folder(sim, parent),
                                vec![
                                    (
                                        "_created_items",
                                        ais_ids(update.created_items.iter().map(|id| id.uuid())),
                                    ),
                                    ("_embedded", embedded),
                                    ("_updated_category_versions", ais_versions(&update)),
                                ],
                            )
                        }
                        Err(error) => Self::inventory_error(error),
                    };
                }
                let Ok(create) = parse_ais_create_category_body(text) else {
                    return CapsResponse::bad_request();
                };
                match sim.ais_create_category(parent, &create) {
                    // A folder the grid has just made holds nothing, and
                    // saying so is what lets a viewer record its version
                    // instead of holding it unknown until something fetches it.
                    Ok((update, folder)) => {
                        let embedded = match ais_embedded(
                            &[InventoryListing::empty(folder)],
                            &[],
                            &["categories", "items", "links"],
                        ) {
                            Ok(embedded) => embedded,
                            Err(response) => return response,
                        };
                        ais_answer(
                            AisAnswer::new(201, name, agent).folder(sim, parent),
                            vec![
                                (
                                    "_created_categories",
                                    ais_ids(update.created_categories.iter().map(|id| id.uuid())),
                                ),
                                (
                                    "_created_items",
                                    ais_ids(update.created_items.iter().map(|id| id.uuid())),
                                ),
                                ("_embedded", embedded),
                                ("_updated_category_versions", ais_versions(&update)),
                            ],
                        )
                    }
                    Err(error) => Self::inventory_error(error),
                }
            }
            // A slam: the folder's links replaced by the ones the bare array
            // names (the reference's `SlamFolder`).
            "PUT" if !read_only => {
                let Some(folder_id) = parse_ais_category_links_url(&suffix) else {
                    return CapsResponse::method_not_allowed();
                };
                let Ok(text) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let links = match parse_ais_slam_body(text) {
                    Ok(Some(links)) => links,
                    // A `{ links: [...] }` map, as measured on Second Life.
                    Ok(None) => {
                        return ais_error(
                            400,
                            1,
                            "List of links required.",
                            ("category.py", "Category.put", 686),
                            Vec::new(),
                        );
                    }
                    Err(_) => return CapsResponse::bad_request(),
                };
                // Two links to one target, or an entry whose type is neither
                // an item nor a folder link, refuse the whole slam with the
                // folder untouched (`inventory-mutations`' refused slams).
                let mut targets = HashSet::new();
                if links.iter().any(|link| {
                    !matches!(
                        link.link_type,
                        crate::ASSET_CODE_LINK | crate::ASSET_CODE_LINK_FOLDER
                    ) || !targets.insert(link.linked_id)
                }) {
                    return ais_error(
                        400,
                        0,
                        "Duplicate links or invalid link type.",
                        // Second Life's line is a four-digit one the census
                        // records only as `<n>`.
                        ("categories.py", "CategoryAPI._get_links", 1000),
                        Vec::new(),
                    );
                }
                match sim.ais_slam_links(folder_id, &links) {
                    Ok((update, updated, result)) => {
                        let embedded = match ais_embedded(&[], &result, &["links"]) {
                            Ok(embedded) => embedded,
                            Err(response) => return response,
                        };
                        let linked = Llsd::Map(
                            result
                                .iter()
                                .map(|link| {
                                    (link.asset_id.to_string(), Llsd::Uuid(link.item_id.uuid()))
                                })
                                .collect(),
                        );
                        ais_answer(
                            AisAnswer::new(201, name, agent).folder(sim, folder_id),
                            vec![
                                (
                                    "_category_items_removed",
                                    ais_ids(
                                        update.category_items_removed.iter().map(|id| id.uuid()),
                                    ),
                                ),
                                (
                                    "_created_items",
                                    ais_ids(update.created_items.iter().map(|id| id.uuid())),
                                ),
                                ("_embedded", embedded),
                                ("_linked_ids", linked),
                                ("_updated_category_versions", ais_versions(&update)),
                                (
                                    "_updated_items",
                                    ais_ids(updated.iter().map(|id| id.uuid())),
                                ),
                            ],
                        )
                    }
                    Err(error) => Self::inventory_error(error),
                }
            }
            "PATCH" if !read_only => {
                let Ok(text) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Some(body) = parse_llsd_body(request.body) else {
                    return CapsResponse::bad_request();
                };
                // Second Life's AIS3 moves nothing: a `parent_id` in a
                // `PATCH` is refused, folder or item, and the object stays
                // (`book/src/gridspec/inventory.md` § Fetching).
                let new_parent = body
                    .get("parent_id")
                    .is_some()
                    .then(|| parse_ais_move_body(text));
                // A `parent_id` is refused only where it names another
                // parent: the reference's updates carry the unchanged one
                // (`LLInventoryItem::asLLSD`).
                if let Some(folder_id) = parse_ais_category_url(&suffix) {
                    let current = sim
                        .agent_inventory()
                        .folder(folder_id)
                        .and_then(|folder| folder.parent_id);
                    if let Some(parent) = new_parent
                        && parent.as_ref().ok() != current.as_ref()
                    {
                        let Ok(parent) = parent else {
                            return CapsResponse::bad_request();
                        };
                        return ais_error(
                            400,
                            0,
                            &format!(
                                "Cannot change parent_id.  Use MOVE method.  {folder_id}, {parent} "
                            ),
                            ("categories.py", "Category.mergeUntrusted", 306),
                            Vec::new(),
                        );
                    }
                    let result = match parse_ais_rename_category_body(text) {
                        Ok(new_name) => sim.ais_rename_category(folder_id, new_name),
                        Err(_) => return CapsResponse::bad_request(),
                    };
                    return match result {
                        Ok(update) => ais_answer(
                            AisAnswer::new(200, name, agent).folder(sim, folder_id),
                            vec![
                                (
                                    "_updated_categories",
                                    ais_ids(update.updated_categories.iter().map(|id| id.uuid())),
                                ),
                                ("_updated_category_versions", ais_versions(&update)),
                            ],
                        ),
                        Err(error) => Self::inventory_error(error),
                    };
                }
                if let Some(item_id) = parse_ais_item_url(&suffix) {
                    let current = sim
                        .agent_inventory()
                        .item(item_id)
                        .map(|item| item.folder_id);
                    if let Some(parent) = new_parent
                        && parent.as_ref().ok() != current.as_ref()
                    {
                        let Ok(parent) = parent else {
                            return CapsResponse::bad_request();
                        };
                        return ais_error(
                            400,
                            0,
                            &format!(
                                "Cannot change parent_id.  Use MOVE method. {item_id} {parent}"
                            ),
                            ("items.py", "Item.mergeUntrusted", 732),
                            Vec::new(),
                        );
                    }
                    let result = match parse_ais_update_item_body(text) {
                        Ok(update_fields) => sim.ais_update_item(item_id, &update_fields),
                        Err(_) => return CapsResponse::bad_request(),
                    };
                    return match result {
                        Ok(update) => ais_answer(
                            AisAnswer::new(200, name, agent).item(sim, item_id),
                            vec![
                                ("_updated_category_versions", ais_versions(&update)),
                                ("_updated_items", ais_ids([item_id.uuid()])),
                            ],
                        ),
                        Err(error) => Self::inventory_error(error),
                    };
                }
                CapsResponse::bad_request()
            }
            "DELETE" if !read_only => {
                let (result, folders_removed) =
                    if let Some(folder_id) = parse_ais_category_children_url(&suffix) {
                        (sim.ais_purge_category(folder_id), true)
                    } else if let Some(folder_id) = parse_ais_category_url(&suffix) {
                        (sim.ais_remove_category(folder_id), true)
                    } else if let Some(item_id) = parse_ais_item_url(&suffix) {
                        (sim.ais_remove_item(item_id), false)
                    } else {
                        return CapsResponse::bad_request();
                    };
                match result {
                    Ok(update) => {
                        // An item deleted on its own is named where an
                        // emptied folder's items are.
                        let items: Vec<InventoryKey> = update
                            .category_items_removed
                            .iter()
                            .chain(&update.removed_items)
                            .copied()
                            .collect();
                        let mut keys = vec![
                            ("_active_gestures_removed", Llsd::Array(Vec::new())),
                            ("_attachments_removed", Llsd::Map(HashMap::new())),
                            (
                                "_broken_links_removed",
                                ais_ids(update.broken_links_removed.iter().map(|id| id.uuid())),
                            ),
                            (
                                "_category_items_removed",
                                ais_ids(items.iter().map(|id| id.uuid())),
                            ),
                            ("_updated_category_versions", ais_versions(&update)),
                            ("_wearables_removed", Llsd::Map(HashMap::new())),
                        ];
                        if folders_removed {
                            keys.push((
                                "_categories_removed",
                                ais_ids(update.categories_removed.iter().map(|id| id.uuid())),
                            ));
                            keys.push((
                                "_total_items_removed",
                                Llsd::Integer(i32::try_from(items.len()).unwrap_or(i32::MAX)),
                            ));
                        }
                        ais_answer(AisAnswer::new(200, name, agent), keys)
                    }
                    Err(error) => Self::inventory_error(error),
                }
            }
            // A Library folder copied into the agent's inventory, the
            // destination named by a `Destination` header (the reference's
            // `CopyLibraryCategory`).
            "COPY" if read_only => {
                let Some(source) = parse_ais_category_url(&suffix) else {
                    return CapsResponse::bad_request();
                };
                let Some(destination) = request
                    .destination
                    .and_then(|header| Uuid::parse_str(header.trim()).ok())
                    .map(sl_types::key::InventoryFolderKey::from)
                else {
                    return CapsResponse::bad_request();
                };
                match sim.ais_copy_library_category(source, destination) {
                    Ok((update, listing)) => {
                        let items = listing
                            .children
                            .as_ref()
                            .map(|children| children.items.clone())
                            .unwrap_or_default();
                        let embedded =
                            match ais_embedded(&[], &items, &["categories", "items", "links"]) {
                                Ok(embedded) => embedded,
                                Err(response) => return response,
                            };
                        // Only the destination's step: the copy itself is new.
                        let stepped = AisUpdate {
                            updated_category_versions: update
                                .updated_category_versions
                                .iter()
                                .filter(|(folder, _version)| *folder == destination)
                                .copied()
                                .collect(),
                            ..AisUpdate::default()
                        };
                        let top = listing.folder.folder_id;
                        ais_answer(
                            AisAnswer::new(201, name, agent).folder(sim, top),
                            vec![
                                (
                                    "_created_categories",
                                    ais_ids(update.created_categories.iter().map(|id| id.uuid())),
                                ),
                                (
                                    "_created_items",
                                    ais_ids(update.created_items.iter().map(|id| id.uuid())),
                                ),
                                ("_embedded", embedded),
                                ("_moved_items", Llsd::Array(Vec::new())),
                                ("_removed_items", Llsd::Array(Vec::new())),
                                ("_top_category_id", Llsd::String(top.to_string())),
                                ("_updated_category_versions", ais_versions(&stepped)),
                            ],
                        )
                    }
                    Err(error) => Self::inventory_error(error),
                }
            }
            // The method Second Life's own refusal of a move names, which its
            // service then refuses in turn.
            "MOVE" if parse_ais_category_url(&suffix).is_some() => {
                let path = suffix.split('?').next().unwrap_or_default().to_owned();
                ais_error(
                    405,
                    7,
                    "Method Not Allowed",
                    ("access.py", "Category._not_allowed", 61),
                    vec![
                        (
                            "allowed_methods",
                            Llsd::Array(
                                ["DELETE", "GET", "PATCH", "POST", "PUT", "HEAD", "OPTIONS"]
                                    .into_iter()
                                    .map(|method| Llsd::String(method.to_owned()))
                                    .collect(),
                            ),
                        ),
                        ("method", Llsd::String("MOVE".to_owned())),
                        ("url", Llsd::String(path)),
                    ],
                )
            }
            _ => CapsResponse::method_not_allowed(),
        }
    }

    /// Maps a serving-tree mutation failure to its HTTP status: an unknown
    /// target answers `404` (the AIS REST convention), an invalid parent
    /// (unknown, or a cycle-creating move) `400`.
    const fn inventory_error(error: SimInventoryError) -> CapsResponse {
        match error {
            SimInventoryError::UnknownTarget => CapsResponse::not_found(),
            SimInventoryError::InvalidParent => CapsResponse::bad_request(),
        }
    }

    /// Serves the bodyless `SimulatorFeatures` GET: the session's stored
    /// feature document ([`SimSession::set_simulator_features`]). Wrong
    /// method → `405`.
    fn dispatch_simulator_features(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        CapsResponse::llsd_xml(build_simulator_features_response(sim.simulator_features()))
    }

    /// Serves the bodyless `LSLSyntax` GET: the session's stored syntax
    /// document ([`SimSession::set_lsl_syntax`]), or an empty table when none
    /// was set. Wrong method → `405`.
    fn dispatch_lsl_syntax(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        CapsResponse::llsd_xml(sim.lsl_syntax_document().map_or_else(
            || build_lsl_syntax_document(&sl_wire::LslSyntax::default()),
            str::to_owned,
        ))
    }

    /// Serves the `ExtEnvironment` EEP surface. GET answers the stored
    /// settings for the `?parcelid=` query (absent → `-1`, the region; an
    /// unset parcel inherits the region entry). PUT parses the update body,
    /// applies it via [`SimSession::apply_environment_update`], and echoes
    /// the stored result (the `{ environment, success: true }` envelope the
    /// reference viewer reads back); a `day_asset`-only update answers a
    /// graceful `200 { success: false, message }` — the fixture has no
    /// settings-asset store to resolve the id against. DELETE drops the
    /// stored entry ([`SimSession::reset_environment`]) and echoes what the
    /// land inherits instead. A malformed query or body → `400`; other
    /// methods → `405`.
    fn dispatch_environment(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        let Ok(parcel_id) = parse_parcel_id_query(request.query) else {
            return CapsResponse::bad_request();
        };
        match request.method {
            "GET" => CapsResponse::llsd_xml(sim.environment_body(parcel_id).to_llsd_xml()),
            "PUT" => {
                let Ok(track_no) = parse_track_no_query(request.query) else {
                    return CapsResponse::bad_request();
                };
                let Some(body) = parse_llsd_body(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Some(update) = environment_update_from_llsd(&body) else {
                    return CapsResponse::bad_request();
                };
                if update.day_cycle.is_none() && update.day_asset.is_some() {
                    return CapsResponse::llsd_xml(environment_failure_body(
                        "day_asset updates are not supported: this simulator has no \
                         settings-asset store",
                    ));
                }
                let stored = sim.apply_environment_update(parcel_id, track_no, update);
                if sim.bare_environment_replies() {
                    return CapsResponse::llsd_xml(
                        Llsd::Map(HashMap::from([("success".to_owned(), Llsd::Boolean(true))]))
                            .to_llsd_xml(),
                    );
                }
                CapsResponse::llsd_xml(environment_to_llsd(&stored).to_llsd_xml())
            }
            "DELETE" => {
                let Ok(track_no) = parse_track_no_query(request.query) else {
                    return CapsResponse::bad_request();
                };
                let _inherited = sim.reset_environment(parcel_id, track_no);
                if sim.bare_environment_replies() {
                    return CapsResponse::llsd_xml(
                        Llsd::Map(HashMap::from([
                            ("messageID".to_owned(), Llsd::Uuid(Uuid::nil())),
                            ("regionID".to_owned(), Llsd::Uuid(sim.region_id())),
                            ("success".to_owned(), Llsd::Boolean(true)),
                        ]))
                        .to_llsd_xml(),
                    );
                }
                CapsResponse::llsd_xml(sim.environment_body(parcel_id).to_llsd_xml())
            }
            _ => CapsResponse::method_not_allowed(),
        }
    }

    /// Serves one `RemoteParcelRequest` POST: resolves the requested region +
    /// location against the parcel-cover store
    /// ([`SimSession::resolve_remote_parcel`]). A hit answers the parcel id;
    /// a miss (foreign region or uncovered location) answers a `200` empty
    /// map — the "could not resolve" signal. Wrong method → `405`, malformed
    /// body → `400`.
    fn dispatch_remote_parcel(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(remote) = parse_remote_parcel_request(&body) else {
            return CapsResponse::bad_request();
        };
        match sim.resolve_remote_parcel(&remote) {
            Some(parcel_id) => CapsResponse::llsd_xml(build_remote_parcel_response(parcel_id)),
            None => CapsResponse::llsd_xml(Llsd::Map(HashMap::new()).to_llsd_xml()),
        }
    }

    /// Serves one `ParcelPropertiesUpdate` POST: reads the parcel record and
    /// hands it to the driver as
    /// [`ServerEvent::ParcelPropertiesUpdated`](crate::ServerEvent::ParcelPropertiesUpdated),
    /// the event the UDP message raises, so both forms land in one place. The
    /// reply is an empty map; the reference viewer ignores it. Wrong method →
    /// `405`, malformed body → `400`.
    fn dispatch_parcel_update(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(update) = crate::parse_parcel_properties_update_request(&body) else {
            return CapsResponse::bad_request();
        };
        sim.push_parcel_update(update);
        CapsResponse::llsd_xml(Llsd::Map(HashMap::new()).to_llsd_xml())
    }

    /// Serves one `GetObjectCost` POST: the stored costs of the requested
    /// objects ([`SimSession::set_object_cost`]); unknown ids are omitted
    /// (the "no such object" signal). Wrong method → `405`, malformed body →
    /// `400`.
    fn dispatch_object_cost(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(ids) = parse_get_object_cost_request(&body) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_get_object_cost_response(&sim.object_costs(&ids)))
    }

    /// Serves one `GetObjectPhysicsData` POST: the stored physics data of the
    /// requested objects ([`SimSession::set_object_physics`]); unknown ids
    /// are omitted. Wrong method → `405`, malformed body → `400`.
    fn dispatch_object_physics(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(ids) = parse_get_object_physics_data_request(&body) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_get_object_physics_data_response(
            &sim.object_physics(&ids),
        ))
    }

    /// Serves one `ResourceCostSelected` POST: the component-wise sum of the
    /// stored selection costs of the requested objects
    /// ([`SimSession::set_selection_cost`]); the roots/prims request form is
    /// validated but does not change the arithmetic. Wrong method → `405`,
    /// malformed body → `400`.
    fn dispatch_resource_cost_selected(
        sim: &SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok((_kind, ids)) = parse_resource_cost_selected_request(&body) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_resource_cost_selected_response(
            &sim.selection_cost(&ids),
        ))
    }

    /// Serves the bodyless `AttachmentResources` GET: the agent's stored
    /// scripted-attachment report
    /// ([`SimSession::set_attachment_resources`]), labelled as the session's
    /// grid labels it ([`SimSession::set_attachment_resources_label`]). Wrong
    /// method → `405`.
    fn dispatch_attachment_resources(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let response = CapsResponse::llsd_xml(build_attachment_resources_response(
            sim.attachment_resources(),
        ));
        match sim.attachment_resources_label() {
            LlsdLabel::Llsd => response,
            LlsdLabel::ServerDefault => CapsResponse {
                content_type: OPENSIM_DEFAULT_CONTENT_TYPE,
                ..response
            },
        }
    }

    /// Reads one `AvatarRenderInfo` request: a GET asks the driver for the
    /// region's statement, a POST hands it the report. Second Life answers
    /// every POST `200 {}`, so a report it cannot read is answered the same
    /// and files nothing; any other method is Second Life's `405` (measured
    /// on aditi, 2026-10-21 — `book/src/gridspec/avatars.md` § Render info).
    fn dispatch_avatar_render_info(sim: &SimSession, request: &CapsRequest<'_>) -> CapsDispatch {
        let Some(agent) = sim.agent_id() else {
            return CapsDispatch::Response(CapsResponse::not_found());
        };
        match request.method {
            "GET" => CapsDispatch::AvatarRenderInfoRead { reader: agent },
            "POST" => match parse_llsd_body(request.body)
                .as_ref()
                .map(parse_avatar_render_report_request)
            {
                Some(Ok(reports)) => CapsDispatch::AvatarRenderInfoReport {
                    reporter: agent,
                    reports,
                },
                Some(Err(_)) | None => {
                    CapsDispatch::Response(Self::avatar_render_report_response())
                }
            },
            _ => CapsDispatch::Response(CapsResponse {
                body: b"Method Not Allowed".to_vec(),
                ..CapsResponse::method_not_allowed()
            }),
        }
    }

    /// The answer to a [`CapsDispatch::AvatarRenderInfoRead`]: the region's
    /// statement.
    #[must_use]
    pub fn avatar_render_info_response(info: &AvatarRenderInfo) -> CapsResponse {
        CapsResponse::llsd_xml(build_avatar_render_info_response(info))
    }

    /// The answer to a [`CapsDispatch::AvatarRenderInfoReport`], and to any
    /// POST of `AvatarRenderInfo`: Second Life's empty map.
    #[must_use]
    pub fn avatar_render_report_response() -> CapsResponse {
        CapsResponse::llsd_xml(build_avatar_render_report_response())
    }

    /// Serves the two-stage `LandResources` surface. The cap URL itself takes
    /// the `{ parcel_id }` POST (validated; the stored reports are served
    /// as-is regardless of the requested parcel — their scope is the
    /// driver's choice) and answers the summary/detail follow-up URLs,
    /// minted as the cap's own sub-paths. The follow-up GETs serve the
    /// stored reports ([`SimSession::set_land_resource_summary`] /
    /// [`SimSession::set_land_resource_details`]). Wrong method → `405`,
    /// malformed POST body → `400`, unknown sub-path → `404`.
    fn dispatch_land_resources(&self, sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        match cap_sub_path(request.path) {
            None => {
                if request.method != "POST" {
                    return CapsResponse::method_not_allowed();
                }
                let Some(body) = parse_llsd_body(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok(_parcel_id) = parse_land_resources_request(&body) else {
                    return CapsResponse::bad_request();
                };
                CapsResponse::llsd_xml(build_land_resources_response(&LandResourcesUrls {
                    script_resource_summary: Some(
                        self.land_resources_url(LAND_RESOURCES_SUMMARY_SUB_PATH),
                    ),
                    script_resource_details: Some(
                        self.land_resources_url(LAND_RESOURCES_DETAIL_SUB_PATH),
                    ),
                }))
            }
            Some(LAND_RESOURCES_SUMMARY_SUB_PATH) => {
                if request.method != "GET" {
                    return CapsResponse::method_not_allowed();
                }
                CapsResponse::llsd_xml(build_land_resource_summary_response(
                    sim.land_resource_summary(),
                ))
            }
            Some(LAND_RESOURCES_DETAIL_SUB_PATH) => {
                if request.method != "GET" {
                    return CapsResponse::method_not_allowed();
                }
                CapsResponse::llsd_xml(build_land_resource_detail_response(
                    sim.land_resource_details(),
                ))
            }
            Some(_) => CapsResponse::not_found(),
        }
    }

    /// Mints a `LandResources` follow-up URL: the cap's own URL plus the
    /// given sub-path (which [`SimCaps::resolve`] tolerates and
    /// [`SimCaps::dispatch_land_resources`] routes on) — the
    /// [`SimCaps::screenshot_uploader_url`] pattern.
    fn land_resources_url(&self, sub_path: &str) -> Url {
        let token = self
            .tokens
            .get(CAP_LAND_RESOURCES)
            .copied()
            .unwrap_or_default();
        let mut url = self.cap_url(token);
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.push(sub_path);
        }
        url
    }

    /// Serves one `GetExperienceInfo` GET (the `/id/?public_id=…`
    /// sub-path-plus-query form the client builder emits): the stored
    /// record per requested id ([`SimSession::experiences`]); unknown ids
    /// answer as `error_ids` entries. An empty or absent query answers
    /// `200` with no records — asking about nothing is not malformed — but a
    /// `public_id` that is not a UUID is a `400`, like every other
    /// unreadable request. Wrong method → `405`.
    fn dispatch_experience_info(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(ids) = parse_experience_info_query(&ais_suffix(request)) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_experience_infos_response(
            &sim.experiences().infos(&ids),
        ))
    }

    /// Serves one `FindExperienceByName` GET: a 1-based
    /// `SEARCH_PAGE_SIZE` page of public records whose name contains the
    /// search text case-insensitively ([`SimExperiences::find`], which
    /// hides invalid and private records). A query missing `page` or
    /// `query` → `400`; wrong method → `405`.
    ///
    /// A neighbouring page that exists is named by the `next_page_url` /
    /// `previous_page_url` the reference viewer enables its paging arrows
    /// from — written as the query suffix that would fetch it, which is the
    /// request this very handler would answer.
    ///
    /// [`SimExperiences::find`]: crate::SimExperiences::find
    fn dispatch_experience_search(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let Some((text, page)) = parse_find_experience_query(&ais_suffix(request)) else {
            return CapsResponse::bad_request();
        };
        let found = sim.experiences().find(&text, page);
        // Second Life numbers the neighbours from the page it was asked for, not
        // the one it clamped to: page 0 offers page 2 next, page 9 of 4 offers
        // page 8 back (aditi, 2026-10-16).
        let next = found
            .has_next_page
            .then(|| find_experience_query(&text, page.max(1).saturating_add(1)));
        let previous = found
            .has_previous_page
            .then(|| find_experience_query(&text, page.saturating_sub(1)));
        CapsResponse::llsd_xml(build_experience_search_response(
            &found.infos,
            next.as_deref(),
            previous.as_deref(),
        ))
    }

    /// Serves the bodyless `GetExperiences` GET: the agent's allowed /
    /// blocked preference lists ([`SimSession::experiences`]). Wrong
    /// method → `405`.
    fn dispatch_experience_permissions(
        sim: &SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let (allowed, blocked) = sim.experiences().agent_permissions();
        CapsResponse::llsd_xml(build_experience_permissions_response(&allowed, &blocked))
    }

    /// Serves the `ExperiencePreferences` mutation surface. PUT parses the
    /// `{ "<id>": { permission } }` body and applies `Allow`/`Block` (a
    /// malformed or non-permission body → `400`); DELETE parses the
    /// `?<id>` query and forgets the preference (missing/unparsable id →
    /// `400`). Both echo the full post-mutation permission lists — the
    /// same reply shape as `GetExperiences`, which is how the client folds
    /// it. Any experience id is accepted without a record lookup (a
    /// documented exception to the `404`-on-unknown rule: a preference is
    /// the agent's own keyed entry, and viewers can block ids they never
    /// resolved). Other methods → `405`.
    fn dispatch_experience_preferences(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        let (id, permission) = match request.method {
            "PUT" => {
                let Ok(body) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok(parsed) = parse_set_experience_permission_request(body) else {
                    return CapsResponse::bad_request();
                };
                parsed
            }
            "DELETE" => {
                let Some(id) = parse_forget_experience_query(&ais_suffix(request)) else {
                    return CapsResponse::bad_request();
                };
                (id, ExperiencePermission::Forget)
            }
            _ => return CapsResponse::method_not_allowed(),
        };
        let (allowed, blocked) = sim.set_experience_preference(id, permission);
        CapsResponse::llsd_xml(build_experience_permissions_response(&allowed, &blocked))
    }

    /// Serves the experience id-list GETs, routed on the cap name:
    /// `AgentExperiences` → the owned list, `GetAdminExperiences` → the
    /// admin list, `GetCreatorExperiences` → the creator list,
    /// `GroupExperiences` → the `?<group_id>` group's list (missing or
    /// unparsable group id → `400`; an unknown group answers an empty
    /// list). Wrong method → `405`.
    fn dispatch_experience_id_list(
        sim: &SimSession,
        request: &CapsRequest<'_>,
        name: &str,
    ) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let ids = if name == CAP_GROUP_EXPERIENCES {
            let Some(group_id) = parse_group_experiences_query(&ais_suffix(request)) else {
                return CapsResponse::bad_request();
            };
            sim.experiences().group(group_id)
        } else if name == CAP_GET_ADMIN_EXPERIENCES {
            sim.experiences().admin()
        } else if name == CAP_GET_CREATOR_EXPERIENCES {
            sim.experiences().creator()
        } else {
            sim.experiences().owned()
        };
        CapsResponse::llsd_xml(build_experience_ids_response(&ids))
    }

    /// Serves the `IsExperienceAdmin` / `IsExperienceContributor` status
    /// GETs, routed on the cap name: `{ status }` from the store's admin /
    /// creator membership. An unknown experience id answers
    /// `{ status: false }`, never an error; a missing or unparsable
    /// `?experience_id=` query → `400`; wrong method → `405`.
    fn dispatch_experience_status(
        sim: &SimSession,
        request: &CapsRequest<'_>,
        name: &str,
    ) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let Some(id) = parse_experience_id_query(&ais_suffix(request)) else {
            return CapsResponse::bad_request();
        };
        let status = if name == CAP_IS_EXPERIENCE_ADMIN {
            sim.experiences().is_admin(id)
        } else {
            sim.experiences().is_contributor(id)
        };
        CapsResponse::llsd_xml(build_experience_status_response(status))
    }

    /// Serves one `UpdateExperience` POST: parses the edit body, applies it
    /// to the stored record ([`SimSession::apply_experience_update`]), and
    /// echoes the updated record in the wrapped `{ experience_keys }` form
    /// (the client folds the first record into its updated-experience
    /// event). An experience the agent does not administer (an unknown one
    /// included) → `403` with Second Life's sentence; a malformed body →
    /// `400`; wrong method → `405`.
    fn dispatch_update_experience(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(body) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(update) = parse_update_experience_request(body) else {
            return CapsResponse::bad_request();
        };
        // Second Life refuses an edit by anyone who does not administer the
        // experience with a 403 and a sentence, not LLSD (aditi, 2026-10-16).
        if !sim.experiences().is_admin(update.public_id) {
            let agent = sim.agent_id().map_or_else(Uuid::nil, |agent| agent.uuid());
            return CapsResponse::words(
                403,
                &format!(
                    "Agent {agent} is not an admin for experience {}",
                    update.public_id
                ),
            );
        }
        match sim.apply_experience_update(update) {
            Some(updated) => CapsResponse::llsd_xml(build_experience_infos_response(&[updated])),
            None => CapsResponse::not_found(),
        }
    }

    /// Serves the `RegionExperiences` surface. GET answers the region's
    /// stored allowed / blocked / trusted lists, plus the estate's `default`
    /// experience when it has one; POST parses the same-shaped body, replaces
    /// the three lists wholesale
    /// ([`SimSession::apply_region_experiences`]), and echoes what is stored.
    /// A malformed POST body → `400`; other methods → `405`.
    ///
    /// The POST's echo carries the default too, exactly as the GET does: the
    /// reference re-reads the reply through the same `processResponse`, and a
    /// reply that dropped the key would leave the panel's sticky row on the
    /// last thing it read rather than on what the estate now says.
    fn dispatch_region_experiences(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        match request.method {
            "GET" => CapsResponse::llsd_xml(build_region_experiences_response(
                &sim.experiences().region_lists(),
            )),
            "POST" => {
                let Ok(body) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok(posted) = parse_region_experiences_request(body) else {
                    return CapsResponse::bad_request();
                };
                let stored =
                    sim.apply_region_experiences(posted.allowed, posted.blocked, posted.trusted);
                CapsResponse::llsd_xml(build_region_experiences_response(&stored))
            }
            _ => CapsResponse::method_not_allowed(),
        }
    }

    /// Serves the `ExperienceQuery` GET: for each experience the query names,
    /// whether the parcel it names admits it
    /// ([`SimExperiences::parcel_experiences`]). A query without a `parcelid`
    /// → `400`; other methods → `405`.
    ///
    /// The reference asks this whenever an agent with an injected environment
    /// steps over a parcel line, and clears the injections of every experience
    /// answered `false` — so a fixture that declares a parcel's admitted
    /// experiences is declaring where that parcel's visitors keep their sky.
    fn dispatch_experience_query(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let Some((parcel_id, ids)) = parse_experience_query(&ais_suffix(request)) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_experience_query_response(
            &sim.experiences().parcel_experiences(parcel_id, &ids),
        ))
    }

    /// Serves one `ProvisionVoiceAccountRequest` POST from the voice stub.
    /// A refusal maps to the status the viewer interprets
    /// (`llvoicewebrtc.cpp`, `LLVoiceWebRTCConnection::OnVoiceConnectionRequestFailure`):
    /// bad channel credentials → `401` ("channel locked"), a logout for an
    /// unknown `viewer_session` → `404`, an unavailable backend / missing
    /// offer / unknown channel → `400`. The body must be LLSD (else `400`);
    /// the request's fields are otherwise decoded leniently.
    fn dispatch_provision_voice_account(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(body) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok(provision) = parse_provision_voice_account_request(body) else {
            return CapsResponse::bad_request();
        };
        match sim.provision_voice(provision) {
            Ok(info) => CapsResponse::llsd_xml(build_provision_voice_account_response(&info)),
            Err(VoiceProvisionRefusal::BadCredentials) => CapsResponse::unauthorized(),
            Err(VoiceProvisionRefusal::UnknownSession) => CapsResponse::not_found(),
            Err(_refused) => CapsResponse::bad_request(),
        }
    }

    /// Serves one `ParcelVoiceInfoRequest` POST: the body is ignored (the
    /// viewer sends `undef`), the reply describes the agent's recorded
    /// parcel — its stored channel, or the empty-`channel_uri` "no voice
    /// here" form.
    fn dispatch_parcel_voice_info(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        CapsResponse::llsd_xml(build_parcel_voice_info_response(&sim.parcel_voice_info()))
    }

    /// Serves one `VoiceSignalingRequest` POST (the WebRTC ICE trickle):
    /// records the batch on its connection and acks with an undefined body
    /// (the viewer only looks at the status — a non-2xx makes it restart the
    /// voice session). An unknown `viewer_session` answers `404`; a
    /// malformed body `400`.
    fn dispatch_voice_signaling(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Ok(body) = std::str::from_utf8(request.body) else {
            return CapsResponse::bad_request();
        };
        let Ok((viewer_session, candidates, completed)) = parse_voice_signaling_request(body)
        else {
            return CapsResponse::bad_request();
        };
        if sim.record_voice_signaling(viewer_session, candidates, completed) {
            CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
        } else {
            CapsResponse::not_found()
        }
    }

    /// Mints the URL for one capability token: `{base}/cap/{token}`.
    ///
    /// Built via `path_segments_mut` rather than `Url::join` (whose
    /// trailing-slash semantics would drop the base's last path segment). A
    /// cannot-be-a-base URL is returned unchanged — the constructor
    /// documents that `base_url` must be HTTP(S).
    fn cap_url(&self, token: Uuid) -> Url {
        let mut url = self.base_url.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().push("cap").push(&token.to_string());
        }
        url
    }

    /// Resolves a request path to the seed, a granted capability, or
    /// unknown. Matches on the **last** `/cap/<token>` pair so any base-URL
    /// path prefix works; segments after the token (the capability sub-path
    /// future cluster tasks route on) are ignored here.
    fn resolve(&self, path: &str) -> ResolvedPath {
        let Some((_, after)) = path.rsplit_once("/cap/") else {
            return ResolvedPath::Unknown;
        };
        let token_str = after.split_once('/').map_or(after, |(token, _)| token);
        let Ok(token) = Uuid::parse_str(token_str) else {
            return ResolvedPath::Unknown;
        };
        if token == self.seed_token {
            return ResolvedPath::Seed;
        }
        self.tokens
            .iter()
            .find(|(_, minted)| **minted == token)
            .map_or(ResolvedPath::Unknown, |(name, _)| {
                ResolvedPath::Capability(name)
            })
    }
}

/// Parses a CAPS request body as LLSD-XML, or `None` (→ `400`) when it is not
/// UTF-8 or not well-formed.
fn parse_llsd_body(body: &[u8]) -> Option<Llsd> {
    let text = std::str::from_utf8(body).ok()?;
    parse_llsd_xml(text).ok()
}

/// The value of `key` in a raw query string (`a=1&b=2`), if present.
fn query_param<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _value)| *name == key)
        .map(|(_name, value)| value)
}

/// The `ExtEnvironment` `?parcelid=` query value: absent → `-1` (the region),
/// present but not a decimal integer → `Err` (→ `400`).
fn parse_parcel_id_query(query: Option<&str>) -> Result<i32, ()> {
    match query_param(query, "parcelid") {
        None => Ok(-1),
        Some(value) => value.parse::<i32>().map_err(|_error| ()),
    }
}

/// The `ExtEnvironment` `?trackno=` query value: absent → `None`, present but
/// not a decimal integer → `Err` (→ `400`).
fn parse_track_no_query(query: Option<&str>) -> Result<Option<i32>, ()> {
    match query_param(query, "trackno") {
        None => Ok(None),
        Some(value) => value.parse::<i32>().map(Some).map_err(|_error| ()),
    }
}

/// The `ExtEnvironment` graceful-failure reply body:
/// `{ success: false, message }`, HTTP `200` — the reference viewer reads
/// `message` as the failure reason (its `FAIL_REASON` path) rather than an
/// HTTP error.
fn environment_failure_body(message: &str) -> String {
    Llsd::Map(HashMap::from([
        ("success".to_owned(), Llsd::Boolean(false)),
        ("message".to_owned(), Llsd::String(message.to_owned())),
    ]))
    .to_llsd_xml()
}

/// The ticket an uploader sub-path (`upload/<ticket>`, the ticket in hex)
/// names, or `None` when the sub-path is not an uploader's.
fn upload_ticket(sub_path: &str) -> Option<u128> {
    let ticket = sub_path.strip_prefix(UPLOAD_SUB_PATH)?.strip_prefix('/')?;
    u128::from_str_radix(ticket, 16).ok()
}

/// The sub-path below a capability URL's token, if any: the segment(s) after
/// the last `/cap/<token>/`, without a leading slash. Mirrors
/// [`SimCaps::resolve`]'s tolerance for sub-paths — `resolve` routes on the
/// token and ignores what follows; handlers that serve sub-resources (the
/// screenshot uploader) route on this.
fn cap_sub_path(path: &str) -> Option<&str> {
    let (_, after) = path.rsplit_once("/cap/")?;
    after
        .split_once('/')
        .map(|(_token, sub_path)| sub_path)
        .filter(|sub_path| !sub_path.is_empty())
}

/// Reconstructs the sl-wire URL suffix (any capability sub-path plus the
/// original query string) from a dispatched request — the exact form the
/// client-side URL builders emit and the `parse_ais_*_url` inverses consume.
/// The experience-cluster query parsers (`parse_experience_info_query` and
/// friends) consume the same suffix shape: `GetExperienceInfo`'s `/id/?…`
/// sub-path round-trips through it, and the bare-query caps yield `/?…`,
/// whose leading segment the parsers ignore.
fn ais_suffix(request: &CapsRequest<'_>) -> String {
    let sub_path = cap_sub_path(request.path).unwrap_or_default();
    match request.query {
        Some(query) => format!("/{sub_path}?{query}"),
        None => format!("/{sub_path}"),
    }
}

/// Which object an AIS3 `GET` answers at its top level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AisTop {
    /// `GET /category/<id>`: the folder alone.
    Folder,
    /// `GET /category/<id>/children`: the folder and its listing.
    Children,
    /// `GET /item/<id>`: one item.
    Item,
}

/// Dresses an AIS3 `GET` answer as Second Life's inventory service states
/// it (`book/src/gridspec/inventory.md` § Fetching): `_base_uri` at the top
/// (`slcap://` and the capability's name), `_links` on every folder and item
/// (`self` and `parent`, and on the top folder its `categories`, `children`,
/// `items` and `links` — a children listing adds a `category` link to the
/// folder itself, and its `self` is the listing), the asking agent as
/// `agent_id`, and every id stated as a string; an item carries no
/// `is_owner_group`.
fn decorate_ais(body: &mut Llsd, cap: &str, agent: Uuid, top: AisTop) {
    /// An href, as the service types one.
    fn href(path: String) -> Llsd {
        Llsd::Map([("href".to_owned(), Llsd::Uri(path))].into_iter().collect())
    }
    /// A UUID-valued field as text.
    fn text_of(map: &HashMap<String, Llsd>, key: &str) -> String {
        match map.get(key) {
            Some(Llsd::Uuid(id)) => id.to_string(),
            Some(Llsd::String(text)) => text.clone(),
            _ => Uuid::nil().to_string(),
        }
    }
    /// States each of `keys` held as a UUID as a string.
    fn stringify(map: &mut HashMap<String, Llsd>, keys: &[&str]) {
        for key in keys {
            if let Some(value) = map.get_mut(*key)
                && let Llsd::Uuid(id) = value
            {
                *value = Llsd::String(id.to_string());
            }
        }
    }
    /// Dresses one folder or item map, and what it embeds.
    fn dress(map: &mut HashMap<String, Llsd>, agent: Uuid, top: Option<AisTop>) {
        let is_item = map.contains_key("item_id");
        let parent = text_of(map, "parent_id");
        let mut links: Vec<(&str, Llsd)> = vec![("parent", href(format!("/category/{parent}")))];
        if is_item {
            links.push(("self", href(format!("/item/{}", text_of(map, "item_id")))));
            stringify(map, &["item_id", "parent_id", "asset_id", "linked_id"]);
            if let Some(Llsd::Map(permissions)) = map.get_mut("permissions") {
                let _group = permissions.remove("is_owner_group");
                stringify(
                    permissions,
                    &["creator_id", "group_id", "last_owner_id", "owner_id"],
                );
            }
        } else {
            let id = text_of(map, "category_id");
            if let Some(top) = top {
                for part in ["categories", "children", "items", "links"] {
                    links.push((part, href(format!("/category/{id}/{part}"))));
                }
                if top == AisTop::Children {
                    let mut category = href(format!("/category/{id}"));
                    if let Llsd::Map(fields) = &mut category {
                        let _name =
                            fields.insert("name".to_owned(), Llsd::String("self".to_owned()));
                    }
                    links.push(("category", category));
                    links.push(("self", href(format!("/category/{id}/children"))));
                } else {
                    links.push(("self", href(format!("/category/{id}"))));
                }
            } else {
                links.push(("self", href(format!("/category/{id}"))));
            }
            stringify(map, &["category_id", "parent_id"]);
        }
        let _links = map.insert(
            "_links".to_owned(),
            Llsd::Map(
                links
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value))
                    .collect(),
            ),
        );
        let _agent = map.insert("agent_id".to_owned(), Llsd::String(agent.to_string()));
        if let Some(Llsd::Map(embedded)) = map.get_mut("_embedded") {
            for kind in ["categories", "items", "links"] {
                if let Some(Llsd::Map(entries)) = embedded.get_mut(kind) {
                    for entry in entries.values_mut() {
                        if let Llsd::Map(entry) = entry {
                            dress(entry, agent, None);
                        }
                    }
                }
            }
        }
    }
    let Llsd::Map(map) = body else {
        return;
    };
    dress(map, agent, Some(top));
    let _base = map.insert("_base_uri".to_owned(), Llsd::Uri(format!("slcap://{cap}")));
}

/// One AIS3 mutation answer as Second Life's inventory service states it
/// (`book/src/gridspec/inventory.md` § Changing inventory): its status, and
/// the folder or item the request named at the top level — dressed as a
/// `GET` of it is ([`decorate_ais`]) — where the answer carries one.
struct AisAnswer<'a> {
    /// The status: `201` for what makes something, `200` otherwise.
    status: u16,
    /// The capability's name, for `_base_uri`.
    cap: &'a str,
    /// The asking agent.
    agent: Uuid,
    /// The object at the top level, undressed.
    top: Option<(Llsd, AisTop)>,
}

impl<'a> AisAnswer<'a> {
    /// An answer of `status` with nothing at the top level.
    const fn new(status: u16, cap: &'a str, agent: Uuid) -> Self {
        Self {
            status,
            cap,
            agent,
            top: None,
        }
    }

    /// The answer with the agent's folder `folder_id` at the top level, as
    /// the operation left it.
    fn folder(mut self, sim: &SimSession, folder_id: sl_types::key::InventoryFolderKey) -> Self {
        self.top = sim
            .agent_inventory()
            .folder(folder_id)
            .cloned()
            .and_then(|folder| {
                ais_category_children_reply_to_llsd(&InventoryListing::unlisted(folder)).ok()
            })
            .map(|body| (body, AisTop::Folder));
        self
    }

    /// The answer with the agent's item `item_id` at the top level, as the
    /// operation left it.
    fn item(mut self, sim: &SimSession, item_id: sl_types::key::InventoryKey) -> Self {
        self.top = sim
            .agent_inventory()
            .item(item_id)
            .and_then(|item| ais_item_reply_to_llsd(item).ok())
            .map(|body| (body, AisTop::Item));
        self
    }
}

/// Builds the AIS3 answer `answer` with the `_` keys `keys` — every key
/// stated even when it names nothing, as Second Life states them.
fn ais_answer(answer: AisAnswer<'_>, keys: Vec<(&str, Llsd)>) -> CapsResponse {
    let AisAnswer {
        status,
        cap,
        agent,
        top,
    } = answer;
    let (mut body, top) = top.map_or_else(
        || (Llsd::Map(HashMap::new()), None),
        |(body, top)| (body, Some(top)),
    );
    if let Llsd::Map(map) = &mut body {
        for (key, value) in keys {
            let _previous = map.insert(key.to_owned(), value);
        }
    }
    if let Some(top) = top {
        decorate_ais(&mut body, cap, agent, top);
    }
    CapsResponse::llsd_xml_status(status, body.to_llsd_xml())
}

/// Ids as an AIS3 answer lists them.
fn ais_ids(ids: impl IntoIterator<Item = Uuid>) -> Llsd {
    Llsd::Array(ids.into_iter().map(Llsd::Uuid).collect())
}

/// The `_updated_category_versions` of `update`: each folder's version as
/// the operation left it.
fn ais_versions(update: &AisUpdate) -> Llsd {
    Llsd::Map(
        update
            .updated_category_versions
            .iter()
            .map(|(folder, version)| (folder.to_string(), Llsd::Integer(*version)))
            .collect(),
    )
}

/// The `_embedded` block of `folders` and `items`, keeping only the kinds
/// `kinds` names — an answer about links embeds only `links`.
fn ais_embedded(
    folders: &[InventoryListing],
    items: &[InventoryItem],
    kinds: &[&str],
) -> Result<Llsd, CapsResponse> {
    let Ok(update) = ais_inventory_update_to_llsd(folders, items) else {
        return Err(CapsResponse::internal_error());
    };
    let mut embedded = update
        .get("_embedded")
        .cloned()
        .unwrap_or_else(|| Llsd::Map(HashMap::new()));
    if let Llsd::Map(map) = &mut embedded {
        map.retain(|kind, _entries| kinds.contains(&kind.as_str()));
    }
    Ok(embedded)
}

/// An AIS3 error as Second Life's inventory service states one: the status,
/// and an LLSD map of `error_code`, `error_description` and where in the
/// service it was raised (`error_filename`, `error_function`,
/// `error_line_number`), plus any `extra` fields — the values measured by
/// the `inventory-fetch` census (`book/src/gridspec/inventory.md` § Fetching).
fn ais_error(
    status: u16,
    code: i32,
    description: &str,
    (file, function, line): (&str, &str, i32),
    extra: Vec<(&str, Llsd)>,
) -> CapsResponse {
    let mut fields = vec![
        ("error_code", Llsd::Integer(code)),
        ("error_description", Llsd::String(description.to_owned())),
        ("error_filename", Llsd::String(file.to_owned())),
        ("error_function", Llsd::String(function.to_owned())),
        ("error_line_number", Llsd::Integer(line)),
    ];
    fields.extend(extra);
    let body = Llsd::Map(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    );
    CapsResponse::llsd_xml_status(status, body.to_llsd_xml())
}

/// Second Life's AIS3 answer for a folder it does not hold.
fn category_not_found(folder_id: sl_types::key::InventoryFolderKey) -> CapsResponse {
    ais_error(
        404,
        4,
        "Category not found.",
        ("category.py", "Category._get", 427),
        vec![("category_id", Llsd::String(folder_id.to_string()))],
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::REQUESTED_CAPABILITIES;

    /// The test-error type: any assertion helper failure propagates via `?`.
    type TestError = Box<dyn std::error::Error>;

    /// A capability's server-side dispatch status in the pinned table.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CapStatus {
        /// A handler is registered ([`SERVED_CAPABILITIES`] +
        /// [`SimCaps::handler_for`]).
        Served,
        /// No server-side handler yet — a `protocol-sim-caps-*` cluster
        /// task will wire one.
        Pending,
    }

    /// Builds a [`SimCaps`] with a deterministic token mint for tests.
    fn caps() -> Result<SimCaps, TestError> {
        let base: Url = "http://127.0.0.1:9001/".parse()?;
        let mut next: u128 = 0;
        let mint = move || {
            next = next.wrapping_add(1);
            Uuid::from_u128(next)
        };
        Ok(SimCaps::new(base, Uuid::from_u128(0x5eed), mint))
    }

    /// **The server-side CAPS coverage table, pinned.** One row per
    /// [`REQUESTED_CAPABILITIES`] entry, in order. The table pins *dispatch
    /// registration* (a handler wired into [`SimCaps::handler_for`]), not
    /// codec existence — several capabilities already have sl-wire
    /// `parse_*_request`/`build_*_response` inverses but stay `Pending`
    /// until a cluster task routes them. Those tasks flip rows to `Served`
    /// as they land; any other change to this table is a loud diff.
    #[test]
    fn caps_coverage_table_is_pinned() {
        let expected: Vec<(&str, CapStatus)> = vec![
            ("EventQueueGet", CapStatus::Served),
            ("FetchInventoryDescendents2", CapStatus::Served),
            ("FetchLibDescendents2", CapStatus::Served),
            ("FetchInventory2", CapStatus::Served),
            ("FetchLib2", CapStatus::Served),
            ("GroupMemberData", CapStatus::Served),
            ("GetTexture", CapStatus::Served),
            ("GetMesh", CapStatus::Served),
            ("GetMesh2", CapStatus::Served),
            ("ViewerAsset", CapStatus::Served),
            ("UpdateAvatarAppearance", CapStatus::Served),
            ("IncrementCOFVersion", CapStatus::Served),
            ("NewFileAgentInventory", CapStatus::Served),
            ("UploadBakedTexture", CapStatus::Served),
            ("UpdateGestureAgentInventory", CapStatus::Served),
            ("UpdateNotecardAgentInventory", CapStatus::Served),
            ("UpdateNotecardTaskInventory", CapStatus::Served),
            ("CopyInventoryFromNotecard", CapStatus::Served),
            ("UpdateScriptAgent", CapStatus::Served),
            ("UpdateScriptTask", CapStatus::Served),
            ("UpdateSettingsAgentInventory", CapStatus::Served),
            ("UpdateSettingsTaskInventory", CapStatus::Served),
            // `ObjectAnimation` is never POSTed — it opts into the UDP
            // `ObjectAnimation` stream and has no HTTP handler, so it stays
            // `Pending` (see `CAP_OBJECT_ANIMATION`).
            ("ObjectAnimation", CapStatus::Pending),
            ("ObjectMedia", CapStatus::Served),
            ("ObjectMediaNavigate", CapStatus::Served),
            ("RenderMaterials", CapStatus::Served),
            ("ModifyMaterialParams", CapStatus::Served),
            ("UpdateMaterialAgentInventory", CapStatus::Served),
            ("ProvisionVoiceAccountRequest", CapStatus::Served),
            ("ParcelVoiceInfoRequest", CapStatus::Served),
            ("VoiceSignalingRequest", CapStatus::Served),
            ("GetExperienceInfo", CapStatus::Served),
            ("FindExperienceByName", CapStatus::Served),
            ("GetExperiences", CapStatus::Served),
            ("AgentExperiences", CapStatus::Served),
            ("GetAdminExperiences", CapStatus::Served),
            ("GetCreatorExperiences", CapStatus::Served),
            ("GroupExperiences", CapStatus::Served),
            ("ExperiencePreferences", CapStatus::Served),
            ("IsExperienceAdmin", CapStatus::Served),
            ("IsExperienceContributor", CapStatus::Served),
            ("UpdateExperience", CapStatus::Served),
            ("RegionExperiences", CapStatus::Served),
            ("ExperienceQuery", CapStatus::Served),
            ("ReadOfflineMsgs", CapStatus::Served),
            ("ChatSessionRequest", CapStatus::Served),
            ("AcceptGroupInvite", CapStatus::Served),
            ("DeclineGroupInvite", CapStatus::Served),
            ("AcceptFriendship", CapStatus::Served),
            ("DeclineFriendship", CapStatus::Served),
            ("GroupAPIv1", CapStatus::Served),
            ("AgentProfile", CapStatus::Pending),
            ("UploadAgentProfileImage", CapStatus::Served),
            ("SetDisplayName", CapStatus::Served),
            ("SearchStatRequest", CapStatus::Served),
            ("ProductInfoRequest", CapStatus::Served),
            ("InventoryAPIv3", CapStatus::Served),
            ("LibraryAPIv3", CapStatus::Served),
            ("CreateInventoryCategory", CapStatus::Served),
            ("ExtEnvironment", CapStatus::Served),
            ("GetDisplayNames", CapStatus::Served),
            ("AvatarPickerSearch", CapStatus::Served),
            ("RemoteParcelRequest", CapStatus::Served),
            ("ParcelPropertiesUpdate", CapStatus::Served),
            ("SimulatorFeatures", CapStatus::Served),
            ("LSLSyntax", CapStatus::Served),
            ("AgentPreferences", CapStatus::Served),
            ("UserInfo", CapStatus::Pending),
            ("GetObjectCost", CapStatus::Served),
            ("ResourceCostSelected", CapStatus::Served),
            ("GetObjectPhysicsData", CapStatus::Served),
            ("AttachmentResources", CapStatus::Served),
            ("AvatarRenderInfo", CapStatus::Served),
            ("LandResources", CapStatus::Served),
            ("SendUserReport", CapStatus::Served),
            ("SendUserReportWithScreenshot", CapStatus::Served),
            ("DirectDelivery", CapStatus::Served),
            ("InterestList", CapStatus::Served),
            ("UpdateGestureTaskInventory", CapStatus::Served),
            ("UpdateMaterialTaskInventory", CapStatus::Served),
            ("RequestTaskInventory", CapStatus::Served),
            ("GetMetadata", CapStatus::Served),
            ("MeshUploadFlag", CapStatus::Served),
            ("InventoryThumbnailUpload", CapStatus::Served),
            ("SendPostcard", CapStatus::Served),
            ("EstateAccess", CapStatus::Served),
            ("DispatchRegionInfo", CapStatus::Served),
            ("RegionSchedule", CapStatus::Served),
            ("ModifyRegion", CapStatus::Served),
            ("HomeLocation", CapStatus::Served),
            ("UntrustedSimulatorMessage", CapStatus::Served),
        ];
        let actual: Vec<(&str, CapStatus)> = REQUESTED_CAPABILITIES
            .iter()
            .map(|name| {
                // A capability is served if either the sim-cap registry
                // ([`SimCaps::handler_for`]) or the asset-cap registry
                // ([`AssetCaps::handler_for`]) handles it — the asset caps
                // live on a separate, session-free surface.
                if SimCaps::handler_for(name).is_some()
                    || crate::asset_caps::AssetCaps::handler_for(name).is_some()
                {
                    (*name, CapStatus::Served)
                } else {
                    (*name, CapStatus::Pending)
                }
            })
            .collect();
        assert_eq!(
            actual, expected,
            "a capability's server-side dispatch status changed — if \
             intended, bless it by editing this table"
        );
    }

    /// Every served capability must be one the client actually requests —
    /// a typo'd registry entry would otherwise silently never match a grant
    /// — and every listed name must have a dispatch handler, so
    /// [`SERVED_CAPABILITIES`] and [`SimCaps::handler_for`] cannot drift
    /// apart.
    #[test]
    fn every_served_capability_is_requested_and_handled() {
        for name in SERVED_CAPABILITIES {
            assert!(
                REQUESTED_CAPABILITIES.contains(name),
                "registry key {name:?} is not in REQUESTED_CAPABILITIES"
            );
            assert!(
                SimCaps::handler_for(name).is_some(),
                "served capability {name:?} has no dispatch handler"
            );
        }
    }

    /// The seed URL keeps a base URL's path prefix and appends
    /// `cap/<token>` — the `Url::join` trailing-slash trap must not bite.
    #[test]
    fn cap_urls_keep_the_base_path_prefix() -> Result<(), TestError> {
        let base: Url = "http://127.0.0.1:9001/region/east".parse()?;
        let caps = SimCaps::new(base, Uuid::from_u128(1), || Uuid::from_u128(2));
        assert_eq!(
            caps.seed_url().as_str(),
            "http://127.0.0.1:9001/region/east/cap/00000000-0000-0000-0000-000000000001"
        );
        Ok(())
    }

    /// `grant` serves only registered capabilities (sim caps here, plus the
    /// asset caps the composed surface advertises) and is a pure read:
    /// repeated grants return identical maps. `SimCaps::supports` reports only
    /// the sim caps — the asset caps live on the separate
    /// [`SimCaps::assets`] surface.
    #[test]
    fn grant_omits_unsupported_and_is_stable() -> Result<(), TestError> {
        let caps = caps()?;
        let requested = vec![
            "EventQueueGet".to_owned(),
            "GetTexture".to_owned(),
            "NoSuchCap".to_owned(),
        ];
        let granted = caps.grant(&requested);
        // A sim cap and an asset cap are granted; the unknown name is omitted.
        assert_eq!(granted.len(), 2);
        assert!(granted.contains_key("EventQueueGet"));
        assert!(granted.contains_key("GetTexture"));
        assert!(!granted.contains_key("NoSuchCap"));
        assert_eq!(granted, caps.grant(&requested));
        // `supports` is the sim-cap registry; GetTexture is an asset cap.
        assert!(caps.supports("EventQueueGet"));
        assert!(!caps.supports("GetTexture"));
        assert!(caps.assets().supports("GetTexture"));
        Ok(())
    }

    /// Path resolution: the seed and granted tokens resolve, anything else
    /// (including a malformed token) does not; sub-paths after the token are
    /// tolerated.
    #[test]
    fn resolve_matches_seed_and_granted_tokens() -> Result<(), TestError> {
        let caps = caps()?;
        let seed_path = caps.seed_url().path().to_owned();
        assert_eq!(caps.resolve(&seed_path), ResolvedPath::Seed);
        let granted = caps.grant(&["EventQueueGet".to_owned()]);
        let eq_url: Url = granted
            .get("EventQueueGet")
            .ok_or("EventQueueGet not granted")?
            .parse()?;
        assert_eq!(
            caps.resolve(eq_url.path()),
            ResolvedPath::Capability("EventQueueGet")
        );
        let with_sub_path = format!("{}/extra/segments", eq_url.path());
        assert_eq!(
            caps.resolve(&with_sub_path),
            ResolvedPath::Capability("EventQueueGet")
        );
        assert_eq!(
            caps.resolve("/cap/00000000-0000-0000-0000-0000000000ff"),
            ResolvedPath::Unknown
        );
        assert_eq!(caps.resolve("/cap/not-a-uuid"), ResolvedPath::Unknown);
        assert_eq!(caps.resolve("/somewhere/else"), ResolvedPath::Unknown);
        Ok(())
    }

    /// A withheld capability disappears from **both** halves of the contract:
    /// the grant a client reads to learn the URL, and the URL itself. Leaving
    /// the second behind would let a client that guessed the token keep using a
    /// protocol the grid says it does not speak.
    #[test]
    fn a_withheld_capability_is_neither_granted_nor_resolvable() -> Result<(), TestError> {
        let mut caps = caps()?;
        let name = CAP_UPDATE_AVATAR_APPEARANCE.to_owned();
        let url: Url = caps
            .grant(std::slice::from_ref(&name))
            .get(&name)
            .ok_or("UpdateAvatarAppearance was not granted before withholding")?
            .parse()?;

        assert!(caps.withhold(CAP_UPDATE_AVATAR_APPEARANCE));
        assert!(!caps.supports(CAP_UPDATE_AVATAR_APPEARANCE));
        assert!(
            caps.grant(std::slice::from_ref(&name)).is_empty(),
            "a withheld capability is still advertised"
        );
        assert_eq!(caps.resolve(url.path()), ResolvedPath::Unknown);
        // Withholding twice is not an error, it is a no-op that answers so.
        assert!(!caps.withhold(CAP_UPDATE_AVATAR_APPEARANCE));
        // Everything else is untouched.
        assert!(caps.supports(EVENT_QUEUE_GET));
        Ok(())
    }
}
