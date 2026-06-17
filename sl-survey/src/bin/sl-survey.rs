#![doc = include_str!("../../README.md")]

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write as _;

use sl_client_tokio::{
    Client, Command, Event, LoginParams, LoginRequest, Maturity, ParcelInfo, ProductType,
    RegionIdentity, RegionLimits, Vector, grid_to_handle, handle_to_grid,
};
use tokio::sync::mpsc;
use tracing::{instrument, warn};
use tracing_subscriber::{
    EnvFilter, Layer as _, Registry, filter::LevelFilter, layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};

/// The number of 4×4 m squares along one edge of a (standard 256 m) region.
const SQUARES_PER_SIDE: usize = 64;
/// The total number of 4×4 m squares in a region.
const TOTAL_SQUARES: usize = SQUARES_PER_SIDE * SQUARES_PER_SIDE;
/// The edge length of one parcel grid square, in metres.
const SQUARE_METRES: u16 = 4;
/// The region-local position teleported to on arrival (region centre).
const ARRIVAL_POSITION: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 30.0,
};
/// The look-at direction used on arrival.
const ARRIVAL_LOOK_AT: Vector = Vector {
    x: 1.0,
    y: 0.0,
    z: 0.0,
};

/// Error enum for the application.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// error in clap
    #[error("error in CLI option parsing: {0}")]
    ClapError(
        #[source]
        #[from]
        clap::Error,
    ),
    /// error parsing log filter
    #[error("error parsing log filter: {0}")]
    LogFilterParseError(
        #[source]
        #[from]
        tracing_subscriber::filter::ParseError,
    ),
    /// error joining task
    #[error("error joining task: {0}")]
    JoinError(
        #[source]
        #[from]
        tokio::task::JoinError,
    ),
    /// error constructing tracing-journald layer
    #[cfg(target_os = "linux")]
    #[error("error constructing tracing-journald layer: {0}")]
    TracingJournaldError(#[source] std::io::Error),
    /// error generating man pages
    #[error("error generating man pages: {0}")]
    GenerateManpageError(#[source] std::io::Error),
    /// error generating shell completion
    #[error("error generating shell completion: {0}")]
    GenerateShellCompletionError(#[source] std::io::Error),
    /// error from the underlying client
    #[error("client error: {0}")]
    ClientError(
        #[source]
        #[from]
        sl_client_tokio::Error,
    ),
    /// I/O error writing survey output
    #[error("output I/O error: {0}")]
    IoError(
        #[source]
        #[from]
        std::io::Error,
    ),
    /// error serializing a survey record
    #[error("JSON serialization error: {0}")]
    JsonError(
        #[source]
        #[from]
        serde_json::Error,
    ),
}

/// Parameters for the `survey` subcommand.
#[derive(clap::Parser, Debug, Clone)]
pub struct SurveyParameters {
    /// The XML-RPC login endpoint URL.
    #[clap(long, default_value = "http://127.0.0.1:9000/")]
    login_uri: String,
    /// The avatar's first name.
    #[clap(long)]
    first: String,
    /// The avatar's last name.
    #[clap(long)]
    last: String,
    /// The avatar's password (also read from `SL_PASSWORD`).
    #[clap(long, env = "SL_PASSWORD", hide_env_values = true)]
    password: String,
    /// The login start location (`last`, `home`, or `uri:Region&x&y&z`).
    #[clap(long, default_value = "last")]
    start: String,
    /// The viewer channel reported to the grid.
    #[clap(long, default_value = "sl-survey")]
    channel: String,
    /// The viewer version reported to the grid.
    #[clap(long, default_value = clap::crate_version!())]
    version: String,
    /// The grid x coordinate (region index) of the start region.
    #[clap(long)]
    start_x: u32,
    /// The grid y coordinate (region index) of the start region.
    #[clap(long)]
    start_y: u32,
    /// The minimum grid x coordinate to survey (inclusive).
    #[clap(long)]
    min_x: u32,
    /// The minimum grid y coordinate to survey (inclusive).
    #[clap(long)]
    min_y: u32,
    /// The maximum grid x coordinate to survey (inclusive).
    #[clap(long)]
    max_x: u32,
    /// The maximum grid y coordinate to survey (inclusive).
    #[clap(long)]
    max_y: u32,
    /// The maximum number of regions to survey before stopping.
    #[clap(long, default_value_t = 64)]
    max_regions: u32,
    /// How long to spend collecting data in each region.
    #[clap(long, default_value = "12s")]
    collection_time: humantime::Duration,
    /// The draw distance (metres) advertised so neighbours are enabled.
    #[clap(long, default_value_t = 256.0)]
    draw_distance: f32,
    /// The output file for JSON-lines records (`-` for standard output).
    #[clap(long, default_value = "-")]
    output: String,
}

/// Which subcommand to call.
#[derive(clap::Parser, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "clap subcommand variants hold their parameters inline and cannot be boxed"
)]
pub enum Subcommand {
    /// Survey a rectangle of the grid by teleporting between regions.
    Survey(SurveyParameters),
    /// Generate the man page.
    GenerateManpage {
        /// target dir for man page generation
        #[clap(long)]
        output_dir: std::path::PathBuf,
    },
    /// Generate shell completion.
    GenerateShellCompletion {
        /// output file for shell completion generation
        #[clap(long)]
        output_file: std::path::PathBuf,
        /// which shell
        #[clap(long)]
        shell: clap_complete::aot::Shell,
    },
}

