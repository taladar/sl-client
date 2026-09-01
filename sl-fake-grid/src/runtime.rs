//! The grid orchestrator: the builder, the running [`FakeGrid`], and the
//! per-login session bring-up shared by the HTTP endpoints.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use sl_proto::{
    ArrivalPlacement, EnvironmentSettings, Maturity, OpenSimExtras, ProductType, RegionHandle,
    RegionIdentity, SimSession, SimulatorFeatures, Uuid, VoiceConfig, region_name_from_wire,
};
use sl_types::key::AgentKey;
use sl_types::lsl::Vector;
use sl_types::map::RegionCoordinates;
use sl_wire::{
    GridInfo, KEY_ECONOMY, KEY_GRIDNAME, KEY_GRIDNICK, KEY_LOGIN, KEY_MESSAGE, KEY_PLATFORM,
    LoginGates, LoginSuccess, MapTileRef, SkeletonFolder,
};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{Mutex, broadcast, watch};

use crate::accounts::{Account, AccountConfig};
use crate::driver::{SharedSim, SimState, new_shared_sim, run_timer, run_udp_pump};
use crate::economy_policy::{EconomyConfig, EconomyEvent};
use crate::error::Error;
use crate::map_tiles::MapTileStore;
use crate::scenario::Scenario;
use crate::terrain::TerrainFixture;
use crate::time::{Now, system_clock};
use crate::world::AVATAR_CENTRE_ABOVE_GROUND_M;

/// How the grid describes itself in `get_grid_info` and the login message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridIdentity {
    /// The human-readable grid name (`gridname`).
    pub name: String,
    /// The short nickname (`gridnick`).
    pub nick: String,
    /// The message of the day (`message`, also the login response message).
    pub message: String,
    /// The platform name (`platform`); `OpenSim` makes Firestorm's grid
    /// manager treat the grid as an OpenSim one.
    pub platform: String,
}

impl Default for GridIdentity {
    /// "Fake Grid" / `fakegrid`, platform `OpenSim`.
    fn default() -> Self {
        Self {
            name: "Fake Grid".to_owned(),
            nick: "fakegrid".to_owned(),
            message: "Welcome to the fake grid".to_owned(),
            platform: "OpenSim".to_owned(),
        }
    }
}

/// One region a [`FakeGridBuilder`] defines.
#[derive(Debug, Clone)]
pub struct RegionConfig {
    /// The region name (shown in the viewer, matched by
    /// [`AccountConfig::start_region`](crate::AccountConfig)).
    pub name: String,
    /// The region's grid X index (metres are `grid_x * 256`).
    pub grid_x: u32,
    /// The region's grid Y index.
    pub grid_y: u32,
    /// A fixed region id, or `None` to mint one when the grid starts.
    pub region_id: Option<Uuid>,
    /// The region's maturity rating.
    pub maturity: Maturity,
    /// The region's water height, in metres.
    pub water_height: f32,
    /// The region's ground: the heights streamed as `LayerData` on arrival
    /// and served as the estate RAW download, the wind and cloud fields, and
    /// the detail-texture composition the `RegionHandshake` carries.
    pub terrain: TerrainFixture,
    /// Environment settings (day cycle, day length, sky-track altitudes) the
    /// `ExtEnvironment` capability serves for the region, or `None` for the
    /// session's stock four-hour day. The `parcel_id` says what they apply to
    /// (`-1` = the whole region).
    pub environment: Option<EnvironmentSettings>,
    /// A scenario overriding the grid-wide one for this region.
    pub scenario: Option<Scenario>,
}

impl Default for RegionConfig {
    /// A general-rated 256 m region called "Fake Region" at grid
    /// `(1000, 1000)` with the stock water height and the stock ground
    /// ([`TerrainFixture::default`]: flat at
    /// [`STOCK_TERRAIN_HEIGHT_M`](crate::scenario::STOCK_TERRAIN_HEIGHT_M)).
    fn default() -> Self {
        Self {
            name: "Fake Region".to_owned(),
            grid_x: 1000,
            grid_y: 1000,
            region_id: None,
            maturity: Maturity::Pg,
            water_height: 20.0,
            terrain: TerrainFixture::default(),
            environment: None,
            scenario: None,
        }
    }
}

/// A registered region after the grid started: its config, minted id, and
/// effective scenario.
pub(crate) struct RegionEntry {
    /// The builder-supplied region data.
    pub(crate) config: RegionConfig,
    /// The region id (fixed or minted once at start).
    pub(crate) region_id: Uuid,
    /// The scenario sessions in this region are seeded with.
    scenario: Scenario,
}

impl RegionEntry {
    /// The region handle derived from the grid coordinates.
    pub(crate) fn handle(&self) -> RegionHandle {
        RegionHandle::from_grid(self.config.grid_x, self.config.grid_y)
    }

    /// Builds the identity the automatic `RegionHandshake` carries.
    fn identity(&self) -> RegionIdentity {
        let handle = self.handle();
        RegionIdentity {
            sim_name: region_name_from_wire("fake-grid", &self.config.name)
                .ok()
                .flatten(),
            region_id: self.region_id,
            region_handle: handle,
            grid_coordinates: sl_types::map::GridCoordinates::new(
                self.config.grid_x,
                self.config.grid_y,
            ),
            region_flags: 0,
            region_flags_extended: 0,
            region_protocols: 0,
            maturity: self.config.maturity,
            product: ProductType::FullRegion,
            product_sku: String::new(),
            product_name: "Fake Region".to_owned(),
            cpu_class_id: 0,
            cpu_ratio: 1,
            sim_owner: Uuid::nil(),
            is_estate_manager: false,
            water_height: self.config.water_height,
            billable_factor: 1.0,
            terrain: self.config.terrain.composition,
        }
    }
}

