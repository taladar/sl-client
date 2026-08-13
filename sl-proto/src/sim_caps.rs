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

use sl_wire::{build_seed_response, parse_event_queue_request, parse_seed_request};
use url::Url;
use uuid::Uuid;

use crate::sim_session::SimSession;

/// The LLSD-XML media type CAPS bodies use.
pub const LLSD_XML_CONTENT_TYPE: &str = "application/llsd+xml";

/// The media type of body-less error responses.
const TEXT_PLAIN_CONTENT_TYPE: &str = "text/plain";

/// The capability-name key of the event-queue long-poll.
const EVENT_QUEUE_GET: &str = "EventQueueGet";

/// The capability names this simulator can serve — the registry keys.
///
/// [`SimCaps::handler_for`] maps each entry to its [`CapHandler`]; the
/// `protocol-sim-caps-*` cluster tasks grow both together (plus the pinned
/// coverage table below, which tracks this list against
/// [`REQUESTED_CAPABILITIES`](crate::REQUESTED_CAPABILITIES)).
const SERVED_CAPABILITIES: &[&str] = &[EVENT_QUEUE_GET];

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
}

/// A transport-agnostic CAPS HTTP request, borrowed from the server glue.
///
/// Only `method`, `path` and `body` are consumed today; `query` (and the
/// sub-path below a capability URL) are carried so the asset (HTTP `Range`,
/// 206) and AIS inventory (sub-path routing) cluster tasks extend this type
/// instead of redesigning it — more fields may grow here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapsRequest<'a> {
    /// The HTTP method, uppercase (`"GET"`, `"POST"`, …).
    pub method: &'a str,
    /// The absolute URL path (e.g. `/cap/<uuid>` plus any sub-path).
    pub path: &'a str,
    /// The raw query string without the `?`, if any.
    pub query: Option<&'a str>,
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
    /// The response body.
    pub body: Vec<u8>,
}

impl CapsResponse {
    /// A `200 OK` response carrying an LLSD-XML body.
    const fn llsd_xml(body: String) -> Self {
        Self {
            status: 200,
            content_type: LLSD_XML_CONTENT_TYPE,
            body: body.into_bytes(),
        }
    }

    /// A body-less response with the given status code.
    const fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: TEXT_PLAIN_CONTENT_TYPE,
            body: Vec::new(),
        }
    }

    /// `404 Not Found` — an unknown capability URL, or an event-queue poll
    /// after teardown (the client's "stop polling" signal).
    const fn not_found() -> Self {
        Self::empty(404)
    }

    /// `405 Method Not Allowed` — a known capability URL hit with the wrong
    /// HTTP method.
    const fn method_not_allowed() -> Self {
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
    /// Set once the client has polled with `done`; later polls answer `404`.
    event_queue_done: bool,
}

impl SimCaps {
    /// Creates the CAPS surface for one agent's presence on a region.
    ///
    /// `base_url` is the public HTTP(S) base the capability URLs are minted
    /// under; `seed_token` is the seed capability's own URL token (the login
    /// side embeds the resulting [`SimCaps::seed_url`] in its login
    /// response); `mint_token` supplies the randomness for the per-capability
    /// tokens — sans-I/O purity means the caller owns randomness (a runtime
    /// passes `Uuid::new_v4`, tests pass a deterministic counter).
    pub fn new(base_url: Url, seed_token: Uuid, mut mint_token: impl FnMut() -> Uuid) -> Self {
        let tokens = SERVED_CAPABILITIES
            .iter()
            .map(|name| (*name, mint_token()))
            .collect::<BTreeMap<&'static str, Uuid>>();
        Self {
            base_url,
            seed_token,
            tokens,
            event_queue_done: false,
        }
    }