/// The Clap type for all the commandline parameters.
#[derive(clap::Parser, Debug)]
#[clap(name = "sl-survey",
       about = clap::crate_description!(),
       author = clap::crate_authors!(),
       version = clap::crate_version!(),
       )]
struct Options {
    /// which subcommand to use
    #[clap(subcommand)]
    command: Subcommand,
}

/// The inclusive grid-coordinate rectangle to survey.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    /// Minimum grid x (inclusive).
    min_x: u32,
    /// Minimum grid y (inclusive).
    min_y: u32,
    /// Maximum grid x (inclusive).
    max_x: u32,
    /// Maximum grid y (inclusive).
    max_y: u32,
}

impl Bounds {
    /// Returns `true` if the grid coordinate lies within the rectangle.
    const fn contains(&self, grid_x: u32, grid_y: u32) -> bool {
        grid_x >= self.min_x && grid_x <= self.max_x && grid_y >= self.min_y && grid_y <= self.max_y
    }
}

/// A serialized parcel record.
#[derive(serde::Serialize, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "JSON output DTO mirrors the distinct parcel flag booleans"
)]
struct ParcelRecord {
    /// The parcel's region-local id.
    local_id: i32,
    /// The parcel area in square metres.
    area: i32,
    /// The minimum corner of the parcel bounding box.
    aabb_min: [f32; 3],
    /// The maximum corner of the parcel bounding box.
    aabb_max: [f32; 3],
    /// Anyone may rez objects here.
    create_objects: bool,
    /// Group members may rez objects here.
    create_group_objects: bool,
    /// A ban list is in effect (banlines).
    use_ban_list: bool,
    /// Access is restricted to an allow list.
    use_access_list: bool,
    /// Anonymous avatars are denied.
    deny_anonymous: bool,
    /// The parcel's maximum object/prim capacity.
    max_prims: i32,
    /// The region-wide maximum object/prim capacity.
    sim_wide_max_prims: i32,
}

impl ParcelRecord {
    /// Builds a record from a [`ParcelInfo`].
    const fn from_info(info: &ParcelInfo) -> Self {
        Self {
            local_id: info.local_id,
            area: info.area,
            aabb_min: [info.aabb_min.0, info.aabb_min.1, info.aabb_min.2],
            aabb_max: [info.aabb_max.0, info.aabb_max.1, info.aabb_max.2],
            create_objects: info.create_objects(),
            create_group_objects: info.create_group_objects(),
            use_ban_list: info.use_ban_list(),
            use_access_list: info.use_access_list(),
            deny_anonymous: info.deny_anonymous(),
            max_prims: info.max_prims,
            sim_wide_max_prims: info.sim_wide_max_prims,
        }
    }
}