/// The identity a login mints once and every circuit of that login shares:
/// the client opens each circuit (the login region, a teleport destination)
/// with the same `UseCircuitCode` triple, and the legacy asset upload path
/// derives asset ids from the secure session id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionIds {
    /// The session id the client echoes in every message.
    pub(crate) session_id: Uuid,
    /// The secure session id (asset-upload entropy).
    pub(crate) secure_session_id: Uuid,
    /// The circuit code.
    pub(crate) circuit_code: sl_wire::CircuitCode,
}

impl SessionIds {
    /// Mints a fresh identity for a login.
    fn mint(minter: &IdMinter) -> Self {
        Self {
            session_id: minter.uuid(),
            secure_session_id: minter.uuid(),
            circuit_code: sl_wire::CircuitCode(minter.circuit_code()),
        }
    }
}

/// The grid's one source of minted identifiers — session ids, capability
/// tokens, circuit codes, defaulted agent and region ids.
///
/// By default it is as random as [`Uuid::new_v4`] ever was. Seeded
/// ([`FakeGridBuilder::deterministic`]), it is a xorshift stream instead, so
/// two grids built from the same seed mint the same identifiers in the same
/// order — which is what lets two runs of a scenario produce comparable
/// records. The minted uuids still carry the v4 version and variant bits, so
/// nothing downstream can tell them from random ones.
#[derive(Clone, Debug, Default)]
pub(crate) struct IdMinter {
    /// The xorshift state behind a lock, or `None` for OS randomness.
    seeded: Option<std::sync::Arc<std::sync::Mutex<u64>>>,
}

impl IdMinter {
    /// A deterministic minter: the same seed yields the same stream.
    pub(crate) fn seeded(seed: u64) -> Self {
        Self {
            // Zero is xorshift's fixed point; nudge it off.
            seeded: Some(std::sync::Arc::new(std::sync::Mutex::new(seed.max(1)))),
        }
    }

    /// The next 64 raw bits of the stream.
    fn next_bits(&self) -> Option<u64> {
        let state = self.seeded.as_ref()?;
        let mut guard = state.lock().ok()?;
        // Marsaglia xorshift64.
        let mut x = *guard;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *guard = x;
        drop(guard);
        Some(x)
    }

    /// The next uuid: seeded stream bits shaped as a v4, or a real v4.
    pub(crate) fn uuid(&self) -> Uuid {
        match (self.next_bits(), self.next_bits()) {
            (Some(high), Some(low)) => {
                let raw = (u128::from(high) << 64) | u128::from(low);
                let versioned =
                    (raw & !(0xf000 << 48) & !(0xc0 << 56)) | (0x4000 << 48) | (0x80 << 56);
                Uuid::from_u128(versioned)
            }
            _unseeded => Uuid::new_v4(),
        }
    }

    /// A non-zero circuit code.
    pub(crate) fn circuit_code(&self) -> u32 {
        let low = self.uuid().as_u128() & u128::from(u32::MAX);
        u32::try_from(low).unwrap_or(1).max(1)
    }
}

/// A record of one completed inter-region teleport, published on
/// [`FakeGrid::teleports`]: the agent now lives in session `to_seq`, and
/// session `from_seq` has been retired.
#[derive(Debug, Clone)]
pub struct TeleportNotice {
    /// The teleported agent.
    pub agent_id: AgentKey,
    /// The retired source session's sequence number.
    pub from_seq: u64,
    /// The destination session's sequence number.
    pub to_seq: u64,
    /// The destination region's name.
    pub region_name: String,
}

/// A record of one successful login, published on
/// [`FakeGrid::logins`].
#[derive(Debug, Clone)]
pub struct LoginNotice {
    /// The sequence number identifying the new session.
    pub session_seq: u64,
    /// The logged-in agent.
    pub agent_id: AgentKey,
    /// The avatar's first name.
    pub first_name: String,
    /// The avatar's last name.
    pub last_name: String,
    /// The region logged into.
    pub region_name: String,
}

/// The shared core the HTTP endpoints and the public handle both hold.
pub(crate) struct GridCore {
    /// The registered accounts.
    pub(crate) accounts: Vec<Account>,
    /// The registered regions (first entry = default start region).
    regions: Vec<RegionEntry>,
    /// The login policy gates applied to every account.
    pub(crate) gates: LoginGates,
    /// Whether the login response is trimmed to the request's `options`.
    pub(crate) honor_options: bool,
    /// The identifier source every login and capability grant draws from.
    pub(crate) minter: IdMinter,
    /// The clock every session machine is stamped from.
    pub(crate) clock: Now,
    /// How long an empty `EventQueueGet` poll is held before the 502.
    pub(crate) eq_hold: Duration,
    /// The bound HTTP port (fixed after `start`).
    pub(crate) http_port: u16,
    /// The login URI (`http://127.0.0.1:<port>/`), parsed once at start.
    login_uri: url::Url,
    /// The grid's self-description.
    pub(crate) identity: GridIdentity,
    /// The `get_grid_info` document, derived once at start.
    pub(crate) grid_info: GridInfo,
    /// The economy helper policy.
    pub(crate) economy: EconomyConfig,
    /// The world-map tiles served under the login URI.
    pub(crate) map_tiles: MapTileStore,
    /// Live sessions by sequence number.
    pub(crate) sessions: Mutex<HashMap<u64, SharedSim>>,
    /// Mints session sequence numbers.
    next_session: AtomicU64,
    /// Flipped to `true` on shutdown.
    pub(crate) shutdown_tx: watch::Sender<bool>,
    /// Publishes successful logins.
    pub(crate) logins_tx: broadcast::Sender<LoginNotice>,
    /// Publishes accepted economy-helper purchases.
    pub(crate) economy_tx: broadcast::Sender<EconomyEvent>,
    /// Publishes completed inter-region teleports.
    pub(crate) teleports_tx: broadcast::Sender<TeleportNotice>,
}

