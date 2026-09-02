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

use std::collections::{BTreeMap, HashMap};

use sl_types::key::AgentKey;
use sl_wire::{
    AisUpdate, AssetUploadResponse, DisplayName, ExperiencePermission, LandResourcesUrls, Llsd,
    ObjectMediaRequest, ObjectMediaResponse, build_agent_preferences_response,
    build_asset_upload_response, build_attachment_resources_response,
    build_avatar_picker_search_response, build_create_inventory_category_response,
    build_display_names_response, build_experience_ids_response, build_experience_infos_response,
    build_experience_permissions_response, build_experience_status_response,
    build_get_object_cost_response, build_get_object_physics_data_response,
    build_land_resource_detail_response, build_land_resource_summary_response,
    build_land_resources_response, build_lsl_syntax_document,
    build_modify_material_params_response, build_parcel_voice_info_response,
    build_provision_voice_account_response, build_region_experiences_response,
    build_remote_parcel_response, build_render_materials_response,
    build_resource_cost_selected_response, build_seed_response, build_simulator_features_response,
    is_ais_current_outfit_links_url, is_ais_orphans_url, parse_agent_preferences,
    parse_ais_category_children_fetch_url, parse_ais_category_children_url,
    parse_ais_category_links_url, parse_ais_category_url, parse_ais_create_category_body,
    parse_ais_create_category_url, parse_ais_create_link_body, parse_ais_item_url,
    parse_ais_move_body, parse_ais_rename_category_body, parse_ais_update_item_body,
    parse_avatar_picker_search_query, parse_create_inventory_category_request,
    parse_display_names_query, parse_event_queue_request, parse_experience_id_query,
    parse_experience_info_query, parse_fetch_inventory_items_request,
    parse_fetch_inventory_request, parse_find_experience_query, parse_forget_experience_query,
    parse_get_object_cost_request, parse_get_object_physics_data_request,
    parse_group_experiences_query, parse_land_resources_request, parse_llsd_xml,
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
    ais_inventory_update_to_llsd, ais_item_reply_to_llsd, ais_mutation_reply_to_llsd,
    chat_session_agent_params_from_llsd, chat_session_request_from_llsd,
    chat_session_roster_to_llsd, environment_to_llsd, environment_update_from_llsd,
    fetch_inventory_items_to_llsd, inventory_descendents_to_llsd,
    parse_copy_inventory_from_notecard, server_appearance_update_to_llsd, session_history_to_llsd,
};
use crate::sim_inventory::SimInventoryError;
use crate::sim_session::{CapsUploadMetadata, SimSession};
use crate::{
    CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES, CAP_ATTACHMENT_RESOURCES,
    CAP_AVATAR_PICKER_SEARCH, CAP_CHAT_SESSION_REQUEST, CAP_COPY_INVENTORY_FROM_NOTECARD,
    CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES, CAP_EXT_ENVIRONMENT,
    CAP_FETCH_INVENTORY, CAP_FETCH_INVENTORY_ITEM, CAP_FETCH_LIBRARY, CAP_FETCH_LIBRARY_ITEM,
    CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES,
    CAP_GET_DISPLAY_NAMES, CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_OBJECT_COST,
    CAP_GET_OBJECT_PHYSICS_DATA, CAP_GROUP_EXPERIENCES, CAP_INVENTORY_API_V3,
    CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_LAND_RESOURCES, CAP_LIBRARY_API_V3,
    CAP_LSL_SYNTAX, CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT,
    CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS,
    CAP_RESOURCE_COST_SELECTED, CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    CAP_SIMULATOR_FEATURES, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY, CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY, CAP_UPDATE_NOTECARD_TASK_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK, CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE, CAP_VOICE_SIGNALING, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE,
    CHAT_SESSION_DECLINE_P2P_VOICE, CHAT_SESSION_FETCH_HISTORY, CHAT_SESSION_INVITE,
    CHAT_SESSION_START_CONFERENCE, Event, InventoryFolder, InventoryItem, ServerEvent,
    VoiceProvisionRefusal, offline_messages_to_llsd,
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
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
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
    CAP_GET_DISPLAY_NAMES,
    CAP_AVATAR_PICKER_SEARCH,
    CAP_AGENT_PREFERENCES,
    CAP_SEND_USER_REPORT,
    CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    // The content upload/update, materials and MOAP cluster.
    CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_TASK_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SCRIPT_TASK,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_AVATAR_APPEARANCE,
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
    CAP_GET_OBJECT_COST,
    CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_RESOURCE_COST_SELECTED,
    CAP_ATTACHMENT_RESOURCES,
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
    /// The `AvatarPickerSearch` name search over [`SimSession`]'s display-name
    /// store — the modern replacement for the UDP `AvatarPickerRequest`.
    AvatarPickerSearch,
    /// The `GetDisplayNames` people-service lookup, served from
    /// [`SimSession`]'s display-name store ([`SimSession::display_name`]).
    DisplayNames,
    /// The `AgentPreferences` merge-and-echo of the agent's server-stored
    /// preferences ([`SimSession::agent_preferences`]).
    AgentPreferences,
    /// The one-step `SendUserReport` abuse-report POST.
    UserReport,
    /// The two-step `SendUserReportWithScreenshot` uploader: the report POST
    /// answers with an uploader URL (a sub-path of the cap's own URL), the
    /// raw screenshot bytes complete it.
    UserReportScreenshot,
    /// The shared two-stage asset-upload state machine serving every
    /// `UPLOAD_CAPABILITIES` entry (`NewFileAgentInventory`,
    /// `UploadBakedTexture`, and the `Update*{Agent,Task}Inventory` family):
    /// step 1 parks the parsed metadata and answers an uploader URL, step 2
    /// (the raw-bytes POST) completes it into
    /// [`ServerEvent::CapsAssetUploaded`](crate::ServerEvent::CapsAssetUploaded).
    AssetUpload,
    /// The single-POST `UpdateAvatarAppearance` server-side-bake trigger.
    AvatarAppearance,
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
    /// `bytes 0-1023/4096`), set on the asset-delivery caps' `206` and `416`
    /// responses and `None` otherwise.
    pub content_range: Option<String>,
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
            body: body.into_bytes(),
        }
    }

    /// A body-less response with the given status code.
    const fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: TEXT_PLAIN_CONTENT_TYPE,
            content_range: None,
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
            body,
        }
    }

    /// `416 Range Not Satisfiable` with `Content-Range: bytes */{total}` — a
    /// range whose start is past the end of an *existing* asset. HTTP-correct;
    /// the client turns it into an empty chunk and stops. (OpenSim instead
    /// serves the whole asset here to dodge a reference-viewer 416 bug; our
    /// client handles 416 cleanly, so we stay spec-correct.)
    pub(crate) fn range_not_satisfiable(total: usize) -> Self {
        Self {
            status: 416,
            content_type: TEXT_PLAIN_CONTENT_TYPE,
            content_range: Some(format!("bytes */{total}")),
            body: Vec::new(),
        }
    }
}