/// A serialized per-region survey record (one JSON line).
#[derive(serde::Serialize, Debug)]
struct RegionRecord {
    /// The region handle.
    region_handle: u64,
    /// The region's grid x coordinate.
    grid_x: u32,
    /// The region's grid y coordinate.
    grid_y: u32,
    /// The region (simulator) name.
    sim_name: String,
    /// The maturity rating (`PG`/`Mature`/`Adult`/`Unknown`).
    maturity: &'static str,
    /// The product type (`FullRegion`/`Homestead`/`Openspace`/`Unknown`).
    product: &'static str,
    /// The raw product SKU string.
    product_sku: String,
    /// The raw product name string.
    product_name: String,
    /// The raw region flags bitfield.
    region_flags: u32,
    /// The maximum concurrent agents (0 if unknown).
    max_agents: u32,
    /// The hard agent cap (0 if not provided).
    hard_max_agents: u32,
    /// The hard region-wide object cap (0 if not provided).
    hard_max_objects: u32,
    /// The surveyed parcels.
    parcels: Vec<ParcelRecord>,
    /// The discovered neighbour region handles.
    neighbors: Vec<u64>,
    /// Whether every region square was covered by a parcel query.
    parcels_complete: bool,
    /// Whether the region's limits (`RegionInfo`) reply was received.
    limits_received: bool,
}

/// Accumulated state for the region currently being surveyed.
#[derive(Debug)]
struct RegionAccum {
    /// The region handle.
    handle: u64,
    /// The region identity (maturity/product), once received.
    identity: Option<RegionIdentity>,
    /// The region limits, once received.
    limits: Option<RegionLimits>,
    /// The parcels found so far.
    parcels: Vec<ParcelRecord>,
    /// Per-square coverage from parcel-bitmap walking.
    covered: Vec<bool>,
    /// The next parcel-query sequence id.
    next_seq: i32,
    /// The discovered neighbour handles (in survey bounds).
    neighbors: Vec<u64>,
}

impl RegionAccum {
    /// Creates an empty accumulator for `handle`.
    fn new(handle: u64) -> Self {
        Self {
            handle,
            identity: None,
            limits: None,
            parcels: Vec::new(),
            covered: vec![false; TOTAL_SQUARES],
            next_seq: 0,
            neighbors: Vec::new(),
        }
    }

    /// The index of the first not-yet-covered square, if any remain.
    fn first_uncovered(&self) -> Option<usize> {
        self.covered.iter().position(|covered| !covered)
    }

    /// Marks every square set in `bitmap` as covered, returning how many were
    /// newly covered.
    fn merge_bitmap(&mut self, bitmap: &[u8]) -> usize {
        let mut newly = 0usize;
        for index in 0..TOTAL_SQUARES {
            let byte_index = index.checked_div(8).unwrap_or(0);
            let bit_index = u32::try_from(index.checked_rem(8).unwrap_or(0)).unwrap_or(0);
            let set = bitmap
                .get(byte_index)
                .is_some_and(|byte| byte.checked_shr(bit_index).unwrap_or(0) & 1 == 1);
            if set
                && let Some(slot) = self.covered.get_mut(index)
                && !*slot
            {
                *slot = true;
                newly = newly.saturating_add(1);
            }
        }
        newly
    }
}

/// Maps a [`Maturity`] to a stable string for output.
const fn maturity_str(maturity: Maturity) -> &'static str {
    match maturity {
        Maturity::Pg => "PG",
        Maturity::Mature => "Mature",
        Maturity::Adult => "Adult",
        Maturity::Unknown => "Unknown",
    }
}