/// Everything a successful login mints before the response is built. The
/// enriched [`LoginSuccess`] is returned alongside (the login server
/// consumes it), so this struct only carries what activation needs.
pub(crate) struct PreparedSession {
    /// The session's sequence number (also its CAPS path component).
    pub(crate) seq: u64,
    /// The shared driver handle (not yet registered or pumped).
    pub(crate) shared: SharedSim,
    /// The region the session lives in (for the notices).
    pub(crate) region_name: String,
    /// The session's seed capability URL.
    pub(crate) seed_url: url::Url,
    /// The session's loopback UDP address.
    pub(crate) udp_addr: SocketAddr,
}

impl GridCore {
    /// The index of the region an account starts in.
    pub(crate) fn start_region(&self, account: &Account) -> Option<usize> {
        match &account.config.start_region {
            Some(name) => self.region_by_name(name),
            None => (!self.regions.is_empty()).then_some(0),
        }
    }

    /// The region at `index` (indices come from the lookups below and from
    /// [`SimState::region`]).
    pub(crate) fn region(&self, index: usize) -> Option<&RegionEntry> {
        self.regions.get(index)
    }

    /// The index of the region called `name`.
    pub(crate) fn region_by_name(&self, name: &str) -> Option<usize> {
        self.regions
            .iter()
            .position(|entry| entry.config.name == name)
    }

    /// The index of the region with the given handle.
    pub(crate) fn region_by_handle(&self, handle: RegionHandle) -> Option<usize> {
        self.regions
            .iter()
            .position(|entry| entry.handle() == handle)
    }

    /// The index of the region with the given id.
    pub(crate) fn region_by_id(&self, region_id: Uuid) -> Option<usize> {
        self.regions
            .iter()
            .position(|entry| entry.region_id == region_id)
    }