/// The outcome of dispatching one CAPS request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsDispatch {
    /// Respond immediately.
    Response(CapsResponse),
    /// An `EventQueueGet` poll with nothing queued: hold the request open.
    /// The runtime re-dispatches when [`SimSession::has_caps_events`] turns
    /// true, or answers [`SimCaps::event_queue_timeout`] (`502` — the
    /// "nothing yet, re-poll" signal the reference viewer expects) when its
    /// hold (~30 s) expires.
    EventQueueWouldBlock,
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
        }
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
            CAP_GET_DISPLAY_NAMES => Some(CapHandler::DisplayNames),
            CAP_AVATAR_PICKER_SEARCH => Some(CapHandler::AvatarPickerSearch),
            CAP_AGENT_PREFERENCES => Some(CapHandler::AgentPreferences),
            CAP_SEND_USER_REPORT => Some(CapHandler::UserReport),
            CAP_SEND_USER_REPORT_WITH_SCREENSHOT => Some(CapHandler::UserReportScreenshot),
            CAP_UPDATE_AVATAR_APPEARANCE => Some(CapHandler::AvatarAppearance),
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
            CAP_GET_OBJECT_COST => Some(CapHandler::ObjectCost),
            CAP_GET_OBJECT_PHYSICS_DATA => Some(CapHandler::ObjectPhysics),
            CAP_RESOURCE_COST_SELECTED => Some(CapHandler::ResourceCostSelected),
            CAP_ATTACHMENT_RESOURCES => Some(CapHandler::AttachmentResources),
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
            ResolvedPath::Capability(name) => match Self::handler_for(name) {
                Some(CapHandler::EventQueue) => self.dispatch_event_queue(sim, request),
                Some(CapHandler::ChatSession) => {
                    CapsDispatch::Response(Self::dispatch_chat_session(sim, request))
                }
                Some(CapHandler::OfflineMessages) => {
                    CapsDispatch::Response(Self::dispatch_offline_messages(sim, request))
                }
                Some(CapHandler::DisplayNames) => {
                    CapsDispatch::Response(Self::dispatch_display_names(sim, request))
                }
                Some(CapHandler::AvatarPickerSearch) => {
                    CapsDispatch::Response(Self::dispatch_avatar_picker_search(sim, request))
                }
                Some(CapHandler::AgentPreferences) => {
                    CapsDispatch::Response(Self::dispatch_agent_preferences(sim, request))
                }
                Some(CapHandler::UserReport) => {
                    CapsDispatch::Response(Self::dispatch_user_report(sim, request))
                }
                Some(CapHandler::UserReportScreenshot) => {
                    CapsDispatch::Response(self.dispatch_user_report_screenshot(sim, request))
                }
                Some(CapHandler::AssetUpload) => {
                    CapsDispatch::Response(self.dispatch_caps_upload(sim, request, name))
                }
                Some(CapHandler::AvatarAppearance) => {
                    CapsDispatch::Response(Self::dispatch_update_avatar_appearance(sim, request))
                }
                Some(CapHandler::CopyInventoryFromNotecard) => CapsDispatch::Response(
                    Self::dispatch_copy_inventory_from_notecard(sim, request),
                ),
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
                Some(CapHandler::CreateInventoryCategory) => {
                    CapsDispatch::Response(Self::dispatch_create_inventory_category(sim, request))
                }
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
                Some(CapHandler::ExperienceIdList) => {
                    CapsDispatch::Response(Self::dispatch_experience_id_list(sim, request, name))
                }
                Some(CapHandler::ExperienceStatus) => {
                    CapsDispatch::Response(Self::dispatch_experience_status(sim, request, name))
                }
                Some(CapHandler::UpdateExperience) => {
                    CapsDispatch::Response(Self::dispatch_update_experience(sim, request))
                }
                Some(CapHandler::RegionExperiences) => {
                    CapsDispatch::Response(Self::dispatch_region_experiences(sim, request))
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
            },
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
        let messages = sim.take_offline_messages();
        CapsResponse::llsd_xml(offline_messages_to_llsd(&messages).to_llsd_xml())
    }

    /// Serves one `GetDisplayNames` GET: looks each `ids` query parameter up
    /// in the session's display-name store. Known agents answer as full
    /// `agents` records, unknown ids as `bad_ids` (the grid's "could not
    /// resolve" form). No ids — or no query at all — answers an empty
    /// `agents` array (tolerant).
    fn dispatch_display_names(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let query = request.query.unwrap_or_default();
        let ids = parse_display_names_query(&format!("?{query}"));
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
            .collect::<Vec<DisplayName>>();
        CapsResponse::llsd_xml(build_display_names_response(&records))
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
    /// cap-specific metadata, parks it under `cap_name`, and answers
    /// `{ state: "upload", uploader }` with the cap's own `upload` sub-path.
    /// Step 2 (a POST to that sub-path) takes the parked metadata, has the
    /// session mint the stored ids and push
    /// [`ServerEvent::CapsAssetUploaded`](crate::ServerEvent::CapsAssetUploaded),
    /// and answers `{ state: "complete", new_asset, new_inventory_item? }` —
    /// plus `{ compiled, errors }` for a script upload. A bytes-POST with no
    /// parked upload answers `400`; a re-POST of step 1 replaces the parked
    /// metadata; any other sub-path answers `404`. Wrong method → `405`, an
    /// unparsable metadata body → `400`.
    fn dispatch_caps_upload(
        &self,
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
        cap_name: &'static str,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        match cap_sub_path(request.path) {
            None => {
                let Some(metadata) = Self::parse_upload_metadata(cap_name, request.body) else {
                    return CapsResponse::bad_request();
                };
                sim.park_caps_upload(cap_name, metadata);
                let uploader = self.upload_uploader_url(cap_name);
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "upload".to_owned(),
                    uploader: Some(uploader.to_string()),
                    ..AssetUploadResponse::default()
                }))
            }
            Some(UPLOAD_SUB_PATH) => {
                let Some(metadata) = sim.take_caps_upload(cap_name) else {
                    return CapsResponse::bad_request();
                };
                let is_script = metadata.is_script();
                let (new_asset, new_inventory_item) =
                    sim.complete_caps_upload(metadata, request.body.to_vec());
                CapsResponse::llsd_xml(build_asset_upload_response(&AssetUploadResponse {
                    state: "complete".to_owned(),
                    new_asset: Some(new_asset.uuid()),
                    new_inventory_item: new_inventory_item.map(|item| item.uuid()),
                    // A script upload reports the compile result; the sim server
                    // "compiles" cleanly (a real grid would run the compiler).
                    compiled: is_script.then_some(true),
                    ..AssetUploadResponse::default()
                }))
            }
            Some(_) => CapsResponse::not_found(),
        }
    }

    /// Parses the step-1 metadata of a two-stage upload for `cap_name` into the
    /// parked [`CapsUploadMetadata`], or `None` (→ `400`) when the body is not
    /// UTF-8 or not well-formed for the cap. The four agent-inventory `Update*`
    /// caps (gesture / notecard / settings / material) share the bare
    /// `{ item_id }` body and fall through to the catch-all arm.
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
            CAP_UPDATE_NOTECARD_TASK_INVENTORY => {
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
    /// URL plus the shared [`UPLOAD_SUB_PATH`] (routed back to
    /// [`SimCaps::dispatch_caps_upload`]).
    fn upload_uploader_url(&self, cap_name: &str) -> Url {
        let token = self.tokens.get(cap_name).copied().unwrap_or_default();
        let mut url = self.cap_url(token);
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.push(UPLOAD_SUB_PATH);
        }
        url
    }

    /// Serves one `UpdateAvatarAppearance` POST: parses the requested Current
    /// Outfit Folder version, surfaces it as
    /// [`ServerEvent::ServerAppearanceRequested`](crate::ServerEvent::ServerAppearanceRequested),
    /// and answers the accept reply `{ success: true }` (the baked-texture ids
    /// arrive separately over UDP `AvatarAppearance`). Wrong method → `405`, a
    /// non-LLSD body → `400`.
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
        sim.push_content_event(ServerEvent::ServerAppearanceRequested { cof_version });
        let reply = Event::ServerAppearanceUpdate {
            success: true,
            error: None,
            expected_cof_version: None,
        };
        CapsResponse::llsd_xml(server_appearance_update_to_llsd(&reply).to_llsd_xml())
    }

    /// Serves one `CopyInventoryFromNotecard` POST: surfaces the copy request
    /// as
    /// [`ServerEvent::CopyInventoryFromNotecardRequested`](crate::ServerEvent::CopyInventoryFromNotecardRequested)
    /// and acks with an undefined body (the copied item is delivered over the
    /// normal inventory-update stream, so there is no reply payload). Wrong
    /// method → `405`, a non-LLSD body → `400`.
    fn dispatch_copy_inventory_from_notecard(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        if request.method != "POST" {
            return CapsResponse::method_not_allowed();
        }
        let Some(body) = parse_llsd_body(request.body) else {
            return CapsResponse::bad_request();
        };
        let copy = parse_copy_inventory_from_notecard(&body);
        sim.push_content_event(ServerEvent::CopyInventoryFromNotecardRequested {
            notecard_id: copy.notecard,
            object_id: copy.object,
            item_id: copy.item,
            folder_id: copy.folder,
        });
        CapsResponse::llsd_xml(UNDEF_LLSD_BODY.to_owned())
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
                let ids = parse_render_materials_request(text);
                CapsResponse::llsd_xml(build_render_materials_response(&sim.region_materials(&ids)))
            }
            "GET" => {
                CapsResponse::llsd_xml(build_render_materials_response(&sim.region_materials(&[])))
            }
            "PUT" => {
                let Ok(text) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let updates = parse_render_materials_put_request(text);
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
        let Some(media_request) = parse_object_media_request(&body) else {
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
        let Some(navigate) = parse_object_media_navigate_request(&body) else {
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
        let tree = if name == CAP_FETCH_LIBRARY {
            sim.library_inventory()
        } else {
            sim.agent_inventory()
        };
        let events: Vec<Event> = folders
            .iter()
            .filter_map(|folder| {
                tree.descendents(
                    folder.folder_id,
                    folder.fetch_folders,
                    folder.fetch_items,
                    folder.sort_order,
                )
            })
            .collect();
        match inventory_descendents_to_llsd(&events) {
            Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
            Err(_) => CapsResponse::internal_error(),
        }
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
        match fetch_inventory_items_to_llsd(agent_id, &items) {
            Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
            Err(_) => CapsResponse::internal_error(),
        }
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
        match request.method {
            "GET" => {
                let tree = if read_only {
                    sim.library_inventory()
                } else {
                    sim.agent_inventory()
                };
                // A children URL missing the `?depth=` query lists one level,
                // the builder's smallest useful fetch.
                if let Some((folder_id, depth)) = parse_ais_category_children_fetch_url(&suffix)
                    .or_else(|| parse_ais_category_children_url(&suffix).map(|id| (id, 1)))
                {
                    let Some(folder) = tree.folder(folder_id).cloned() else {
                        return CapsResponse::not_found();
                    };
                    let Some((folders, items)) = tree.children_to_depth(folder_id, depth) else {
                        return CapsResponse::not_found();
                    };
                    return match ais_category_children_reply_to_llsd(&folder, &folders, &items) {
                        Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
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
                if let Some(item_id) = parse_ais_item_url(&suffix) {
                    let Some(item) = tree.item(item_id) else {
                        return CapsResponse::not_found();
                    };
                    return match ais_item_reply_to_llsd(item) {
                        Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
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
                        Ok((update, items)) => Self::ais_reply(&update, &[], &items),
                        Err(error) => Self::inventory_error(error),
                    };
                }
                let Ok(create) = parse_ais_create_category_body(text) else {
                    return CapsResponse::bad_request();
                };
                match sim.ais_create_category(parent, &create) {
                    Ok((update, folder)) => Self::ais_reply(&update, &[folder], &[]),
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
                let is_move = body.get("parent_id").is_some();
                if let Some(folder_id) = parse_ais_category_url(&suffix) {
                    let result = if is_move {
                        match parse_ais_move_body(text) {
                            Ok(parent) => sim.ais_move_category(folder_id, parent),
                            Err(_) => return CapsResponse::bad_request(),
                        }
                    } else {
                        match parse_ais_rename_category_body(text) {
                            Ok(new_name) => sim.ais_rename_category(folder_id, new_name),
                            Err(_) => return CapsResponse::bad_request(),
                        }
                    };
                    return match result {
                        Ok(update) => {
                            let folders: Vec<InventoryFolder> = sim
                                .agent_inventory()
                                .folder(folder_id)
                                .cloned()
                                .into_iter()
                                .collect();
                            Self::ais_reply(&update, &folders, &[])
                        }
                        Err(error) => Self::inventory_error(error),
                    };
                }
                if let Some(item_id) = parse_ais_item_url(&suffix) {
                    let result = if is_move {
                        match parse_ais_move_body(text) {
                            Ok(parent) => sim.ais_move_item(item_id, parent),
                            Err(_) => return CapsResponse::bad_request(),
                        }
                    } else {
                        match parse_ais_update_item_body(text) {
                            Ok(update_fields) => sim.ais_update_item(item_id, &update_fields),
                            Err(_) => return CapsResponse::bad_request(),
                        }
                    };
                    return match result {
                        Ok(update) => {
                            let items: Vec<InventoryItem> = sim
                                .agent_inventory()
                                .item(item_id)
                                .cloned()
                                .into_iter()
                                .collect();
                            Self::ais_reply(&update, &[], &items)
                        }
                        Err(error) => Self::inventory_error(error),
                    };
                }
                CapsResponse::bad_request()
            }
            "DELETE" if !read_only => {
                if let Some(folder_id) = parse_ais_category_children_url(&suffix) {
                    let result = sim.ais_purge_category(folder_id);
                    return Self::ais_mutation_response(result);
                }
                if let Some(folder_id) = parse_ais_category_url(&suffix) {
                    let result = sim.ais_remove_category(folder_id);
                    return Self::ais_mutation_response(result);
                }
                if let Some(item_id) = parse_ais_item_url(&suffix) {
                    let result = sim.ais_remove_item(item_id);
                    return Self::ais_mutation_response(result);
                }
                CapsResponse::bad_request()
            }
            _ => CapsResponse::method_not_allowed(),
        }
    }

    /// Serializes an AIS3 mutation outcome that embeds nothing (the delete
    /// verbs): the change-set meta on success, the mapped error status
    /// otherwise.
    fn ais_mutation_response(result: Result<AisUpdate, SimInventoryError>) -> CapsResponse {
        match result {
            Ok(update) => Self::ais_reply(&update, &[], &[]),
            Err(error) => Self::inventory_error(error),
        }
    }

    /// Builds a `200` AIS3 reply from a change-set and the affected objects
    /// (embedded under `_embedded`), or `500` when the fixture holds an
    /// unserializable item (an out-of-range sale price).
    fn ais_reply(
        update: &AisUpdate,
        folders: &[InventoryFolder],
        items: &[InventoryItem],
    ) -> CapsResponse {
        match ais_mutation_reply_to_llsd(update, folders, items) {
            Ok(body) => CapsResponse::llsd_xml(body.to_llsd_xml()),
            Err(_) => CapsResponse::internal_error(),
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
    /// document ([`SimSession::set_lsl_syntax`]). Wrong method → `405`.
    fn dispatch_lsl_syntax(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        CapsResponse::llsd_xml(build_lsl_syntax_document(sim.lsl_syntax()))
    }

    /// Serves the `ExtEnvironment` EEP surface. GET answers the stored
    /// settings for the `?parcelid=` query (absent → `-1`, the region; an
    /// unset parcel inherits the region entry). PUT parses the update body,
    /// applies it via [`SimSession::apply_environment_update`], and echoes
    /// the stored result (the `{ environment, success: true }` envelope the
    /// reference viewer reads back); a `day_asset`-only update answers a
    /// graceful `200 { success: false, message }` — the fixture has no
    /// settings-asset store to resolve the id against. A malformed query or
    /// body → `400`; other methods (including the reference's DELETE reset,
    /// out of scope here) → `405`.
    fn dispatch_environment(sim: &mut SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        let Ok(parcel_id) = parse_parcel_id_query(request.query) else {
            return CapsResponse::bad_request();
        };
        match request.method {
            "GET" => CapsResponse::llsd_xml(
                environment_to_llsd(&sim.environment(parcel_id)).to_llsd_xml(),
            ),
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
                CapsResponse::llsd_xml(environment_to_llsd(&stored).to_llsd_xml())
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
    /// ([`SimSession::set_attachment_resources`]). Wrong method → `405`.
    fn dispatch_attachment_resources(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        CapsResponse::llsd_xml(build_attachment_resources_response(
            sim.attachment_resources(),
        ))
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
    /// `200` with no records — the parser is lenient by design (a
    /// documented exception to the `400`-on-malformed rule). Wrong
    /// method → `405`.
    fn dispatch_experience_info(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let ids = parse_experience_info_query(&ais_suffix(request));
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
    /// [`SimExperiences::find`]: crate::SimExperiences::find
    fn dispatch_experience_search(sim: &SimSession, request: &CapsRequest<'_>) -> CapsResponse {
        if request.method != "GET" {
            return CapsResponse::method_not_allowed();
        }
        let Some((text, page)) = parse_find_experience_query(&ais_suffix(request)) else {
            return CapsResponse::bad_request();
        };
        CapsResponse::llsd_xml(build_experience_infos_response(
            &sim.experiences().find(&text, page),
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
                let Ok(Some(parsed)) = parse_set_experience_permission_request(body) else {
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
    /// event). An unknown `public_id` → `404`; a malformed body → `400`;
    /// wrong method → `405`.
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
        match sim.apply_experience_update(update) {
            Some(updated) => CapsResponse::llsd_xml(build_experience_infos_response(&[updated])),
            None => CapsResponse::not_found(),
        }
    }

    /// Serves the `RegionExperiences` surface. GET answers the region's
    /// stored allowed / blocked / trusted lists; POST parses the
    /// same-shaped body, replaces the lists wholesale
    /// ([`SimSession::apply_region_experiences`]), and echoes the stored
    /// triple. A malformed POST body → `400`; other methods → `405`.
    fn dispatch_region_experiences(
        sim: &mut SimSession,
        request: &CapsRequest<'_>,
    ) -> CapsResponse {
        match request.method {
            "GET" => {
                let (allowed, blocked, trusted) = sim.experiences().region_lists();
                CapsResponse::llsd_xml(build_region_experiences_response(
                    &allowed, &blocked, &trusted,
                ))
            }
            "POST" => {
                let Ok(body) = std::str::from_utf8(request.body) else {
                    return CapsResponse::bad_request();
                };
                let Ok((allowed, blocked, trusted)) = parse_region_experiences_request(body) else {
                    return CapsResponse::bad_request();
                };
                let (allowed, blocked, trusted) =
                    sim.apply_region_experiences(allowed, blocked, trusted);
                CapsResponse::llsd_xml(build_region_experiences_response(
                    &allowed, &blocked, &trusted,
                ))
            }
            _ => CapsResponse::method_not_allowed(),
        }
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
            ("GroupMemberData", CapStatus::Pending),
            ("GetTexture", CapStatus::Served),
            ("GetMesh", CapStatus::Served),
            ("GetMesh2", CapStatus::Served),
            ("ViewerAsset", CapStatus::Served),
            ("UpdateAvatarAppearance", CapStatus::Served),
            ("NewFileAgentInventory", CapStatus::Served),
            ("UploadBakedTexture", CapStatus::Served),
            ("UpdateGestureAgentInventory", CapStatus::Served),
            ("UpdateNotecardAgentInventory", CapStatus::Served),
            ("UpdateNotecardTaskInventory", CapStatus::Served),
            ("CopyInventoryFromNotecard", CapStatus::Served),
            ("UpdateScriptAgent", CapStatus::Served),
            ("UpdateScriptTask", CapStatus::Served),
            ("UpdateSettingsAgentInventory", CapStatus::Served),
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
            ("ReadOfflineMsgs", CapStatus::Served),
            ("ChatSessionRequest", CapStatus::Served),
            ("AcceptGroupInvite", CapStatus::Pending),
            ("DeclineGroupInvite", CapStatus::Pending),
            ("InventoryAPIv3", CapStatus::Served),
            ("LibraryAPIv3", CapStatus::Served),
            ("CreateInventoryCategory", CapStatus::Served),
            ("ExtEnvironment", CapStatus::Served),
            ("GetDisplayNames", CapStatus::Served),
            ("AvatarPickerSearch", CapStatus::Served),
            ("RemoteParcelRequest", CapStatus::Served),
            ("SimulatorFeatures", CapStatus::Served),
            ("LSLSyntax", CapStatus::Served),
            ("AgentPreferences", CapStatus::Served),
            ("UserInfo", CapStatus::Pending),
            ("GetObjectCost", CapStatus::Served),
            ("ResourceCostSelected", CapStatus::Served),
            ("GetObjectPhysicsData", CapStatus::Served),
            ("AttachmentResources", CapStatus::Served),
            ("LandResources", CapStatus::Served),
            ("SendUserReport", CapStatus::Served),
            ("SendUserReportWithScreenshot", CapStatus::Served),
            ("DirectDelivery", CapStatus::Pending),
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
}
