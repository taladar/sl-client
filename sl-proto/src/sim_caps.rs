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
    AssetUploadResponse, DisplayName, Llsd, ObjectMediaRequest, ObjectMediaResponse,
    build_agent_preferences_response, build_asset_upload_response, build_display_names_response,
    build_modify_material_params_response, build_render_materials_response, build_seed_response,
    parse_agent_preferences, parse_display_names_query, parse_event_queue_request, parse_llsd_xml,
    parse_modify_material_params_request, parse_new_file_agent_inventory_request,
    parse_object_media_navigate_request, parse_object_media_request,
    parse_render_materials_put_request, parse_render_materials_request, parse_seed_request,
    parse_send_user_report, parse_update_avatar_appearance_request,
    parse_update_item_asset_request, parse_update_script_agent_request,
    parse_update_script_task_request, parse_update_task_item_asset_request,
};
use url::Url;
use uuid::Uuid;

use crate::asset_caps::AssetCaps;
use crate::bookkeeping_ids::ImSessionId;
use crate::session::{
    chat_session_request_from_llsd, chat_session_roster_to_llsd,
    parse_copy_inventory_from_notecard, server_appearance_update_to_llsd, session_history_to_llsd,
};
use crate::sim_session::{CapsUploadMetadata, SimSession};
use crate::{
    CAP_AGENT_PREFERENCES, CAP_CHAT_SESSION_REQUEST, CAP_COPY_INVENTORY_FROM_NOTECARD,
    CAP_GET_DISPLAY_NAMES, CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE, CAP_READ_OFFLINE_MSGS, CAP_RENDER_MATERIALS,
    CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT, CAP_UPDATE_AVATAR_APPEARANCE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY, CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY, CAP_UPDATE_NOTECARD_TASK_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK, CAP_UPDATE_SETTINGS_AGENT_INVENTORY,
    CAP_UPLOAD_BAKED_TEXTURE, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE,
    CHAT_SESSION_DECLINE_P2P_VOICE, CHAT_SESSION_FETCH_HISTORY, Event, ServerEvent,
    offline_messages_to_llsd,
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
    /// The `ChatSessionRequest` chat-session lifecycle (accept / decline /
    /// decline p2p voice / fetch history), served from [`SimSession`]'s
    /// chat-session registry.
    ChatSession,
    /// The deliver-once `ReadOfflineMsgs` fetch of messages stored while the
    /// agent was offline ([`SimSession::take_offline_messages`]).
    OfflineMessages,
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

    /// `502 Bad Gateway` — the "nothing yet, re-poll" answer to a held
    /// event-queue poll whose hold expired.
    const fn bad_gateway() -> Self {
        Self::empty(502)
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
            CAP_AGENT_PREFERENCES => Some(CapHandler::AgentPreferences),
            CAP_SEND_USER_REPORT => Some(CapHandler::UserReport),
            CAP_SEND_USER_REPORT_WITH_SCREENSHOT => Some(CapHandler::UserReportScreenshot),
            CAP_UPDATE_AVATAR_APPEARANCE => Some(CapHandler::AvatarAppearance),
            CAP_COPY_INVENTORY_FROM_NOTECARD => Some(CapHandler::CopyInventoryFromNotecard),
            CAP_RENDER_MATERIALS => Some(CapHandler::RenderMaterials),
            CAP_MODIFY_MATERIAL_PARAMS => Some(CapHandler::ModifyMaterialParams),
            CAP_OBJECT_MEDIA => Some(CapHandler::ObjectMedia),
            CAP_OBJECT_MEDIA_NAVIGATE => Some(CapHandler::ObjectMediaNavigate),
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
    /// member. `"accept invitation"` answers the session's roster (an empty
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
            ("FetchInventoryDescendents2", CapStatus::Pending),
            ("FetchLibDescendents2", CapStatus::Pending),
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
            ("ProvisionVoiceAccountRequest", CapStatus::Pending),
            ("ParcelVoiceInfoRequest", CapStatus::Pending),
            ("VoiceSignalingRequest", CapStatus::Pending),
            ("GetExperienceInfo", CapStatus::Pending),
            ("FindExperienceByName", CapStatus::Pending),
            ("GetExperiences", CapStatus::Pending),
            ("AgentExperiences", CapStatus::Pending),
            ("GetAdminExperiences", CapStatus::Pending),
            ("GetCreatorExperiences", CapStatus::Pending),
            ("GroupExperiences", CapStatus::Pending),
            ("ExperiencePreferences", CapStatus::Pending),
            ("IsExperienceAdmin", CapStatus::Pending),
            ("IsExperienceContributor", CapStatus::Pending),
            ("UpdateExperience", CapStatus::Pending),
            ("RegionExperiences", CapStatus::Pending),
            ("ReadOfflineMsgs", CapStatus::Served),
            ("ChatSessionRequest", CapStatus::Served),
            ("AcceptGroupInvite", CapStatus::Pending),
            ("DeclineGroupInvite", CapStatus::Pending),
            ("InventoryAPIv3", CapStatus::Pending),
            ("LibraryAPIv3", CapStatus::Pending),
            ("CreateInventoryCategory", CapStatus::Pending),
            ("ExtEnvironment", CapStatus::Pending),
            ("GetDisplayNames", CapStatus::Served),
            ("RemoteParcelRequest", CapStatus::Pending),
            ("SimulatorFeatures", CapStatus::Pending),
            ("LSLSyntax", CapStatus::Pending),
            ("AgentPreferences", CapStatus::Served),
            ("GetObjectCost", CapStatus::Pending),
            ("ResourceCostSelected", CapStatus::Pending),
            ("GetObjectPhysicsData", CapStatus::Pending),
            ("AttachmentResources", CapStatus::Pending),
            ("LandResources", CapStatus::Pending),
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