    /// The registered account owning `agent_id`.
    pub(crate) fn account_by_agent(&self, agent_id: AgentKey) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| account.agent_id == agent_id)
    }

    /// Creates the session machinery for a login: a fresh identity, the
    /// region session, and the enriched [`LoginSuccess`]. Nothing is
    /// registered yet — the caller only activates the session when the login
    /// server actually answers success.
    pub(crate) async fn prepare_session(
        self: &Arc<Self>,
        account: &Account,
        region_index: usize,
    ) -> Result<(PreparedSession, Box<LoginSuccess>), Error> {
        let ids = SessionIds::mint(&self.minter);
        let region = self.region(region_index).ok_or(Error::UnknownRegion {
            region: region_index.to_string(),
        })?;
        let prepared = self
            .prepare_region_session(account, region_index, ids, None)
            .await?;
        let mut success = Box::new(LoginSuccess::minimal(
            account.agent_id,
            ids.session_id,
            ids.secure_session_id,
            ids.circuit_code,
            Ipv4Addr::LOCALHOST,
            prepared.udp_addr.port(),
            prepared.seed_url.clone(),
        ));
        {
            let mut state = prepared.shared.state.lock().await;
            register_account_display_name(&mut state.sim, account);
            enrich_success(&mut success, account, region, &state.sim);
        }
        success.message = Some(self.identity.message.clone());
        success.map_server_url = Some(self.login_uri.clone());
        success.currency = Some(self.economy.currency_symbol.clone());
        Ok((prepared, success))
    }

    /// Creates one region session for an agent: binds the session's UDP
    /// socket, seeds a fresh [`SimSession`] with the region's scenario under
    /// the login's identity, and mints its CAPS surface. Shared by the login
    /// (the start region) and a teleport (the destination, with the arrival
    /// placed where the teleport asked). Nothing is registered yet.
    pub(crate) async fn prepare_region_session(
        self: &Arc<Self>,
        account: &Account,
        region_index: usize,
        ids: SessionIds,
        arrival: Option<ArrivalPlacement>,
    ) -> Result<PreparedSession, Error> {
        let region = self.region(region_index).ok_or(Error::UnknownRegion {
            region: region_index.to_string(),
        })?;
        let socket = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?);
        let udp_addr = socket.local_addr()?;
        let seq = self.next_session.fetch_add(1, Ordering::Relaxed);

        let now = (self.clock)();
        let mut sim = SimSession::new(region.handle(), now);
        sim.set_secure_session_id(ids.secure_session_id);
        sim.set_region_id(region.region_id);
        if let Some(arrival) = arrival {
            sim.set_arrival_position(arrival.position, arrival.look_at);
        } else {
            // A login with no placement of its own lands at the region centre,
            // and `ArrivalPlacement::default` puts that at z = 30 -- which is
            // the legacy default *camera* viewpoint, not a ground position. The
            // fake grid runs no physics, so nothing then pulls the avatar down:
            // it stands wherever it was rezzed, several metres above the
            // ground, out of frame for a camera aimed at the region's content
            // and casting its shadow from nowhere.
            //
            // Place it on the ground instead, at the same offset the NPC
            // fixtures use -- an avatar's position is its centre, so half its
            // 1.9 m height above the terrain.
            let center = sl_proto::Camera::region_center().center;
            let ground = region.config.terrain.height_at(center.x, center.y);
            sim.set_arrival_position(
                sl_types::map::RegionCoordinates::new(
                    center.x,
                    center.y,
                    ground + AVATAR_CENTRE_ABOVE_GROUND_M,
                ),
                Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            );
        }
        if let Some(environment) = region.config.environment.clone() {
            let stamped = EnvironmentSettings {
                region_id: if environment.region_id.is_nil() {
                    region.region_id
                } else {
                    environment.region_id
                },
                ..environment
            };
            sim.set_environment(stamped);
        }
        (region.scenario.setup)(&mut sim, now);
        region.scenario.udp_assets.register_xfer_files(&mut sim);
        if *sim.simulator_features() == SimulatorFeatures::default() {
            // The scenario left the feature document untouched: advertise
            // the grid-wide URLs (map tiles, currency helper) the way an
            // OpenSim region does through `OpenSimExtras`, and the voice
            // backend the scenario enabled (`VoiceServerType`).
            let mut features = self.simulator_features();
            features.voice_server_type = sim.voice().advertised_server_type().map(str::to_owned);
            sim.set_simulator_features(features);
        }

        let base_url: url::Url =
            format!("http://127.0.0.1:{}/sim/{seq}", self.http_port).parse()?;
        let caps = {
            let minter = self.minter.clone();
            sl_proto::SimCaps::new(base_url, self.minter.uuid(), move || minter.uuid())
        };
        let seed_url = caps.seed_url();

        // A scenario that names no RAW heightmap serves the region's own
        // ground, so the estate download matches what the viewer stands on.
        let mut udp_assets = region.scenario.udp_assets.clone();
        if udp_assets.terrain_raw.is_none() {
            udp_assets.terrain_raw = Some(region.config.terrain.to_raw());
        }

        let state = SimState {
            sim,
            caps,
            assets: region.scenario.assets.clone(),
            identity: region.identity(),
            on_agent_arrived: region.scenario.on_agent_arrived.clone(),
            on_event: region.scenario.on_event.clone(),
            udp_assets,
            terrain: region.config.terrain.clone(),
            world: region.scenario.world.clone(),
            avatar: crate::world::AvatarIdentity {
                agent_id: account.agent_id,
                first_name: account.config.first_name.clone(),
                last_name: account.config.last_name.clone(),
            },
            seq,
            region: region_index,
            ids,
        };
        let shared = new_shared_sim(
            state,
            socket,
            self.shutdown_tx.subscribe(),
            Arc::clone(&self.clock),
        );
        Ok(PreparedSession {
            seq,
            shared,
            region_name: region.config.name.clone(),
            seed_url,
            udp_addr,
        })
    }

    /// Registers a prepared session and starts its pump, timer, teleport
    /// responder and reaper tasks — called only once the login server
    /// answered success (or a teleport destination is about to be announced).
    pub(crate) async fn activate_session(self: &Arc<Self>, prepared: &PreparedSession) {
        self.sessions
            .lock()
            .await
            .insert(prepared.seq, prepared.shared.clone());
        tokio::spawn(run_udp_pump(prepared.shared.clone()));
        tokio::spawn(run_timer(prepared.shared.clone()));
        tokio::spawn(crate::teleport::run_teleport_responder(
            Arc::clone(self),
            prepared.shared.clone(),
        ));
        tokio::spawn(reap_closed_session(
            Arc::clone(self),
            prepared.seq,
            prepared.shared.clone(),
        ));
    }

    /// Looks a live session up by its sequence number.
    pub(crate) async fn session(&self, seq: u64) -> Option<SharedSim> {
        self.sessions.lock().await.get(&seq).cloned()
    }

    /// Forgets a session (its CAPS paths stop resolving); the pumps exit on
    /// the machine's own closed state.
    pub(crate) async fn remove_session(&self, seq: u64) {
        self.sessions.lock().await.remove(&seq);
    }

    /// The live **root** session of `agent_id` — where the avatar currently
    /// is — if it is logged in.
    ///
    /// A closed session is skipped even in the window before its reaper
    /// pruned it: `SimSession` never resets `agent_presence` on close, so a
    /// logged-out circuit still reports itself a root agent, and after a
    /// relogin `HashMap` order would otherwise decide which of the two a lure
    /// teleport targets.
    pub(crate) async fn root_session_of(&self, agent_id: AgentKey) -> Option<SharedSim> {
        let sessions: Vec<SharedSim> = self.sessions.lock().await.values().cloned().collect();
        for shared in sessions {
            if shared.is_closed() {
                continue;
            }
            let state = shared.state.lock().await;
            if state.avatar.agent_id == agent_id && state.sim.is_root_agent() {
                drop(state);
                return Some(shared);
            }
        }
        None
    }

    /// The stock `SimulatorFeatures` document: mesh enabled plus the
    /// `OpenSimExtras` URLs pointing back at this grid.
    fn simulator_features(&self) -> SimulatorFeatures {
        SimulatorFeatures {
            mesh_rez_enabled: Some(true),
            mesh_upload_enabled: Some(true),
            open_sim_extras: Some(OpenSimExtras {
                map_server_url: Some(self.login_uri.clone()),
                currency: Some(self.economy.currency_symbol.clone()),
                currency_base_uri: Some(self.login_uri.clone()),
                say_range: Some(20),
                shout_range: Some(100),
                whisper_range: Some(10),
                ..OpenSimExtras::default()
            }),
            ..SimulatorFeatures::default()
        }
    }
}