/// Maps a [`ProductType`] to a stable string for output.
const fn product_str(product: ProductType) -> &'static str {
    match product {
        ProductType::FullRegion => "FullRegion",
        ProductType::Homestead => "Homestead",
        ProductType::Openspace => "Openspace",
        ProductType::Unknown => "Unknown",
    }
}

/// Converts the metre coordinate of a square corner (in square units) to an
/// `f32` metre value.
fn square_metres(square: usize) -> f32 {
    let metres = u16::try_from(square)
        .unwrap_or(0)
        .checked_mul(SQUARE_METRES)
        .unwrap_or(0);
    f32::from(metres)
}

/// The result of running one session: either the survey is finished, or a
/// teleport to a known-named region failed and the driver should re-log in
/// there to continue.
enum SessionOutcome {
    /// The survey is complete (queue drained or the region cap reached).
    Done,
    /// Re-log in at the named region (its teleport failed) and continue.
    RelogAt {
        /// The destination region handle.
        handle: u64,
        /// The destination region name (for the login start URI).
        name: String,
    },
}

/// The breadth-first survey driver: consumes session events and issues commands.
/// Persistent BFS state survives re-logins; per-session state is reset each run.
struct Survey {
    /// The survey bounds.
    bounds: Bounds,
    /// The start region handle (anchor for the current session's handshake).
    start_handle: u64,
    /// The maximum number of regions to survey.
    max_regions: u32,
    /// How long to collect in each region.
    collection: std::time::Duration,
    /// The advertised draw distance.
    draw_distance: f32,
    /// Regions already surveyed (or arrived at).
    visited: HashSet<u64>,
    /// Handles already queued, to avoid duplicates.
    queued: HashSet<u64>,
    /// The breadth-first queue of region handles to visit.
    queue: VecDeque<u64>,
    /// Region handle to name, learned from the world map (for re-login).
    names: HashMap<u64, String>,
    /// Whether the world-map request has been sent yet (sent once).
    map_requested: bool,
    /// The number of regions surveyed so far.
    surveyed: u32,
    /// The latest region identity, buffered until the arrival marker.
    pending_identity: Option<RegionIdentity>,
    /// The handle of the region we are currently teleporting to, if any.
    pending_teleport: Option<u64>,
    /// A pending re-login target (set when a named teleport fails).
    relog_target: Option<(u64, String)>,
    /// The region currently being surveyed, if any.
    current: Option<RegionAccum>,
    /// When the current region's collection window ends.
    deadline: Option<tokio::time::Instant>,
    /// The JSON-lines output sink.
    writer: Box<dyn std::io::Write + Send>,
}

impl Survey {
    /// Creates a survey driver anchored at the start region.
    fn new(
        bounds: Bounds,
        start_handle: u64,
        max_regions: u32,
        collection: std::time::Duration,
        draw_distance: f32,
        writer: Box<dyn std::io::Write + Send>,
    ) -> Self {
        let mut queued = HashSet::new();
        queued.insert(start_handle);
        Self {
            bounds,
            start_handle,
            max_regions,
            collection,
            draw_distance,
            visited: HashSet::new(),
            queued,
            queue: VecDeque::new(),
            names: HashMap::new(),
            map_requested: false,
            surveyed: 0,
            pending_identity: None,
            pending_teleport: None,
            relog_target: None,
            current: None,
            deadline: None,
            writer,
        }
    }

    /// Drives one session over its event/command channels, returning whether the
    /// survey is finished or a re-login is needed. Per-session state is reset on
    /// entry so the persistent BFS state can be reused across re-logins.
    async fn run_session(
        &mut self,
        mut events: mpsc::Receiver<Event>,
        commands: mpsc::Sender<Command>,
    ) -> Result<SessionOutcome, Error> {
        self.current = None;
        self.deadline = None;
        self.pending_identity = None;
        self.pending_teleport = None;
        self.relog_target = None;
        commands
            .send(Command::SetDrawDistance(self.draw_distance))
            .await
            .ok();
        loop {
            let tick = async {
                match self.deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                maybe_event = events.recv() => {
                    let Some(event) = maybe_event else { break; };
                    if self.handle_event(event, &commands).await? {
                        break;
                    }
                }
                () = tick => {
                    self.finalize_current()?;
                    self.advance(&commands).await;
                }
            }
        }
        Ok(match self.relog_target.take() {
            Some((handle, name)) => SessionOutcome::RelogAt { handle, name },
            None => SessionOutcome::Done,
        })
    }

