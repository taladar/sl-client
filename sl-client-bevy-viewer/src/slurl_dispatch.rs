//! The **SLURL parse & action dispatcher** (`viewer-slurl-parse-dispatch`): the
//! registry that turns a recognised Second Life URL into a viewer action.
//!
//! # The two halves
//!
//! Parsing already lives in [`crate::url_linkify`] — the shared matcher that turns
//! a run of text into [`LinkTarget`]s, faithful to the reference `LLUrlRegistry`.
//! This module is the **other half**, the reference `LLURLDispatcher` /
//! `LLCommandHandler` family: it takes a parsed [`LinkTarget`] and routes it to a
//! registered handler — open a profile, start an IM, teleport, centre the world
//! map, mute a resident.
//!
//! # Sources
//!
//! A SLURL reaches the dispatcher from several places, all funnelled through the
//! one routing core ([`route_target`]):
//!
//! - **in-app clicks** — a link the user clicks in chat, a notification, a
//!   profile: the [`crate::linkified_text`] widget emits [`LinkActivated`], which
//!   [`dispatch_link_activations`] routes (skipping plain web links, which the
//!   widget opens itself — the reference internal/external browser split).
//! - **external / command-line** — the OS `secondlife://` protocol handler
//!   launches the viewer with the SLURL as an argument (the reference
//!   `secondlife:` registration). [`capture_startup_slurl`] stashes it and
//!   [`apply_startup_slurl`] dispatches it once the agent is in-region. Any other
//!   caller can raise [`DispatchSlurl`] with a raw string to the same effect.
//!
//! # Location handlers (region name → destination)
//!
//! A location SLURL (`secondlife://Region/x/y/z`, a `maps.secondlife.com` link,
//! or the `app/region|teleport|worldmap` apps) names its destination by **region
//! name**, which must be resolved to a grid position before the viewer can act —
//! the reference `LLWorldMapMessage::sendNamedRegionRequest` round trip. The
//! dispatcher fires [`Command::RequestMapByName`] and parks the request in
//! [`PendingLocations`]; [`drive_location_resolves`] completes it when the
//! matching `MapBlockReply` ([`SlSessionEvent::MapBlock`]) lands — teleporting
//! (through the shared [`issue_teleport`] backend, so it drives the same progress
//! overlay every teleport surface uses) or centring the world map
//! ([`OpenWorldMap`]). A parcel link (`app/parcel/<id>/about`) resolves its
//! anchor the same way through [`Command::RequestParcelInfo`] /
//! [`SlSessionEvent::ParcelDetails`] ([`drive_parcel_resolves`]).
//!
//! A bare **teleport** app link (`app/teleport/...`) is guarded behind a
//! confirmation (the reference `TeleportViaSLAPP` alert), so a hostile chat line
//! cannot teleport the agent on a single click; the other location forms open the
//! **world map** centred on the destination (the roadmap routing for this
//! project's map/parcel handlers) rather than teleporting.
//!
//! # Split of responsibilities
//!
//! The `agent/.../inspect`, `objectim` and `app/object/.../inspect` targets — the
//! mini-inspector popups — are handled by [`crate::inspector_popup`]
//! ([[viewer-inspector-popups]]); this dispatcher deliberately leaves them alone
//! so the two consumers partition the [`LinkActivated`] stream cleanly.
//!
//! Reference (Firestorm, read-only): `llurldispatcher` (the region / teleport /
//! grid dispatch), `llcommandhandler` (the `app/` command registry),
//! `llpanelprofile` `LLAgentHandler` (the agent verbs), `llgroupactions`
//! `LLGroupHandler`, `llpanelplaces` `LLParcelHandler`, `llviewerregion`
//! `LLRegionHandler`, `llfloaterworldmap` (`LLWorldMapHandler`), `llurlaction`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use sl_client_bevy::{
    AgentKey, Command, MuteType, RegionCoordinates, RegionHandle, SlCommand, SlEvent, SlIdentity,
    SlSessionEvent, Vector,
};