/// Prunes a session from the grid's table once its machine closes — logout,
/// inactivity, retirement after a teleport away, or abandonment — or the grid
/// shuts down.
///
/// Without it every login leaks its UDP socket, its clone of the scenario's
/// assets and its terrain for the life of the process, and
/// [`GridCore::root_session_of`] can hand a lure teleport a circuit whose
/// client is long gone.
async fn reap_closed_session(core: Arc<GridCore>, seq: u64, shared: SharedSim) {
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut shutdown_rx = shared.shutdown_rx.clone();
    while !*closed_rx.borrow_and_update() {
        tokio::select! {
            changed = closed_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
    core.remove_session(seq).await;
}

/// Derives the `get_grid_info` document: the login URI doubles as the
/// economy helper URI (the helper scripts live next to the login endpoint).
fn grid_info_of(identity: &GridIdentity, login_uri: &url::Url) -> GridInfo {
    GridInfo::new()
        .with(KEY_PLATFORM, identity.platform.clone())
        .with(KEY_LOGIN, login_uri.as_str())
        .with(KEY_GRIDNAME, identity.name.clone())
        .with(KEY_GRIDNICK, identity.nick.clone())
        .with(KEY_ECONOMY, login_uri.as_str())
        .with(KEY_MESSAGE, identity.message.clone())
}

/// Registers the logging-in account with the session's people-service store, so
/// the `GetDisplayNames` capability can resolve the agent's own id.
///
/// Without this the capability answers correctly but unhelpfully: the id lands
/// in `bad_ids`, and the reference viewer renders the name tag as
/// `(???) (???)` and caches that for an hour
/// (`LLAvatarNameResponder::result N unresolved ids`). An account the grid just
/// authenticated is precisely an id it should be able to name.
///
/// The record is the shape a grid sends for an agent who has never set a custom
/// display name: `display_name` equal to the legacy name and
/// `is_display_name_default` true. `username` is the SLID form — dotted and
/// lowercase, with the `Resident` last name elided, as Second Life does.
fn register_account_display_name(sim: &mut SimSession, account: &Account) {
    sim.set_display_name(
        crate::world::AvatarIdentity::new(
            account.agent_id,
            &account.config.first_name,
            &account.config.last_name,
        )
        .display_name_record(),
    );
}

/// Fills the optional login-response fields the fixtures can answer:
/// names, region placement, and the inventory/library skeletons derived
/// from the session's trees.
fn enrich_success(
    success: &mut LoginSuccess,
    account: &Account,
    region: &RegionEntry,
    sim: &SimSession,
) {
    success.first_name = Some(account.config.first_name.clone());
    success.last_name = Some(account.config.last_name.clone());
    success.region_x = region.config.grid_x.checked_mul(256);
    success.region_y = region.config.grid_y.checked_mul(256);
    success.region_size_x = Some(256);
    success.region_size_y = Some(256);
    success.agent_access = Some("M".to_owned());
    success.agent_access_max = Some("A".to_owned());
    // The `voice-config` section mirrors the backend the scenario enabled.
    success.voice_config = sim
        .voice()
        .advertised_server_type()
        .map(|voice_server_type| VoiceConfig {
            voice_server_type: voice_server_type.to_owned(),
        });
    success.start_location = Some("last".to_owned());
    success.seconds_since_epoch = Some(now_epoch_seconds());

    let (roots, skeleton) = skeleton_of(sim.agent_inventory());
    success.inventory_root = roots.first().copied();
    success.inventory_skeleton = skeleton;
    let (lib_roots, lib_skeleton) = skeleton_of(sim.library_inventory());
    success.library_root = lib_roots.first().copied();
    success.library_skeleton = lib_skeleton;
}

/// The wall-clock time as UNIX seconds (the `seconds_since_epoch` field).
fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0))
}

/// Derives the login-response skeleton from an inventory tree: the root
/// folder ids (parent `None`) and every folder as a [`SkeletonFolder`].
fn skeleton_of(
    tree: &sl_proto::SimInventoryTree,
) -> (Vec<sl_types::key::InventoryFolderKey>, Vec<SkeletonFolder>) {
    let mut roots = Vec::new();
    let mut skeleton = Vec::new();
    for folder in tree.folders() {
        if folder.parent_id.is_none() {
            roots.push(folder.folder_id);
        }
        skeleton.push(SkeletonFolder {
            folder_id: folder.folder_id,
            parent_id: folder
                .parent_id
                .unwrap_or_else(|| sl_types::key::InventoryFolderKey::from(Uuid::nil())),
            name: folder.name.clone(),
            type_default: folder.folder_type,
            version: folder.version,
        });
    }
    (roots, skeleton)
}

/// Configures and starts a [`FakeGrid`].
pub struct FakeGridBuilder {
    /// The accounts allowed to log in.
    accounts: Vec<AccountConfig>,
    /// The regions the grid serves (first = default start region).
    regions: Vec<RegionConfig>,
    /// The grid-wide scenario, unless a region overrides it.
    scenario: Scenario,
    /// The login policy gates.
    gates: LoginGates,
    /// Whether login responses honour the request's `options` list.
    honor_options: bool,
    /// The `EventQueueGet` hold before the 502 re-poll answer.
    eq_hold: Duration,
    /// The TCP port to bind, `0` for an ephemeral one.
    http_port: u16,
    /// The grid's self-description.
    identity: GridIdentity,
    /// The economy helper policy.
    economy: EconomyConfig,
    /// Builder-registered map tiles.
    map_tiles: MapTileStore,
    /// The identifier source (random unless seeded).
    minter: IdMinter,
    /// The clock every session machine is stamped from (the system clock
    /// unless [`FakeGridBuilder::clock`] replaced it).
    clock: Now,
}

impl std::fmt::Debug for FakeGridBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeGridBuilder")
            .field("accounts", &self.accounts)
            .field("regions", &self.regions)
            .field("scenario", &self.scenario)
            .field("gates", &self.gates)
            .field("honor_options", &self.honor_options)
            .field("eq_hold", &self.eq_hold)
            .field("http_port", &self.http_port)
            .field("identity", &self.identity)
            .field("economy", &self.economy)
            .field("map_tiles", &self.map_tiles)
            .field("minter", &self.minter)
            .field("clock", &"<closure>")
            .finish()
    }
}

impl Default for FakeGridBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeGridBuilder {
    /// Mint every identifier — session ids, capability tokens, circuit codes,
    /// defaulted agent and region ids — from a seeded stream, so two grids
    /// built from the same seed and content produce the same identifiers in
    /// the same order.
    #[must_use]
    pub fn deterministic(mut self, seed: u64) -> Self {
        self.minter = IdMinter::seeded(seed);
        self
    }

    /// Draws every grid-side instant from `clock` instead of the system one.
    ///
    /// A test that pauses tokio's timer passes
    /// [`crate::time::tokio_clock`], so the session machines stamp their
    /// deadlines on the same clock the timer tasks sleep on.
    #[must_use]
    pub fn clock(mut self, clock: Now) -> Self {
        self.clock = clock;
        self
    }