    /// Handles one session event. Returns `true` when the session is finished.
    async fn handle_event(
        &mut self,
        event: Event,
        commands: &mpsc::Sender<Command>,
    ) -> Result<bool, Error> {
        match event {
            Event::RegionInfoHandshake(identity) => {
                self.pending_identity = Some(*identity);
            }
            Event::RegionHandshakeComplete => {
                // The initial region: its handle comes from the CLI start coords.
                let handle = self.start_handle;
                self.arrive(handle, commands).await;
            }
            Event::RegionChanged { region_handle, .. } => {
                self.pending_teleport = None;
                self.arrive(region_handle, commands).await;
            }
            Event::RegionLimits(limits) => {
                if let Some(current) = self.current.as_mut() {
                    current.limits = Some(limits);
                }
            }
            Event::ParcelProperties(parcel) => {
                self.on_parcel(&parcel, commands).await;
            }
            Event::NeighborDiscovered(neighbor) => {
                self.queue_region(neighbor.region_handle, neighbor.grid_x, neighbor.grid_y);
                if let Some(current) = self.current.as_mut()
                    && !current.neighbors.contains(&neighbor.region_handle)
                {
                    current.neighbors.push(neighbor.region_handle);
                }
            }
            Event::MapBlock(region) => {
                self.names.insert(region.region_handle, region.name.clone());
                self.queue_region(region.region_handle, region.grid_x, region.grid_y);
            }
            Event::TeleportFailed { reason } => {
                // Ignore stray/duplicate failures (a teleport that times out also
                // draws a failure message from the simulator); only the one that
                // clears our in-flight teleport handle is acted upon.
                let Some(handle) = self.pending_teleport.take() else {
                    return Ok(false);
                };
                match self.names.get(&handle).map(|name| (handle, name.clone())) {
                    Some((handle, name)) => {
                        warn!("teleport failed ({reason}); re-logging in at {name}");
                        self.relog_target = Some((handle, name));
                        commands.send(Command::Logout).await.ok();
                    }
                    None => {
                        warn!("teleport failed ({reason}); advancing to the next region");
                        self.advance(commands).await;
                    }
                }
            }
            Event::Disconnected(reason) => {
                warn!("disconnected: {reason:?}");
                return Ok(true);
            }
            Event::LoggedOut => return Ok(true),
            Event::ChatReceived(_)
            | Event::ChatTyping { .. }
            | Event::InstantMessageReceived(_)
            | Event::ImTyping { .. }
            | Event::SitResult { .. }
            | Event::AvatarProperties(_)
            | Event::AvatarInterests(_)
            | Event::AvatarGroups { .. }
            | Event::AvatarPicks { .. }
            | Event::AvatarNotes { .. }
            | Event::InventorySkeleton(_)
            | Event::InventoryDescendents { .. }
            | Event::FriendList(_)
            | Event::FriendsOnline(_)
            | Event::FriendsOffline(_)
            | Event::FriendRightsChanged { .. }
            | Event::ActiveGroupChanged(_)
            | Event::GroupMemberships(_)
            | Event::GroupMembers { .. }
            | Event::GroupRoleData { .. }
            | Event::GroupRoleMembers { .. }
            | Event::GroupTitles { .. }
            | Event::GroupProfileReceived(_)
            | Event::GroupNotices { .. }
            | Event::GroupSessionMessage { .. }
            | Event::GroupSessionParticipant { .. }
            | Event::CreateGroupResult { .. }
            | Event::JoinGroupResult { .. }
            | Event::LeaveGroupResult { .. }
            | Event::DroppedFromGroup { .. }
            | Event::ScriptDialog(_)
            | Event::ScriptPermissionRequest(_)
            | Event::LoadUrl(_)
            | Event::ScriptTeleport(_)
            | Event::MuteList(_)
            | Event::MuteListUnchanged
            | Event::MoneyBalance(_)
            | Event::EconomyData(_)
            | Event::ParcelDwell { .. }
            | Event::ParcelAccessList { .. }
            | Event::EstateInfo(_)
            | Event::EstateAccessList { .. }
            | Event::MapItems { .. }
            | Event::NeighborSeed { .. }
            | Event::ObjectAdded(_)
            | Event::ObjectUpdated(_)
            | Event::ObjectRemoved { .. }
            | Event::ObjectProperties(_)
            | Event::TeleportStarted
            | Event::TeleportProgress { .. }
            | Event::TeleportLocal
            | Event::ParcelOverlay(_)
            | Event::CircuitEstablished { .. } => {}
        }
        Ok(false)
    }