    /// The handler for a served capability name — the dispatch registry.
    ///
    /// Every [`SERVED_CAPABILITIES`] entry maps to a [`CapHandler`] variant
    /// here; the `protocol-sim-caps-*` cluster tasks grow both together.
    fn handler_for(name: &str) -> Option<CapHandler> {
        match name {
            EVENT_QUEUE_GET => Some(CapHandler::EventQueue),
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
    /// the seed round-trip. Unsupported names are silently omitted (the
    /// protocol's feature negotiation); requested order is irrelevant since
    /// the response is a map. Pure and stable: equal requests yield equal
    /// grants, which [`build_seed_response`] serializes byte-identically.
    #[must_use]
    pub fn grant(&self, requested: &[String]) -> HashMap<String, String> {
        requested
            .iter()
            .filter_map(|name| {
                self.tokens
                    .get(name.as_str())
                    .map(|token| (name.clone(), self.cap_url(*token).to_string()))
            })
            .collect()
    }

    /// Routes one CAPS request to its handler.
    ///
    /// Outcomes: an unknown URL answers `404`; the seed URL answers the
    /// grant (`POST` only); the `EventQueueGet` URL implements the long-poll
    /// contract — events now (`200 { id, events }`), nothing queued
    /// ([`CapsDispatch::EventQueueWouldBlock`]; the runtime holds and
    /// eventually answers [`SimCaps::event_queue_timeout`]), `done=true`
    /// teardown (`200`, then `404` for every later poll), and `404` once the
    /// session is closed.
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
            return CapsDispatch::Response(CapsResponse::llsd_xml(
                "<llsd><undef /></llsd>".to_owned(),
            ));
        }
        match sim.take_event_queue_response() {
            Some(xml) => CapsDispatch::Response(CapsResponse::llsd_xml(xml)),
            None => CapsDispatch::EventQueueWouldBlock,
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
            ("GetTexture", CapStatus::Pending),
            ("GetMesh", CapStatus::Pending),
            ("GetMesh2", CapStatus::Pending),
            ("ViewerAsset", CapStatus::Pending),
            ("UpdateAvatarAppearance", CapStatus::Pending),
            ("NewFileAgentInventory", CapStatus::Pending),
            ("UploadBakedTexture", CapStatus::Pending),
            ("UpdateGestureAgentInventory", CapStatus::Pending),
            ("UpdateNotecardAgentInventory", CapStatus::Pending),
            ("UpdateNotecardTaskInventory", CapStatus::Pending),
            ("CopyInventoryFromNotecard", CapStatus::Pending),
            ("UpdateScriptAgent", CapStatus::Pending),
            ("UpdateScriptTask", CapStatus::Pending),
            ("UpdateSettingsAgentInventory", CapStatus::Pending),
            ("ObjectAnimation", CapStatus::Pending),
            ("ObjectMedia", CapStatus::Pending),
            ("ObjectMediaNavigate", CapStatus::Pending),
            ("RenderMaterials", CapStatus::Pending),
            ("ModifyMaterialParams", CapStatus::Pending),
            ("UpdateMaterialAgentInventory", CapStatus::Pending),
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
            ("ReadOfflineMsgs", CapStatus::Pending),
            ("ChatSessionRequest", CapStatus::Pending),
            ("AcceptGroupInvite", CapStatus::Pending),
            ("DeclineGroupInvite", CapStatus::Pending),
            ("InventoryAPIv3", CapStatus::Pending),
            ("LibraryAPIv3", CapStatus::Pending),
            ("CreateInventoryCategory", CapStatus::Pending),
            ("ExtEnvironment", CapStatus::Pending),
            ("GetDisplayNames", CapStatus::Pending),
            ("RemoteParcelRequest", CapStatus::Pending),
            ("SimulatorFeatures", CapStatus::Pending),
            ("LSLSyntax", CapStatus::Pending),
            ("AgentPreferences", CapStatus::Pending),
            ("GetObjectCost", CapStatus::Pending),
            ("ResourceCostSelected", CapStatus::Pending),
            ("GetObjectPhysicsData", CapStatus::Pending),
            ("AttachmentResources", CapStatus::Pending),
            ("LandResources", CapStatus::Pending),
            ("SendUserReport", CapStatus::Pending),
            ("SendUserReportWithScreenshot", CapStatus::Pending),
            ("DirectDelivery", CapStatus::Pending),
        ];
        let actual: Vec<(&str, CapStatus)> = REQUESTED_CAPABILITIES
            .iter()
            .map(|name| {
                if SimCaps::handler_for(name).is_some() {
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

    /// `grant` serves only registered capabilities and is a pure read:
    /// repeated grants return identical maps.
    #[test]
    fn grant_omits_unsupported_and_is_stable() -> Result<(), TestError> {
        let caps = caps()?;
        let requested = vec![
            "EventQueueGet".to_owned(),
            "GetTexture".to_owned(),
            "NoSuchCap".to_owned(),
        ];
        let granted = caps.grant(&requested);
        assert_eq!(granted.len(), 1);
        assert!(granted.contains_key("EventQueueGet"));
        assert_eq!(granted, caps.grant(&requested));
        assert!(caps.supports("EventQueueGet"));
        assert!(!caps.supports("GetTexture"));
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