    /// A builder with no accounts, no regions, the stock scenario, no login
    /// gates, and a 30 s event-queue hold on an ephemeral port.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            regions: Vec::new(),
            scenario: Scenario::default(),
            minter: IdMinter::default(),
            clock: system_clock(),
            gates: LoginGates::default(),
            honor_options: false,
            eq_hold: Duration::from_secs(30),
            http_port: 0,
            identity: GridIdentity::default(),
            economy: EconomyConfig::default(),
            map_tiles: MapTileStore::default(),
        }
    }

    /// Adds an account.
    #[must_use]
    pub fn account(mut self, account: AccountConfig) -> Self {
        self.accounts.push(account);
        self
    }

    /// Adds a region.
    #[must_use]
    pub fn region(mut self, region: RegionConfig) -> Self {
        self.regions.push(region);
        self
    }

    /// Replaces the grid-wide scenario.
    #[must_use]
    pub fn scenario(mut self, scenario: Scenario) -> Self {
        self.scenario = scenario;
        self
    }

    /// Sets the login policy gates (ToS, critical message, redirect,
    /// already-logged-in).
    #[must_use]
    pub fn gates(mut self, gates: LoginGates) -> Self {
        self.gates = gates;
        self
    }

    /// Makes login responses honour the request's `options` list
    /// (SL behaviour; the default keeps every field like OpenSim).
    #[must_use]
    pub const fn honor_options(mut self, honor: bool) -> Self {
        self.honor_options = honor;
        self
    }

    /// Sets how long an empty `EventQueueGet` poll is held before the 502.
    #[must_use]
    pub const fn event_queue_hold(mut self, hold: Duration) -> Self {
        self.eq_hold = hold;
        self
    }

    /// Binds the HTTP listener to a fixed port instead of an ephemeral one.
    #[must_use]
    pub const fn http_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    /// Sets how the grid describes itself in `get_grid_info`.
    #[must_use]
    pub fn grid_identity(mut self, identity: GridIdentity) -> Self {
        self.identity = identity;
        self
    }

    /// Sets the economy helper policy (currency symbol, price, site state,
    /// upgrade requirements, confirm token).
    #[must_use]
    pub fn economy(mut self, economy: EconomyConfig) -> Self {
        self.economy = economy;
        self
    }

    /// Registers a world-map tile served at `map-<zoom>-<x>-<y>-objects.jpg`
    /// under the login URI. Every configured region gets a stock zoom-1 tile
    /// unless one is registered here.
    #[must_use]
    pub fn map_tile(mut self, tile: MapTileRef, jpeg: impl Into<Bytes>) -> Self {
        self.map_tiles.insert(tile, jpeg.into());
        self
    }

    /// Starts the grid: binds the HTTP listener, registers accounts and
    /// regions, and spawns the accept loop.
    ///
    /// # Errors
    ///
    /// Returns a [`Error`] when the configuration is inconsistent
    /// (no regions, duplicate names, unknown start region) or the listener
    /// cannot bind.
    pub async fn start(self) -> Result<FakeGrid, Error> {
        if self.regions.is_empty() {
            return Err(Error::NoRegions);
        }
        check_duplicates(&self.accounts, &self.regions)?;
        let minter = self.minter.clone();
        for account in &self.accounts {
            if let Some(region) = &account.start_region
                && !self.regions.iter().any(|entry| entry.name == *region)
            {
                return Err(Error::UnknownStartRegion {
                    account: format!("{} {}", account.first_name, account.last_name),
                    region: region.clone(),
                });
            }
        }

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, self.http_port)).await?;
        let http_port = listener.local_addr()?.port();
        let login_uri: url::Url = format!("http://127.0.0.1:{http_port}/").parse()?;
        let (shutdown_tx, _) = watch::channel(false);
        let (logins_tx, _) = broadcast::channel(LOGINS_CHANNEL_CAPACITY);
        let (economy_tx, _) = broadcast::channel(LOGINS_CHANNEL_CAPACITY);
        let (teleports_tx, _) = broadcast::channel(LOGINS_CHANNEL_CAPACITY);
        let mut map_tiles = self.map_tiles;
        for region in &self.regions {
            map_tiles.seed_region(region.grid_x, region.grid_y);
        }
        let grid_info = grid_info_of(&self.identity, &login_uri);

        let regions = self
            .regions
            .into_iter()
            .map(|config| {
                let region_id = config.region_id.unwrap_or_else(|| minter.uuid());
                let scenario = config
                    .scenario
                    .clone()
                    .unwrap_or_else(|| self.scenario.clone());
                RegionEntry {
                    config,
                    region_id,
                    scenario,
                }
            })
            .collect();
        let core = Arc::new(GridCore {
            accounts: self
                .accounts
                .into_iter()
                .map(|config| Account::register(config, &minter))
                .collect(),
            regions,
            gates: self.gates,
            honor_options: self.honor_options,
            minter: minter.clone(),
            clock: self.clock,
            eq_hold: self.eq_hold,
            http_port,
            login_uri,
            identity: self.identity,
            grid_info,
            economy: self.economy,
            map_tiles,
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicU64::new(1),
            shutdown_tx,
            logins_tx,
            economy_tx,
            teleports_tx,
        });
        tokio::spawn(crate::http_service::run_http(Arc::clone(&core), listener));
        Ok(FakeGrid { core })
    }
}

/// Broadcast capacity for login notices.
const LOGINS_CHANNEL_CAPACITY: usize = 32;

/// Rejects duplicate account and region names.
fn check_duplicates(accounts: &[AccountConfig], regions: &[RegionConfig]) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    for account in accounts {
        let name = format!("{} {}", account.first_name, account.last_name);
        if !seen.insert(name.clone()) {
            return Err(Error::Duplicate {
                kind: "account",
                name,
            });
        }
    }
    let mut seen = std::collections::HashSet::new();
    for region in regions {
        if !seen.insert(region.name.clone()) {
            return Err(Error::Duplicate {
                kind: "region",
                name: region.name.clone(),
            });
        }
    }
    Ok(())
}