use crate::avatar_profile::OpenAvatarProfile;
use crate::avatars::AvatarState;
use crate::conversations::{ConversationKey, OpenConversation};
use crate::group_profile::OpenGroupProfile;
use crate::linkified_text::LinkActivated;
use crate::mutes::RequestBlock;
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::teleport_progress::{BeginTeleportFlow, TeleportTarget, issue_teleport};
use crate::url_linkify::{LinkTarget, LocationCoords, LocationKind, TextRun, linkify};
use crate::web_floater::{OpenWebBrowser, open_in_system_browser};
use crate::world_map::OpenWorldMap;

/// The catalogue template the teleport-SLURL confirmation raises (the reference
/// `TeleportViaSLAPP` alert). Answered "Teleport" resolves the region and jumps;
/// "Cancel" (or a dismiss) drops the parked destination.
pub(crate) const TELEPORT_VIA_SLAPP_TEMPLATE: &str = "TeleportViaSLAPP";

/// The affirmative button name the [`TELEPORT_VIA_SLAPP_TEMPLATE`] form carries
/// (the stable reference `OK` functor name; its visible label reads "Teleport").
const TELEPORT_CONFIRM_BUTTON: &str = "OK";

/// How long a parked region / parcel resolution waits for its reply before it is
/// abandoned, in seconds — a slow or missing `MapBlockReply` must not leak a
/// pending entry forever.
const RESOLVE_TIMEOUT_SECONDS: f64 = 30.0;

/// The default region-local arrival coordinate for an axis a location SLURL
/// omitted — the region centre for X / Y, matching the reference's `(128, 128, 0)`
/// fallback.
const DEFAULT_HORIZONTAL: i32 = 128;

/// The largest region-local horizontal coordinate (metres), so an arrival on the
/// far edge stays inside the region.
const MAX_HORIZONTAL: i32 = 255;

/// The largest altitude (Z) a SLURL arrival is clamped to, in metres — generous
/// headroom over a region's build ceiling, and within `i16` so the metre value
/// converts to `f32` without a lossy cast.
const MAX_ALTITUDE: i32 = 8192;

/// One region-edge length, in metres — the grid-index → global-metre scale.
const REGION_SIZE_METERS: f64 = 256.0;

// ---------------------------------------------------------------------------
// Public entry: a raw SLURL string from an external source.
// ---------------------------------------------------------------------------

/// A request to parse and dispatch a raw SLURL / app-command string — the entry
/// point for sources outside the in-app link widgets: the `secondlife://` OS
/// protocol handler / command line ([`apply_startup_slurl`]) and any future
/// caller (a landmark's embedded SLURL, a typed address bar). The string is run
/// through the same [`linkify`] matcher the text layer uses, and its first
/// recognised link is routed.
#[derive(Message, Debug, Clone)]
pub(crate) struct DispatchSlurl {
    /// The raw URL string to parse and act on.
    pub(crate) url: String,
}

/// The command-line SLURL captured at startup (the reference `secondlife:`
/// protocol argument), held until the agent is in-region and it can be
/// dispatched. `None` when the viewer was launched without one.
#[derive(Resource, Debug, Default)]
struct StartupSlurl {
    /// The captured URL, cleared once dispatched so it fires exactly once.
    url: Option<String>,
}

// ---------------------------------------------------------------------------
// Parked location / parcel resolutions.
// ---------------------------------------------------------------------------

/// What a resolved location does once its region is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocationAction {
    /// Teleport the agent to the destination (a confirmed `app/teleport` link).
    Teleport,
    /// Centre the world map on the destination (every other location form).
    ShowOnMap,
}

/// A location awaiting its region-name resolution: the destination it names and
/// what to do once the `MapBlockReply` arrives.
#[derive(Debug, Clone)]
struct PendingLocation {
    /// The destination region name (as it will match a `MapBlockReply` name,
    /// case-insensitively).
    region: String,
    /// The region-local arrival coordinates the SLURL supplied.
    coords: LocationCoords,
    /// What to do once resolved.
    action: LocationAction,
    /// The absolute time (seconds) after which the request is abandoned.
    deadline: f64,
}

/// A parcel awaiting its `ParcelInfoReply`: once the anchor's global position is
/// known, the world map centres there (the reference `app/parcel/<id>/about`
/// place lookup, routed here to the world map).
#[derive(Debug, Clone, Copy)]
struct PendingParcel {
    /// The grid-wide parcel id being resolved.
    parcel_id: sl_client_bevy::ParcelKey,
    /// The absolute time (seconds) after which the request is abandoned.
    deadline: f64,
}

