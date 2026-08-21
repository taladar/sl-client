//! The grid orchestrator: the builder, the running [`FakeGrid`], and the
//! per-login session bring-up shared by the HTTP endpoints.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use sl_proto::{
    DEFAULT_TERRAIN_DETAIL_TEXTURES, Maturity, OpenSimExtras, ProductType, RegionHandle,
    RegionIdentity, RegionTerrainComposition, SimSession, SimulatorFeatures, Uuid,
    region_name_from_wire,
};
use sl_types::key::AgentKey;
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
    /// A scenario overriding the grid-wide one for this region.
    pub scenario: Option<Scenario>,
}

impl Default for RegionConfig {
    /// A general-rated 256 m region called "Fake Region" at grid
    /// `(1000, 1000)` with the stock water height.
    fn default() -> Self {
        Self {
            name: "Fake Region".to_owned(),
            grid_x: 1000,
            grid_y: 1000,
            region_id: None,
            maturity: Maturity::Pg,
            water_height: 20.0,
            scenario: None,
        }
    }
}

/// A registered region after the grid started: its config, minted id, and
/// effective scenario.
pub(crate) struct RegionEntry {
    /// The builder-supplied region data.
    config: RegionConfig,
    /// The region id (fixed or minted once at start).
    region_id: Uuid,
    /// The scenario sessions in this region are seeded with.
    scenario: Scenario,
}

impl RegionEntry {
    /// The region handle derived from the grid coordinates.
    fn handle(&self) -> RegionHandle {
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
            terrain: RegionTerrainComposition {
                detail_textures: DEFAULT_TERRAIN_DETAIL_TEXTURES,
                start_heights: [10.0; 4],
                height_ranges: [60.0; 4],
            },
        }
    }
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
}

/// Everything a successful login mints before the response is built. The
/// enriched [`LoginSuccess`] is returned alongside (the login server
/// consumes it), so this struct only carries what activation needs.
pub(crate) struct PreparedSession {
    /// The session's sequence number (also its CAPS path component).
    pub(crate) seq: u64,
    /// The shared driver handle (not yet registered or pumped).
    pub(crate) shared: SharedSim,
    /// The region logged into (for the login notice).
    pub(crate) region_name: String,
}

impl GridCore {
    /// The region an account starts in.
    pub(crate) fn start_region(&self, account: &Account) -> Option<&RegionEntry> {
        match &account.config.start_region {
            Some(name) => self.regions.iter().find(|entry| entry.config.name == *name),
            None => self.regions.first(),
        }
    }

    /// Creates the session machinery for a login: binds the session's UDP
    /// socket, seeds a fresh [`SimSession`] with the region's scenario, and
    /// builds the enriched [`LoginSuccess`]. Nothing is registered yet — the
    /// caller only activates the session when the login server actually
    /// answers success.
    pub(crate) async fn prepare_session(
        self: &Arc<Self>,
        account: &Account,
        region: &RegionEntry,
    ) -> Result<(PreparedSession, Box<LoginSuccess>), Error> {
        let socket = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?);
        let udp_port = socket.local_addr()?.port();
        let seq = self.next_session.fetch_add(1, Ordering::Relaxed);

        let mut sim = SimSession::new(region.handle(), Instant::now());
        let secure_session_id = Uuid::new_v4();
        sim.set_secure_session_id(secure_session_id);
        sim.set_region_id(region.region_id);
        (region.scenario.setup)(&mut sim);
        region.scenario.udp_assets.register_xfer_files(&mut sim);
        if *sim.simulator_features() == SimulatorFeatures::default() {
            // The scenario left the feature document untouched: advertise
            // the grid-wide URLs (map tiles, currency helper) the way an
            // OpenSim region does through `OpenSimExtras`.
            sim.set_simulator_features(self.simulator_features());
        }

        let base_url: url::Url =
            format!("http://127.0.0.1:{}/sim/{seq}", self.http_port).parse()?;
        let caps = sl_proto::SimCaps::new(base_url, Uuid::new_v4(), Uuid::new_v4);
        let seed_url = caps.seed_url();

        let session_id = Uuid::new_v4();
        let circuit_code = sl_wire::CircuitCode(rand_circuit_code());
        let mut success = Box::new(LoginSuccess::minimal(
            account.agent_id,
            session_id,
            secure_session_id,
            circuit_code,
            Ipv4Addr::LOCALHOST,
            udp_port,
            seed_url,
        ));
        enrich_success(&mut success, account, region, &sim);
        success.message = Some(self.identity.message.clone());
        success.map_server_url = Some(self.login_uri.clone());
        success.currency = Some(self.economy.currency_symbol.clone());

        let state = SimState {
            sim,
            caps,
            assets: region.scenario.assets.clone(),
            identity: region.identity(),
            on_agent_arrived: region.scenario.on_agent_arrived.clone(),
            on_event: region.scenario.on_event.clone(),
            udp_assets: region.scenario.udp_assets.clone(),
        };
        let shared = new_shared_sim(state, socket, self.shutdown_tx.subscribe());
        Ok((
            PreparedSession {
                seq,
                shared,
                region_name: region.config.name.clone(),
            },
            success,
        ))
    }

    /// Registers a prepared session and starts its pump and timer tasks —
    /// called only once the login server answered success.
    pub(crate) async fn activate_session(&self, prepared: &PreparedSession) {
        self.sessions
            .lock()
            .await
            .insert(prepared.seq, prepared.shared.clone());
        tokio::spawn(run_udp_pump(prepared.shared.clone()));
        tokio::spawn(run_timer(prepared.shared.clone()));
    }

    /// Looks a live session up by its sequence number.
    pub(crate) async fn session(&self, seq: u64) -> Option<SharedSim> {
        self.sessions.lock().await.get(&seq).cloned()
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

/// Mints a random non-zero circuit code.
fn rand_circuit_code() -> u32 {
    // Uuid::new_v4 is the crate's only randomness source; fold its low bits.
    let low = Uuid::new_v4().as_u128() & u128::from(u32::MAX);
    u32::try_from(low).unwrap_or(1).max(1)
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
#[derive(Debug)]
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
}

impl Default for FakeGridBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeGridBuilder {
    /// A builder with no accounts, no regions, the stock scenario, no login
    /// gates, and a 30 s event-queue hold on an ephemeral port.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            regions: Vec::new(),
            scenario: Scenario::default(),
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
        let mut map_tiles = self.map_tiles;
        for region in &self.regions {
            map_tiles.seed_region(region.grid_x, region.grid_y);
        }
        let grid_info = grid_info_of(&self.identity, &login_uri);

        let regions = self
            .regions
            .into_iter()
            .map(|config| {
                let region_id = config.region_id.unwrap_or_else(Uuid::new_v4);
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
            accounts: self.accounts.into_iter().map(Account::register).collect(),
            regions,
            gates: self.gates,
            honor_options: self.honor_options,
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
        self.core
            .session(notice.session_seq)
            .await
            .map(|shared| FakeAgent {
                shared,
                agent_id: notice.agent_id,
            })
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
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

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