/// A running loopback fake grid.
pub struct FakeGrid {
    /// The shared core the HTTP endpoints also hold.
    core: Arc<GridCore>,
}

impl FakeGrid {
    /// The login URI to point a client at (`http://127.0.0.1:<port>/`).
    #[must_use]
    pub fn login_uri(&self) -> url::Url {
        self.core.login_uri.clone()
    }

    /// The bound HTTP port.
    #[must_use]
    pub fn http_port(&self) -> u16 {
        self.core.http_port
    }

    /// The stable agent id minted for an account, if it exists.
    #[must_use]
    pub fn account_agent_id(&self, first_name: &str, last_name: &str) -> Option<AgentKey> {
        self.core
            .accounts
            .iter()
            .find(|account| {
                account.config.first_name == first_name && account.config.last_name == last_name
            })
            .map(|account| account.agent_id)
    }

    /// Subscribes to successful-login notices.
    #[must_use]
    pub fn logins(&self) -> broadcast::Receiver<LoginNotice> {
        self.core.logins_tx.subscribe()
    }

    /// Subscribes to accepted economy-helper purchases.
    #[must_use]
    pub fn economy_events(&self) -> broadcast::Receiver<EconomyEvent> {
        self.core.economy_tx.subscribe()
    }

    /// The `get_grid_info` document the grid serves.
    #[must_use]
    pub fn grid_info(&self) -> &GridInfo {
        &self.core.grid_info
    }

    /// How the grid describes itself.
    #[must_use]
    pub fn identity(&self) -> &GridIdentity {
        &self.core.identity
    }

    /// The live session handle for a login notice.
    pub async fn agent(&self, notice: &LoginNotice) -> Option<FakeAgent> {
        self.agent_by_seq(notice.session_seq).await
    }

    /// The live session handle for a session sequence number (a
    /// [`LoginNotice::session_seq`] or a [`TeleportNotice::to_seq`]).
    pub async fn agent_by_seq(&self, seq: u64) -> Option<FakeAgent> {
        let shared = self.core.session(seq).await?;
        let agent_id = shared.state.lock().await.avatar.agent_id;
        Some(FakeAgent { shared, agent_id })
    }

    /// Subscribes to completed inter-region teleports (both the automatic
    /// answers to client requests and [`FakeGrid::teleport_agent`]).
    #[must_use]
    pub fn teleports(&self) -> broadcast::Receiver<TeleportNotice> {
        self.core.teleports_tx.subscribe()
    }

    /// The names of the grid's regions, in builder order.
    #[must_use]
    pub fn region_names(&self) -> Vec<String> {
        self.core
            .regions
            .iter()
            .map(|entry| entry.config.name.clone())
            .collect()
    }

    /// The handle of the region called `name`, if it exists.
    #[must_use]
    pub fn region_handle(&self, name: &str) -> Option<RegionHandle> {
        self.core
            .region_by_name(name)
            .and_then(|index| self.core.region(index))
            .map(RegionEntry::handle)
    }

    /// The id of the region called `name`, if it exists (what a landmark
    /// fixture names).
    #[must_use]
    pub fn region_id(&self, name: &str) -> Option<Uuid> {
        self.core
            .region_by_name(name)
            .and_then(|index| self.core.region(index))
            .map(|entry| entry.region_id)
    }

    /// Teleports a logged-in agent to `region_name` — the grid-initiated
    /// counterpart of a client request (what a lure or a scripted push does):
    /// `TeleportStart`, the destination session, the event-queue trio, and
    /// the source's retirement once the destination confirms the arrival.
    /// Returns the handle onto the destination session; a same-region request
    /// finishes as a `TeleportLocal` and returns `agent` itself.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownRegion`] for a region the grid does not serve,
    /// [`Error::NotRootAgent`] when the agent has not arrived in its current
    /// region, [`Error::TeleportTimedOut`] when the client never completed
    /// its movement into the destination (the client was told
    /// `timeout_tport`), and socket errors binding the destination.
    pub async fn teleport_agent(
        &self,
        agent: &FakeAgent,
        region_name: &str,
        position: RegionCoordinates,
        look_at: Vector,
    ) -> Result<FakeAgent, Error> {
        let region = self
            .core
            .region_by_name(region_name)
            .ok_or_else(|| Error::UnknownRegion {
                region: region_name.to_owned(),
            })?;
        let request = crate::teleport::TeleportRequest {
            region,
            arrival: ArrivalPlacement { position, look_at },
            flags: sl_types::map::TeleportFlags::VIA_LOCATION,
            progress: sl_proto::teleport_strings::SENDING_DEST,
        };
        match crate::teleport::teleport_session(&self.core, &agent.shared, request).await? {
            crate::teleport::TeleportOutcome::Local => Ok(agent.clone()),
            crate::teleport::TeleportOutcome::Moved(shared) => Ok(FakeAgent {
                shared,
                agent_id: agent.agent_id,
            }),
        }
    }

    /// Shuts the grid down: the accept loop, every session pump, and every
    /// timer task exit.
    pub fn shutdown(&self) {
        self.core.shutdown_tx.send_replace(true);
    }
}

impl std::fmt::Debug for FakeGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeGrid")
            .field("login_uri", &self.core.login_uri)
            .finish_non_exhaustive()
    }
}

impl Drop for FakeGrid {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A handle onto one live session, for tests and tools to drive the grid
/// side of the conversation.
#[derive(Clone)]
pub struct FakeAgent {
    /// The session's shared driver handle.
    shared: SharedSim,
    /// The agent this session belongs to.
    agent_id: AgentKey,
}

impl std::fmt::Debug for FakeAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeAgent")
            .field("agent_id", &self.agent_id)
            .finish_non_exhaustive()
    }
}

impl FakeAgent {
    /// The agent id this session belongs to.
    #[must_use]
    pub const fn agent_id(&self) -> AgentKey {
        self.agent_id
    }