/// The dispatcher's parked async resolutions and the single outstanding teleport
/// confirmation.
#[derive(Resource, Debug, Default)]
struct PendingLocations {
    /// Region-name resolutions in flight.
    locations: Vec<PendingLocation>,
    /// Parcel-anchor resolutions in flight.
    parcels: Vec<PendingParcel>,
    /// The destination a raised [`TELEPORT_VIA_SLAPP_TEMPLATE`] confirm is
    /// guarding — a single slot: a second teleport link (before the first is
    /// answered) replaces it, matching the reference's one-at-a-time modal alert.
    teleport_confirm: Option<(String, LocationCoords)>,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// Wires the SLURL dispatcher: the routing systems, the async region / parcel
/// resolvers, the teleport-confirmation reader, and the startup-SLURL capture.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SlurlDispatchPlugin;

impl Plugin for SlurlDispatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DispatchSlurl>()
            .init_resource::<StartupSlurl>()
            .init_resource::<PendingLocations>()
            .add_systems(Startup, capture_startup_slurl)
            .add_systems(
                Update,
                (
                    apply_startup_slurl,
                    dispatch_link_activations,
                    dispatch_external_slurls,
                    handle_teleport_confirmations,
                    drive_location_resolves,
                    drive_parcel_resolves,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// The routing core.
// ---------------------------------------------------------------------------

/// The message channels the router writes to, grouped so the routing systems stay
/// within Bevy's per-system parameter budget.
#[derive(SystemParam)]
struct DispatchOut<'w> {
    /// Open an avatar profile floater.
    profiles: MessageWriter<'w, OpenAvatarProfile>,
    /// Open a group profile floater.
    group_profiles: MessageWriter<'w, OpenGroupProfile>,
    /// Open / select an IM conversation.
    conversations: MessageWriter<'w, OpenConversation>,
    /// Raise a confirmation (the teleport-SLURL guard).
    notifications: MessageWriter<'w, ShowNotification>,
    /// Send a protocol command (unmute, friendship offer, name / map / parcel
    /// requests).
    commands: MessageWriter<'w, SlCommand>,
    /// Ask for a block — the guarded channel every Block affordance uses.
    blocks: MessageWriter<'w, RequestBlock>,
    /// Open the embedded web browser (a trusted web SLURL from an external
    /// source).
    browsers: MessageWriter<'w, OpenWebBrowser>,
}

/// Route a parsed [`LinkTarget`] to its handler. `allow_web` opens plain web
/// links here (the external / command-line path); it is `false` for an in-app
/// click, where [`crate::linkified_text`] has already opened the web link itself.
/// `url` is the canonical link URL, used for a web open and for logging.
fn route_target(
    target: &LinkTarget,
    url: &str,
    out: &mut DispatchOut,
    pending: &mut PendingLocations,
    avatars: &AvatarState,
    now: f64,
    allow_web: bool,
) {
    match target {
        LinkTarget::Web { trusted } => {
            if allow_web {
                open_web(url, *trusted, out);
            }
        }
        LinkTarget::Agent { key, action, grid } => {
            if grid.is_some() {
                debug!("slurl: ignoring cross-grid agent link {url}");
                return;
            }
            route_agent(*key, action, out, avatars);
        }
        LinkTarget::Group { key, grid } => {
            if grid.is_some() {
                debug!("slurl: ignoring cross-grid group link {url}");
                return;
            }
            out.group_profiles.write(OpenGroupProfile { group: *key });
        }
        LinkTarget::Parcel { key, grid } => {
            if grid.is_some() {
                debug!("slurl: ignoring cross-grid parcel link {url}");
                return;
            }
            request_parcel(*key, out, pending, now);
        }
        LinkTarget::Location {
            kind,
            grid,
            region,
            coords,
        } => {
            if grid.is_some() {
                debug!("slurl: ignoring cross-grid location link {url}");
                return;
            }
            route_location(*kind, region, *coords, out, pending, now);
        }
        // The inspector popups own the object targets ([[viewer-inspector-popups]]);
        // the dispatcher deliberately leaves them for that consumer.
        LinkTarget::Object { .. } | LinkTarget::ObjectAction { .. } => {
            debug!("slurl: {url} routed by the inspector, not the dispatcher");
        }
        LinkTarget::Experience { .. } => {
            info!("slurl: experience profiles are not yet supported ({url})");
        }
    }
}

/// Route an `app/agent/<id>/<verb>` link, mirroring the reference `LLAgentHandler`
/// verbs the viewer can act on. The `inspect` verb is the inspector popup's
/// (handled elsewhere); every unrecognised or name-style verb opens the full
/// profile, matching the common "click a resident name → profile" behaviour.
fn route_agent(agent: AgentKey, action: &str, out: &mut DispatchOut, avatars: &AvatarState) {
    match action.to_ascii_lowercase().as_str() {
        // The inspector popup owns `inspect` — never reached here (the router
        // sends `Object`/inspect targets to that consumer), but guarded anyway.
        "inspect" => {}
        "im" => {
            out.conversations.write(OpenConversation {
                key: ConversationKey::Direct(agent),
            });
        }
        "offerteleport" => {
            out.commands.write(SlCommand(Command::OfferTeleport {
                targets: vec![agent],
                message: String::new(),
            }));
        }
        "requestfriend" => {
            out.commands.write(SlCommand(Command::OfferFriendship {
                to_agent_id: agent,
                message: String::new(),
            }));
        }
        "mute" => {
            let name = avatars
                .name_of(agent)
                .map(ToOwned::to_owned)
                .unwrap_or_default();
            out.blocks
                .write(RequestBlock::new(agent.uuid(), name, MuteType::Agent));
        }
        "unmute" => {
            let name = avatars
                .name_of(agent)
                .map(ToOwned::to_owned)
                .unwrap_or_default();
            out.commands.write(SlCommand(Command::Unmute {
                id: agent.uuid(),
                name,
            }));
        }
        "pay" => {
            // Avatar pay is not wired yet (the pie-menu Pay slice is a
            // placeholder too); do not silently no-op the click misleadingly.
            info!("slurl: avatar pay is not yet supported");
        }
        // about / mention / completename / displayname / username / anything
        // else: open the full profile.
        _profile => {
            out.profiles.write(OpenAvatarProfile { agent });
        }
    }
}

/// Route a location SLURL by its form: a confirmed teleport for the `app/teleport`
/// app, otherwise open the world map centred on the destination. Both need the
/// region name resolved first, so the work is parked in [`PendingLocations`].
fn route_location(
    kind: LocationKind,
    region: &str,
    coords: LocationCoords,
    out: &mut DispatchOut,
    pending: &mut PendingLocations,
    now: f64,
) {
    if region.is_empty() {
        debug!("slurl: location link has no region name");
        return;
    }
    match kind {
        LocationKind::Teleport => {
            // Guard a teleport behind the reference confirmation, so a hostile
            // chat link cannot jump the agent on one click. The destination is
            // parked in a single slot until the confirm is answered.
            pending.teleport_confirm = Some((region.to_owned(), coords));
            out.notifications.write(
                ShowNotification::new(TELEPORT_VIA_SLAPP_TEMPLATE)
                    .arg("LOCATION", location_label(region, coords)),
            );
        }
        LocationKind::Slurl
        | LocationKind::MapUrl
        | LocationKind::Region
        | LocationKind::WorldMap => {
            park_location(region, coords, LocationAction::ShowOnMap, out, pending, now);
        }
    }
}

/// Park a region-name resolution and fire its `MapBlockReply` request.
fn park_location(
    region: &str,
    coords: LocationCoords,
    action: LocationAction,
    out: &mut DispatchOut,
    pending: &mut PendingLocations,
    now: f64,
) {
    out.commands.write(SlCommand(Command::RequestMapByName {
        name: region.to_owned(),
    }));
    pending.locations.push(PendingLocation {
        region: region.to_owned(),
        coords,
        action,
        deadline: now + RESOLVE_TIMEOUT_SECONDS,
    });
}

/// Park a parcel-anchor resolution and fire its `ParcelInfoReply` request.
fn request_parcel(
    parcel_id: sl_client_bevy::ParcelKey,
    out: &mut DispatchOut,
    pending: &mut PendingLocations,
    now: f64,
) {
    out.commands
        .write(SlCommand(Command::RequestParcelInfo { parcel_id }));
    pending.parcels.push(PendingParcel {
        parcel_id,
        deadline: now + RESOLVE_TIMEOUT_SECONDS,
    });
}

/// Open a plain web link — the embedded browser for a trusted Second Life host,
/// the system browser otherwise (the reference internal/external split).
fn open_web(url: &str, trusted: bool, out: &mut DispatchOut) {
    if trusted {
        out.browsers.write(OpenWebBrowser {
            url: Some(url.to_owned()),
        });
    } else {
        open_in_system_browser(url);
    }
}

// ---------------------------------------------------------------------------
// Routing systems.
// ---------------------------------------------------------------------------

/// Route every in-app link click ([`LinkActivated`]) — the chat / notification /
/// profile link surfaces. Plain web links are skipped (the widget opened them).
fn dispatch_link_activations(
    mut activated: MessageReader<LinkActivated>,
    mut out: DispatchOut,
    mut pending: ResMut<PendingLocations>,
    avatars: Res<AvatarState>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    for event in activated.read() {
        route_target(
            &event.target,
            &event.url,
            &mut out,
            &mut pending,
            &avatars,
            now,
            false,
        );
    }
}

/// Route every external / command-line SLURL ([`DispatchSlurl`]): parse the raw
/// string and route its first recognised link (web links included, since no
/// widget opened them).
fn dispatch_external_slurls(
    mut requests: MessageReader<DispatchSlurl>,
    mut out: DispatchOut,
    mut pending: ResMut<PendingLocations>,
    avatars: Res<AvatarState>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    for request in requests.read() {
        let Some(link) = first_link(&request.url) else {
            info!("slurl: no recognised link in {}", request.url);
            continue;
        };
        route_target(
            &link.target,
            &link.url,
            &mut out,
            &mut pending,
            &avatars,
            now,
            true,
        );
    }
}

/// The teleport confirmation's answer: on "Teleport" resolve the parked
/// destination and jump; on cancel / dismiss drop it.
fn handle_teleport_confirmations(
    mut responses: MessageReader<NotificationResponse>,
    mut out: DispatchOut,
    mut pending: ResMut<PendingLocations>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    for response in responses.read() {
        if response.template != TELEPORT_VIA_SLAPP_TEMPLATE {
            continue;
        }
        let Some((region, coords)) = pending.teleport_confirm.take() else {
            continue;
        };
        if response.button == Some(TELEPORT_CONFIRM_BUTTON) {
            park_location(
                &region,
                coords,
                LocationAction::Teleport,
                &mut out,
                &mut pending,
                now,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Async resolvers.
// ---------------------------------------------------------------------------

/// Complete parked region-name resolutions as their `MapBlockReply` lands
/// ([`SlSessionEvent::MapBlock`]), and drop the timed-out ones. Reads the map
/// replies directly (broadcast alongside the world-map floater's own reader), so
/// the dispatcher needs no coupling to the floater's model.
fn drive_location_resolves(
    mut events: MessageReader<SlEvent>,
    mut pending: ResMut<PendingLocations>,
    mut commands: MessageWriter<SlCommand>,
    mut world_map: MessageWriter<OpenWorldMap>,
    mut begin: MessageWriter<BeginTeleportFlow>,
    time: Res<Time>,
) {
    // Collect this frame's resolved region names → (grid position, handle).
    let mut resolved: Vec<(String, u32, u32, RegionHandle)> = Vec::new();
    for event in events.read() {
        if let SlSessionEvent::MapBlock(info) = &event.0
            && let Some(name) = info.name.as_ref()
        {
            resolved.push((
                name.to_string(),
                info.grid_coordinates.x(),
                info.grid_coordinates.y(),
                info.region_handle,
            ));
        }
    }
    let now = time.elapsed_secs_f64();
    pending.locations.retain(|location| {
        if let Some((_name, grid_x, grid_y, handle)) = resolved
            .iter()
            .find(|(name, _x, _y, _handle)| name.eq_ignore_ascii_case(&location.region))
        {
            complete_location(
                location,
                *grid_x,
                *grid_y,
                *handle,
                &mut commands,
                &mut world_map,
                &mut begin,
            );
            return false;
        }
        if now >= location.deadline {
            info!(
                "slurl: region '{}' did not resolve; dropping",
                location.region
            );
            return false;
        }
        true
    });
}

/// Act on a resolved location: teleport to it (through the shared backend, so the
/// progress overlay opens) or centre the world map on it.
fn complete_location(
    location: &PendingLocation,
    grid_x: u32,
    grid_y: u32,
    handle: RegionHandle,
    commands: &mut MessageWriter<SlCommand>,
    world_map: &mut MessageWriter<OpenWorldMap>,
    begin: &mut MessageWriter<BeginTeleportFlow>,
) {
    match location.action {
        LocationAction::Teleport => {
            let position = RegionCoordinates::new(
                region_local(location.coords.x, DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
                region_local(location.coords.y, DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
                region_local(location.coords.z, 0, MAX_ALTITUDE),
            );
            let label = format!(
                "{} ({:.0}, {:.0})",
                location.region,
                position.x(),
                position.y()
            );
            issue_teleport(
                commands,
                begin,
                TeleportTarget {
                    region_handle: handle,
                    position,
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                },
                Some(label),
            );
        }
        LocationAction::ShowOnMap => {
            world_map.write(OpenWorldMap {
                east: global_axis(
                    grid_x,
                    location.coords.x,
                    DEFAULT_HORIZONTAL,
                    MAX_HORIZONTAL,
                ),
                north: global_axis(
                    grid_y,
                    location.coords.y,
                    DEFAULT_HORIZONTAL,
                    MAX_HORIZONTAL,
                ),
            });
        }
    }
}

/// Complete parked parcel resolutions as their `ParcelInfoReply` lands, centring
/// the world map on the parcel anchor's global position; drop the timed-out ones.
fn drive_parcel_resolves(
    mut events: MessageReader<SlEvent>,
    mut pending: ResMut<PendingLocations>,
    mut world_map: MessageWriter<OpenWorldMap>,
    time: Res<Time>,
) {
    let mut resolved: Vec<(sl_client_bevy::ParcelKey, f64, f64)> = Vec::new();
    for event in events.read() {
        if let SlSessionEvent::ParcelDetails(details) = &event.0 {
            resolved.push((
                details.parcel_id,
                details.global_position.x(),
                details.global_position.y(),
            ));
        }
    }
    let now = time.elapsed_secs_f64();
    pending.parcels.retain(|parcel| {
        if let Some((_id, east, north)) =
            resolved.iter().find(|(id, _e, _n)| *id == parcel.parcel_id)
        {
            world_map.write(OpenWorldMap {
                east: *east,
                north: *north,
            });
            return false;
        }
        if now >= parcel.deadline {
            info!("slurl: parcel did not resolve; dropping");
            return false;
        }
        true
    });
}

// ---------------------------------------------------------------------------
// Startup SLURL (the `secondlife://` OS protocol / command line).
// ---------------------------------------------------------------------------

/// Capture a `secondlife://` / `hop://` / `x-grid-location-info://` / map-URL
/// SLURL passed on the command line (the OS protocol handler launches the viewer
/// with it), to dispatch once the agent is in-region.
fn capture_startup_slurl(mut startup: ResMut<StartupSlurl>) {
    startup.url = std::env::args().skip(1).find(|arg| is_slurl_arg(arg));
    if let Some(url) = startup.url.as_ref() {
        info!("slurl: captured startup URL {url}");
    }
}

/// Whether a command-line argument looks like a dispatchable SLURL / map URL.
fn is_slurl_arg(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    lower.starts_with("secondlife://")
        || lower.starts_with("hop://")
        || lower.starts_with("x-grid-location-info://")
        || lower.contains("maps.secondlife.com/secondlife/")
        || lower.contains("slurl.com/secondlife/")
}

/// Dispatch the captured startup SLURL once the agent is in-region (so a location
/// can resolve), then clear it so it fires exactly once.
fn apply_startup_slurl(
    identity: Option<Res<SlIdentity>>,
    mut startup: ResMut<StartupSlurl>,
    mut requests: MessageWriter<DispatchSlurl>,
) {
    if startup.url.is_none() {
        return;
    }
    let in_region = identity.is_some_and(|identity| identity.region_handle.is_some());
    if !in_region {
        return;
    }
    if let Some(url) = startup.url.take() {
        info!("slurl: dispatching startup URL {url}");
        requests.write(DispatchSlurl { url });
    }
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// The first recognised link in `text`, or `None` if it holds none.
fn first_link(text: &str) -> Option<crate::url_linkify::LinkMatch> {
    linkify(text).into_iter().find_map(|run| match run {
        TextRun::Link(link) => Some(link),
        TextRun::Plain(_) => None,
    })
}

/// A region-local arrival coordinate in metres as an `f32`: the supplied value or
/// `default`, clamped to `[0, max]`. The clamp keeps the value within `i16`, so
/// the metre count converts to `f32` losslessly (no `as` cast).
fn region_local(coord: Option<i32>, default: i32, max: i32) -> f32 {
    let clamped = coord.unwrap_or(default).clamp(0, max);
    f32::from(i16::try_from(clamped).unwrap_or(0))
}

/// A global-metre axis position from a grid index and a region-local coordinate:
/// `grid * 256 + local`, with the local value defaulted / clamped as at arrival.
fn global_axis(grid: u32, coord: Option<i32>, default: i32, max: i32) -> f64 {
    f64::from(grid).mul_add(
        REGION_SIZE_METERS,
        f64::from(coord.unwrap_or(default).clamp(0, max)),
    )
}

/// The confirmation's `[LOCATION]` label: `Region (x, y, z)` from the parsed
/// destination, filling omitted coordinates with the arrival defaults so the user
/// sees exactly where "Teleport" would send them.
fn location_label(region: &str, coords: LocationCoords) -> String {
    format!(
        "{region} ({}, {}, {})",
        coords.x.unwrap_or(DEFAULT_HORIZONTAL),
        coords.y.unwrap_or(DEFAULT_HORIZONTAL),
        coords.z.unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_HORIZONTAL, MAX_ALTITUDE, MAX_HORIZONTAL, first_link, global_axis, is_slurl_arg,
        region_local,
    };
    use crate::url_linkify::{LinkTarget, LocationKind};

    /// A region-local coordinate defaults an omitted axis and clamps an
    /// out-of-range one, staying within the region / altitude bounds.
    #[test]
    fn region_local_defaults_and_clamps() {
        let close = |value: f32, want: f32| (value - want).abs() < 1.0e-3;
        assert!(close(
            region_local(Some(64), DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
            64.0
        ));
        assert!(
            close(
                region_local(None, DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
                128.0
            ),
            "an omitted axis falls back to the region centre"
        );
        assert!(
            close(
                region_local(Some(9000), DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
                255.0
            ),
            "an over-range horizontal is clamped to the region edge"
        );
        assert!(
            close(region_local(Some(-5), 0, MAX_ALTITUDE), 0.0),
            "a negative coordinate is clamped to the floor"
        );
    }

    /// A global axis is `grid * 256 + local`, with the local value defaulted.
    #[test]
    fn global_axis_combines_grid_and_local() {
        let close = |value: f64, want: f64| (value - want).abs() < 1.0e-6;
        // Grid index 1000 → 256000 m; local 128 (default) → 256128.
        assert!(close(
            global_axis(1000, None, DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
            256_128.0
        ));
        assert!(close(
            global_axis(1000, Some(200), DEFAULT_HORIZONTAL, MAX_HORIZONTAL),
            256_200.0
        ));
    }

    /// The startup-argument sniff accepts the SLURL / map-URL schemes and rejects
    /// an ordinary flag or file argument.
    #[test]
    fn startup_argument_sniff() {
        assert!(is_slurl_arg("secondlife://Ahern/128/128/24"));
        assert!(is_slurl_arg("hop://grid.example.org:8002/Sandbox/10/20"));
        assert!(is_slurl_arg(
            "http://maps.secondlife.com/secondlife/Ahern/128/128/24"
        ));
        assert!(!is_slurl_arg("--windowed"));
        assert!(!is_slurl_arg("/home/user/config.toml"));
    }

    /// The external-source parser pulls the first recognised link out of a raw
    /// string, and its parsed target is what the router acts on.
    #[test]
    fn first_link_extracts_a_location_target() -> Result<(), Box<dyn core::error::Error>> {
        let link = first_link("secondlife://Ahern/128/128/24").ok_or("expected a link")?;
        assert!(matches!(
            link.target,
            LinkTarget::Location {
                kind: LocationKind::Slurl,
                ..
            }
        ));
        assert!(first_link("just some plain text").is_none());
        Ok(())
    }
}