    /// Begins surveying the region identified by `handle`.
    async fn arrive(&mut self, handle: u64, commands: &mpsc::Sender<Command>) {
        if self.visited.contains(&handle) {
            // Already surveyed (e.g. a revisit); move on.
            self.advance(commands).await;
            return;
        }
        self.visited.insert(handle);
        let mut accum = RegionAccum::new(handle);
        accum.identity = self.pending_identity.take();
        self.current = Some(accum);
        let now = tokio::time::Instant::now();
        self.deadline = Some(now.checked_add(self.collection).unwrap_or(now));
        // Request the world map once, to enumerate the in-bounds regions and
        // resolve their names (used for the re-login fallback).
        if !self.map_requested {
            self.map_requested = true;
            commands
                .send(Command::RequestMapBlocks {
                    min_x: self.bounds.min_x,
                    max_x: self.bounds.max_x,
                    min_y: self.bounds.min_y,
                    max_y: self.bounds.max_y,
                })
                .await
                .ok();
        }
        commands.send(Command::RequestRegionInfo).await.ok();
        self.send_next_parcel_query(commands).await;
    }

    /// Sends a parcel query for the first uncovered square, if any remain.
    async fn send_next_parcel_query(&mut self, commands: &mpsc::Sender<Command>) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let Some(index) = current.first_uncovered() else {
            return;
        };
        let col = index.checked_rem(SQUARES_PER_SIDE).unwrap_or(0);
        let row = index.checked_div(SQUARES_PER_SIDE).unwrap_or(0);
        let west = square_metres(col);
        let south = square_metres(row);
        let east = square_metres(col.saturating_add(1));
        let north = square_metres(row.saturating_add(1));
        let sequence_id = current.next_seq;
        current.next_seq = current.next_seq.saturating_add(1);
        commands
            .send(Command::RequestParcelProperties {
                west,
                south,
                east,
                north,
                sequence_id,
            })
            .await
            .ok();
    }

    /// Records a parcel reply and continues the bitmap walk.
    async fn on_parcel(&mut self, parcel: &ParcelInfo, commands: &mpsc::Sender<Command>) {
        let advance = {
            let Some(current) = self.current.as_mut() else {
                return;
            };
            let newly = current.merge_bitmap(&parcel.bitmap);
            // The simulator may send a parcel both unsolicited (on entry) and in
            // reply to our query, so record each local id only once.
            if !current
                .parcels
                .iter()
                .any(|recorded| recorded.local_id == parcel.local_id)
            {
                current.parcels.push(ParcelRecord::from_info(parcel));
            }
            // Keep walking only while the query made progress, to avoid looping
            // on a grid that ignores or wrongly answers a query.
            newly > 0
        };
        if advance {
            self.send_next_parcel_query(commands).await;
        }
    }

    /// Queues a region for a later visit, if it is in bounds and not already
    /// surveyed or queued.
    fn queue_region(&mut self, handle: u64, grid_x: u32, grid_y: u32) {
        if !self.bounds.contains(grid_x, grid_y) {
            return;
        }
        if self.visited.contains(&handle) || self.queued.contains(&handle) {
            return;
        }
        self.queued.insert(handle);
        self.queue.push_back(handle);
    }

    /// Writes the current region's record and clears it.
    fn finalize_current(&mut self) -> Result<(), Error> {
        let Some(current) = self.current.take() else {
            return Ok(());
        };
        self.deadline = None;
        let (grid_x, grid_y) = handle_to_grid(current.handle);
        let parcels_complete = current.first_uncovered().is_none();
        let handle = current.handle;
        let identity = current.identity;
        let limits = current.limits;
        let parcels = current.parcels;
        let neighbors = current.neighbors;
        let record = RegionRecord {
            region_handle: handle,
            grid_x,
            grid_y,
            sim_name: identity
                .as_ref()
                .map(|identity| identity.sim_name.clone())
                .or_else(|| limits.as_ref().map(|limits| limits.sim_name.clone()))
                .unwrap_or_default(),
            maturity: maturity_str(
                identity
                    .as_ref()
                    .map_or(Maturity::Unknown, |identity| identity.maturity),
            ),
            product: product_str(
                identity
                    .as_ref()
                    .map_or(ProductType::Unknown, |identity| identity.product),
            ),
            product_sku: identity
                .as_ref()
                .map(|identity| identity.product_sku.clone())
                .unwrap_or_default(),
            product_name: identity
                .as_ref()
                .map(|identity| identity.product_name.clone())
                .unwrap_or_default(),
            region_flags: identity
                .as_ref()
                .map_or(0, |identity| identity.region_flags),
            max_agents: limits.as_ref().map_or(0, |limits| limits.max_agents),
            hard_max_agents: limits.as_ref().map_or(0, |limits| limits.hard_max_agents),
            hard_max_objects: limits.as_ref().map_or(0, |limits| limits.hard_max_objects),
            parcels,
            neighbors,
            parcels_complete,
            limits_received: limits.is_some(),
        };
        let line = serde_json::to_string(&record)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        self.surveyed = self.surveyed.saturating_add(1);
        Ok(())
    }

    /// Teleports to the next queued region (by handle), or logs out when the
    /// queue is drained or the region cap is reached. A teleport that fails is
    /// handled in `handle_event` by re-logging in directly at the region (using
    /// its map name) as a fallback.
    async fn advance(&mut self, commands: &mpsc::Sender<Command>) {
        if self.surveyed >= self.max_regions {
            commands.send(Command::Logout).await.ok();
            return;
        }
        while let Some(handle) = self.queue.pop_front() {
            if self.visited.contains(&handle) {
                continue;
            }
            self.pending_teleport = Some(handle);
            commands
                .send(Command::Teleport {
                    region_handle: handle,
                    position: ARRIVAL_POSITION,
                    look_at: ARRIVAL_LOOK_AT,
                })
                .await
                .ok();
            return;
        }
        commands.send(Command::Logout).await.ok();
    }
}