    /// The grid's current instant — what a `send_*` call driven from a test
    /// must stamp its message with, so a paused-clock grid is not handed a
    /// real-time deadline.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.shared.now()
    }

    /// Runs `f` against the live session machine, then flushes transmits,
    /// events, and wakeups — the only sanctioned way to call `send_*` /
    /// `set_*` / `enqueue_*` on it.
    pub async fn with_sim<R>(&self, f: impl FnOnce(&mut SimSession) -> R) -> R {
        self.shared.with_sim(f).await
    }

    /// Subscribes to the session's [`sl_proto::ServerEvent`] broadcast.
    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<sl_proto::ServerEvent> {
        self.shared.subscribe_events()
    }

    /// This session's sequence number.
    pub async fn session_seq(&self) -> u64 {
        self.shared.state.lock().await.seq
    }

    /// Whether the session has closed (logout, inactivity, or retirement
    /// after a teleport away).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }
}

#[cfg(test)]
mod test {
    use std::net::IpAddr;

    use pretty_assertions::assert_eq;
    use sl_wire::messages::{
        CompleteAgentMovement, CompleteAgentMovementAgentDataBlock, UseCircuitCode,
        UseCircuitCodeCircuitCodeBlock,
    };
    use sl_wire::{AnyMessage, PacketFlags, SequenceNumber, Writer, encode_datagram};

    use super::*;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// Encodes one client-direction message as an unreliable datagram.
    fn client_datagram(message: &AnyMessage, sequence: u32) -> Result<Vec<u8>, TestError> {
        let mut writer = Writer::new();
        message.id().encode(&mut writer);
        message.encode_body(&mut writer)?;
        Ok(encode_datagram(
            PacketFlags::EMPTY,
            SequenceNumber(sequence),
            &writer.into_bytes(),
        ))
    }

    /// Drives a prepared session's machine through the circuit handshake the
    /// way a client does, leaving it hosting the root agent.
    async fn root_the_agent(
        shared: &SharedSim,
        agent_id: AgentKey,
        ids: SessionIds,
    ) -> Result<(), TestError> {
        let from = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40_001);
        let open = AnyMessage::UseCircuitCode(UseCircuitCode {
            circuit_code: UseCircuitCodeCircuitCodeBlock {
                code: ids.circuit_code.0,
                session_id: ids.session_id,
                id: agent_id.uuid(),
            },
        });
        let complete = AnyMessage::CompleteAgentMovement(CompleteAgentMovement {
            agent_data: CompleteAgentMovementAgentDataBlock {
                agent_id: agent_id.uuid(),
                session_id: ids.session_id,
                circuit_code: ids.circuit_code.0,
            },
        });
        for (sequence, message) in [(1, open), (2, complete)] {
            let datagram = client_datagram(&message, sequence)?;
            shared
                .with_sim(|sim| sim.handle_datagram(from, &datagram, shared.now()))
                .await?;
        }
        Ok(())
    }

    /// A closed session stops being a candidate for a lure the moment it
    /// closes — its `agent_presence` still says root — and is pruned from the
    /// session table rather than leaking its socket and world for the life of
    /// the process.
    #[tokio::test]
    async fn a_closed_session_is_skipped_and_pruned() -> Result<(), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .start()
            .await?;
        let core = Arc::clone(&grid.core);
        let account = core.accounts.first().ok_or("no account")?.clone();
        let (prepared, _success) = core.prepare_session(&account, 0).await?;
        core.activate_session(&prepared).await;
        let ids = prepared.shared.state.lock().await.ids;
        root_the_agent(&prepared.shared, account.agent_id, ids).await?;

        assert!(
            core.root_session_of(account.agent_id).await.is_some(),
            "the arrived session hosts the agent"
        );

        prepared.shared.with_sim(SimSession::abandon).await;
        assert!(
            prepared.shared.is_closed(),
            "abandoning closes the session machine"
        );
        assert!(
            prepared.shared.state.lock().await.sim.is_root_agent(),
            "a closed session still reports itself a root agent, which is why \
             the closed check is needed",
        );
        assert!(
            core.root_session_of(account.agent_id).await.is_none(),
            "a closed session must never host a lure teleport"
        );

        for _ in 0..100_u32 {
            if core.session(prepared.seq).await.is_none() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err("the closed session was never pruned from the table".into())
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let account = AccountConfig::new("Test", "User", "pw");
        let result = check_duplicates(&[account.clone(), account], &[RegionConfig::default()]);
        assert!(matches!(
            result,
            Err(Error::Duplicate {
                kind: "account",
                ..
            })
        ));
        let result = check_duplicates(&[], &[RegionConfig::default(), RegionConfig::default()]);
        assert!(matches!(
            result,
            Err(Error::Duplicate { kind: "region", .. })
        ));
    }

    #[test]
    fn skeleton_derives_roots_and_folders() -> Result<(), Box<dyn std::error::Error>> {
        let mut tree = sl_proto::SimInventoryTree::default();
        tree.insert_folder(sl_proto::InventoryFolder {
            folder_id: sl_types::key::InventoryFolderKey::from(Uuid::from_u128(1)),
            parent_id: None,
            name: "Root".to_owned(),
            folder_type: 8,
            version: 1,
        });
        tree.insert_folder(sl_proto::InventoryFolder {
            folder_id: sl_types::key::InventoryFolderKey::from(Uuid::from_u128(2)),
            parent_id: Some(sl_types::key::InventoryFolderKey::from(Uuid::from_u128(1))),
            name: "Child".to_owned(),
            folder_type: -1,
            version: 3,
        });
        let (roots, skeleton) = skeleton_of(&tree);
        assert_eq!(roots.len(), 1);
        assert_eq!(skeleton.len(), 2);
        let child = skeleton
            .iter()
            .find(|folder| folder.name == "Child")
            .ok_or("child folder missing from the skeleton")?;
        assert_eq!(child.version, 3);
        Ok(())
    }
}