/// Implementation of the survey subcommand.
///
/// # Errors
///
/// Fails if login, the survey run, or output writing fails.
#[instrument(skip(parameters))]
async fn survey_command(parameters: SurveyParameters) -> Result<(), Error> {
    let bounds = Bounds {
        min_x: parameters.min_x,
        min_y: parameters.min_y,
        max_x: parameters.max_x,
        max_y: parameters.max_y,
    };
    let start_handle = grid_to_handle(parameters.start_x, parameters.start_y);

    let writer: Box<dyn std::io::Write + Send> = if parameters.output == "-" {
        Box::new(std::io::stdout())
    } else {
        Box::new(std::fs::File::create(&parameters.output)?)
    };

    let mut survey = Survey::new(
        bounds,
        start_handle,
        parameters.max_regions,
        parameters.collection_time.into(),
        parameters.draw_distance,
        writer,
    );

    // Each iteration runs one logged-in session; a teleport failure to a named
    // region ends the session with a re-login at that region.
    let mut start_location = parameters.start;
    loop {
        let request = LoginRequest::new(
            parameters.first.clone(),
            parameters.last.clone(),
            parameters.password.clone(),
            start_location.clone(),
            parameters.channel.clone(),
            parameters.version.clone(),
        );
        let params = LoginParams {
            login_uri: parameters.login_uri.clone(),
            request,
        };

        tracing::info!("logging in for survey");
        let client = Client::connect(params).await?;
        let (event_tx, event_rx) = mpsc::channel::<Event>(256);
        let (command_tx, command_rx) = mpsc::channel::<Command>(64);
        let run = tokio::spawn(client.run(event_tx, command_rx));

        let outcome = survey.run_session(event_rx, command_tx).await?;
        run.await??;

        match outcome {
            SessionOutcome::Done => break,
            SessionOutcome::RelogAt { handle, name } => {
                tracing::info!("re-logging in at {name} to continue the survey");
                survey.start_handle = handle;
                start_location = format!("uri:{name}&128&128&30");
            }
        }
    }

    tracing::info!("survey complete");
    Ok(())
}

/// The main behaviour of the binary.
///
/// # Errors
///
/// Fails if the selected subcommand fails.
#[instrument]
async fn do_stuff() -> Result<(), crate::Error> {
    let options = <Options as clap::Parser>::parse();
    tracing::debug!("{:#?}", options);

    match options.command {
        Subcommand::Survey(parameters) => {
            survey_command(parameters).await?;
        }
        Subcommand::GenerateManpage { output_dir } => {
            clap_mangen::generate_to(<Options as clap::CommandFactory>::command(), output_dir)
                .map_err(crate::Error::GenerateManpageError)?;
        }
        Subcommand::GenerateShellCompletion { output_file, shell } => {
            let mut f = std::fs::File::create(output_file)
                .map_err(crate::Error::GenerateShellCompletionError)?;
            let mut c = <Options as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut c, "sl-survey", &mut f);
        }
    }

    Ok(())
}

/// The main function mainly just handles setting up tracing
/// and handling any Err Results.
#[tokio::main]
async fn main() -> Result<(), Error> {
    let terminal_env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .parse(std::env::var("RUST_LOG").unwrap_or_else(|_ignored| "warn".to_owned()))?;
    let file_env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::TRACE.into())
        .parse(std::env::var("SL_SURVEY_LOG").unwrap_or_else(|_ignored| "trace".to_owned()))?;
    #[cfg(target_os = "linux")]
    let journald_env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::TRACE.into())
        .parse(
            std::env::var("SL_SURVEY_JOURNALD_LOG").unwrap_or_else(|_ignored| "info".to_owned()),
        )?;
    let registry = Registry::default();
    let registry =
        registry.with(tracing_subscriber::fmt::Layer::default().with_filter(terminal_env_filter));
    let log_dir = std::env::var("SL_SURVEY_LOG_DIR");
    let file_layer = if let Ok(log_dir) = log_dir {
        let log_file = std::env::var("SL_SURVEY_LOG_FILE")
            .unwrap_or_else(|_ignored| "sl_survey.log".to_owned());
        let file_appender = tracing_appender::rolling::never(log_dir, log_file);
        Some(
            tracing_subscriber::fmt::Layer::default()
                .with_writer(file_appender)
                .with_filter(file_env_filter),
        )
    } else {
        None
    };
    let registry = registry.with(file_layer);
    #[cfg(target_os = "linux")]
    let registry = registry.with(
        tracing_journald::layer()
            .map_err(crate::Error::TracingJournaldError)?
            .with_filter(journald_env_filter),
    );
    registry.init();
    log_panics::init();
    #[expect(
        clippy::print_stderr,
        reason = "final print in our error chain; tracing output above may be filtered out"
    )]
    match do_stuff().await {
        Ok(()) => (),
        Err(e) => {
            tracing::error!("{e}");
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    tracing::debug!("Exiting");
    Ok(())
}
