//! Login, session driving, and the per-avatar aditi cooldown guard.
//!
//! Each test runs against a freshly logged-in [`Session`] (one or two of them).
//! [`login`] performs the XML-RPC login, answering an MFA challenge via the
//! avatar's `mfa_command`, and spawns the client run loop. [`TestContext`] hands
//! the live session(s) and a [`Metrics`] collector to the test body.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sl_client_tokio::{
    AgentKey, CircuitId, Client, ClientDirectories, Command, Diagnostic, Event, GroupKey,
    InventoryCacheConfig, LoginParams, LoginRejectKind, LoginRequest, MeshKey, RegionHandle,
    StartLocation,
};
use sl_repl::Avatar;
use time::format_description::well_known::Rfc3339;
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::fixtures::Fixtures;
use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::record::Completeness;

/// How long an aditi avatar must wait between logins, to avoid rate-limiting.
const ADITI_LOGIN_COOLDOWN: TimeDuration = TimeDuration::seconds(120);

/// How many times to retry an OpenSim login that was rejected as
/// "already logged in" before giving up. A prior session that did not log out
/// cleanly leaves a stale presence; the *rejected* attempt itself evicts that
/// ghost (OpenSim's login service marks the grid-user logged-out before
/// returning the rejection), so the next attempt normally succeeds. One retry
/// is plenty; the cap stops a genuinely-online duplicate from looping forever.
const ALREADY_LOGGED_IN_MAX_RETRIES: u8 = 2;

/// A short settle delay before retrying an "already logged in" OpenSim login, to
/// let the grid finish evicting the stale presence (god-kick to the last region
/// plus the grid-user logged-out write) before the next attempt.
const ALREADY_LOGGED_IN_RETRY_DELAY: Duration = Duration::from_secs(1);

/// How long [`Session::logout`] waits for a clean `LoggedOut` before forcing the
/// run loop down.
const LOGOUT_GRACE: Duration = Duration::from_secs(15);

/// A logged-in client session: the spawned run loop plus its event and command
/// channels.
#[derive(Debug)]
pub struct Session {
    /// The agent's own id, available after login.
    agent_id: Option<AgentKey>,
    /// The handle of the region the agent is currently in. Seeded from the login
    /// response and kept current as [`Session::wait_for`] observes region
    /// handovers ([`Event::RegionChanged`] from a teleport or border crossing), so
    /// a case pairs it with a region-local position to issue an intra-region
    /// [`Command::Teleport`] against wherever the agent now is.
    region_handle: Option<RegionHandle>,
    /// The identity of the current root circuit. Seeded from the login response
    /// and updated on each region handover ([`Event::RegionChanged`]) as
    /// [`Session::wait_for`] observes it, so a case pairs it with a region-local
    /// parcel/object id to build the `ScopedParcelId` / `ScopedObjectId` the
    /// scoped parcel/object commands take (e.g. the dwell request in
    /// `parcel-info-dwell`) for the region the agent is currently in.
    circuit_id: Option<CircuitId>,
    /// Inbound events from the run loop, after a forwarder has drained them off
    /// the run loop's bounded channel into this unbounded one. A case typically
    /// waits on one session at a time, leaving the others' event channels unread;
    /// because each runtime's run loop blocks while pushing an event onto a full
    /// channel, an unread bounded channel would stall that session's run loop
    /// (its queued commands never transmit, its incoming packets never decode).
    /// The forwarder keeps every session draining regardless of which one the
    /// case is currently reading, so no avatar can stall another.
    events: mpsc::UnboundedReceiver<Event>,
    /// Outbound commands to the run loop.
    commands: mpsc::Sender<Command>,
    /// Protocol diagnostics collected from the run loop's diagnostic channel,
    /// so a case can inspect anomalies (e.g. a missing `LogoutReply`) that are
    /// kept separate from [`Event`] and would otherwise only be logged.
    diagnostics: Arc<Mutex<Vec<Diagnostic>>>,
    /// The spawned `Client::run` task.
    run: JoinHandle<Result<(), sl_client_tokio::Error>>,
    /// The grid this session belongs to, retained so the session can reconnect
    /// the same avatar mid-case (see [`Session::relogin`]).
    grid: Grid,
    /// The avatar credentials, retained so [`Session::relogin`] can log the same
    /// account back in after a [`Session::disconnect`].
    avatar: Avatar,
    /// The viewer channel reported at login, retained for [`Session::relogin`].
    channel: String,
    /// The viewer version reported at login, retained for [`Session::relogin`].
    version: String,
    /// The `start` wire string this avatar logs in at (`"last"` for almost every
    /// case; a fixed `"uri:Region&x&y&z"` for cases that must be co-located with
    /// an in-world resource). Retained so [`Session::relogin`] lands the same
    /// place the initial login did.
    start_location: String,
    /// The harness state directory holding the per-avatar login-cooldown stamps,
    /// so [`Session::relogin`] can honour the aditi cooldown rather than bypass
    /// it (the initial logins are gated by the runner).
    state_dir: PathBuf,
    /// Whether to bypass the login cooldown (the runner's `--force`), threaded so
    /// [`Session::relogin`] makes the same choice as the initial login.
    force: bool,
    /// The per-account inventory disk-cache directory, or `None` to leave the
    /// inventory disk cache off (the default for every case). When `Some`, the
    /// runtime loads `<agent-uuid>.inv.llsd.gz` before the login skeleton and
    /// writes it back on logout, so a [`Session::relogin`] sees the cache the
    /// preceding [`Session::disconnect`] saved. Retained so the reconnection uses
    /// the same directory as the initial login (the `inventory-cache-skip` case).
    cache_dir: Option<PathBuf>,
    /// Whether the run loop is currently live. A [`Session::disconnect`] tears it
    /// down (the avatar goes offline on the grid) without discarding the identity
    /// needed to [`Session::relogin`].
    connected: bool,
    /// The region's capability map (name → URL), captured from the run loop's
    /// caps reporter and refreshed on every region change. A case reads a cap
    /// (e.g. `GetTexture`) from it to drive a `TextureStore`.
    caps: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl Session {
    /// The current region capability URL for `name` (e.g. `GetTexture`), or
    /// `None` if the caps have not arrived yet or the region does not offer it.
    #[must_use]
    pub fn cap(&self, name: &str) -> Option<String> {
        match self.caps.lock() {
            Ok(caps) => caps.get(name).cloned(),
            Err(poisoned) => poisoned.into_inner().get(name).cloned(),
        }
    }

    /// The agent's own id, if login reported one.
    #[must_use]
    pub const fn agent_id(&self) -> Option<AgentKey> {
        self.agent_id
    }

    /// The handle of the region the agent is currently in, if login reported one.
    /// Kept current across region handovers (teleport / crossing) as the case
    /// drives [`Session::wait_for`], so a case can target an intra-region
    /// [`Command::Teleport`] against the agent's present region.
    #[must_use]
    pub const fn region_handle(&self) -> Option<RegionHandle> {
        self.region_handle
    }

    /// The identity of the current root circuit, if known. Kept current across
    /// region handovers as the case drives [`Session::wait_for`], so a case pairs
    /// it with a region-local parcel/object id to build the `ScopedParcelId` a
    /// scoped parcel command (e.g. the dwell request) takes for the agent's
    /// present region.
    #[must_use]
    pub const fn circuit_id(&self) -> Option<CircuitId> {
        self.circuit_id
    }

    /// A snapshot of the protocol diagnostics seen so far on this session.
    ///
    /// Diagnostics are collected on a background task, so a case that has just
    /// observed the event a diagnostic accompanies (e.g. [`Event::LoggedOut`]
    /// after a logout that timed out) should allow a brief grace for the
    /// diagnostic to be recorded before reading them.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.lock().map_or_else(
            |poisoned| poisoned.into_inner().clone(),
            |guard| guard.clone(),
        )
    }

    /// Send a command to the run loop.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Disconnected`] if the run loop has stopped.
    pub async fn send(&self, command: Command) -> Result<(), TestFailure> {
        self.commands
            .send(command)
            .await
            .map_err(|_closed| TestFailure::Disconnected("command channel closed".to_owned()))
    }

    /// Await the first event for which `predicate` returns `Some`, up to
    /// `timeout`. An intervening `Disconnected` (unless the predicate consumes
    /// it) fails the wait.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Timeout`] if the timeout elapses,
    /// [`TestFailure::Disconnected`] if the session drops first.
    pub async fn wait_for<T, P>(
        &mut self,
        timeout: Duration,
        mut predicate: P,
    ) -> Result<T, TestFailure>
    where
        P: FnMut(&Event) -> Option<T>,
    {
        // Split the disjoint field borrows so the drain loop can keep the cached
        // region identity current while it reads events.
        let events = &mut self.events;
        let region_handle = &mut self.region_handle;
        let circuit_id = &mut self.circuit_id;
        let wait = async {
            loop {
                match events.recv().await {
                    None => {
                        return Err(TestFailure::Disconnected("event channel closed".to_owned()));
                    }
                    Some(event) => {
                        // A region handover (teleport or border crossing) moves the
                        // agent to a new region on a new root circuit. Track it as
                        // events flow so `region_handle()` / `circuit_id()` reflect
                        // where the agent is *now*, not merely where it logged in —
                        // updated before the predicate runs, so the very event that
                        // resolves the wait already sees the new region.
                        if let Event::RegionChanged {
                            region_handle: handle,
                            circuit,
                            ..
                        } = &event
                        {
                            *region_handle = Some(*handle);
                            *circuit_id = Some(*circuit);
                        }
                        if let Some(value) = predicate(&event) {
                            return Ok(value);
                        }
                        if let Event::Disconnected(reason) = &event {
                            return Err(TestFailure::Disconnected(format!("{reason:?}")));
                        }
                    }
                }
            }
        };
        match tokio::time::timeout(timeout, wait).await {
            Ok(result) => result,
            Err(_elapsed) => Err(TestFailure::Timeout(
                "timed out waiting for an expected event".to_owned(),
            )),
        }
    }

    /// Await the initial region becoming active (handshake complete or a region
    /// change), up to `timeout`.
    ///
    /// # Errors
    ///
    /// Propagates [`Session::wait_for`] errors.
    pub async fn wait_for_region(&mut self, timeout: Duration) -> Result<(), TestFailure> {
        self.wait_for(timeout, |event| {
            matches!(
                event,
                Event::RegionHandshakeComplete | Event::RegionChanged { .. }
            )
            .then_some(())
        })
        .await
    }

    /// Log out cleanly: request logout, wait briefly for `LoggedOut`, then join
    /// the run task.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Client`] if the run loop errored, or
    /// [`TestFailure::Join`] if the task panicked.
    pub async fn logout(mut self) -> Result<(), TestFailure> {
        self.commands.send(Command::Logout).await.ok();
        let _logged_out = self
            .wait_for(LOGOUT_GRACE, |event| {
                matches!(event, Event::LoggedOut).then_some(())
            })
            .await;
        drop(self.commands);
        match self.run.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(TestFailure::Client(error)),
            Err(join) => Err(TestFailure::Join(join.to_string())),
        }
    }

    /// Whether the run loop is currently live (i.e. the avatar is logged in).
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        self.connected
    }

    /// Log this avatar out and tear the run loop down *without* discarding the
    /// session — the identity needed to [`Session::relogin`] is kept, so a case
    /// can take an avatar offline (e.g. to make a peer's instant message go to
    /// offline storage) and bring the same account back later.
    ///
    /// Unlike [`Session::logout`], which consumes the session at the end of a
    /// run, this leaves a reusable but disconnected handle: sends fail and the
    /// event stream is empty until [`Session::relogin`].
    ///
    /// # Errors
    ///
    /// Never returns an error today (a failed logout is logged and the run loop
    /// is forced down regardless), but the signature is fallible for symmetry
    /// with [`Session::relogin`] and to allow stricter teardown later.
    pub async fn disconnect(&mut self) -> Result<(), TestFailure> {
        if !self.connected {
            return Ok(());
        }
        // Request a clean logout, then force the run loop down by closing the
        // command channel (mirrors `logout`), and join the task.
        self.commands.send(Command::Logout).await.ok();
        let _logged_out = self
            .wait_for(LOGOUT_GRACE, |event| {
                matches!(event, Event::LoggedOut).then_some(())
            })
            .await;
        // Replace the live command sender with a dead one (its receiver is
        // dropped immediately): this drops the only live sender, so the run
        // loop sees its command channel close and shuts down, and any later
        // `send` on this disconnected session fails cleanly.
        let (dead_tx, dead_rx) = mpsc::channel::<Command>(1);
        drop(dead_rx);
        self.commands = dead_tx;
        // Swap the real run task out for an already-finished placeholder so the
        // struct stays valid, then await the real one.
        let placeholder = tokio::spawn(async { Ok(()) });
        let run = std::mem::replace(&mut self.run, placeholder);
        match run.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("run loop error on disconnect: {error}"),
            Err(join) => tracing::warn!("run task join error on disconnect: {join}"),
        }
        // Replace the event receiver with a closed one (its sender is dropped),
        // so `wait_for` on a disconnected session returns immediately rather
        // than blocking until a timeout.
        let (_event_tx, event_rx) = mpsc::unbounded_channel::<Event>();
        self.events = event_rx;
        self.connected = false;
        Ok(())
    }

    /// Log the same avatar back in after a [`Session::disconnect`], replacing the
    /// run loop, channels, and login-derived identity in place.
    ///
    /// This performs a fresh XML-RPC login (answering MFA as
    /// [`login`] does) and, on OpenSim, inherits the "already logged in" retry
    /// that evicts any stale presence left by the preceding disconnect.
    ///
    /// On a grid that rate-limits logins (aditi) it first *waits out* the
    /// per-avatar login cooldown via [`wait_out_cooldown`] — the same guard the
    /// runner applies to the initial logins, but waited rather than failed, so an
    /// in-test relogin honours the rate limit instead of bypassing it. The
    /// runner's `--force`, threaded onto the session, skips the wait, mirroring
    /// the initial login; per the project rule, do not force aditi.
    ///
    /// # Errors
    ///
    /// Returns a [`TestFailure`] if the login fails (see [`login`]) or the
    /// cooldown stamp cannot be written.
    pub async fn relogin(&mut self) -> Result<(), TestFailure> {
        let grid = self.grid;
        let avatar = self.avatar.clone();
        let channel = self.channel.clone();
        let version = self.version.clone();
        let start_location = self.start_location.clone();
        let state_dir = self.state_dir.clone();
        let force = self.force;
        let cache_dir = self.cache_dir.clone();
        if grid.needs_cooldown() {
            let label = avatar_label(&avatar);
            wait_out_cooldown(&state_dir, &label, force).await?;
        }
        *self = connect_and_spawn(
            grid,
            &avatar,
            &channel,
            &version,
            &start_location,
            &state_dir,
            force,
            cache_dir,
        )
        .await?;
        Ok(())
    }
}

/// The stable per-avatar label used for cooldown stamps: the avatar's
/// `First Last` identity (matches the runner's labelling).
fn avatar_label(avatar: &Avatar) -> String {
    format!("{} {}", avatar.first(), avatar.last())
}

/// Log in to `grid` as `avatar`, answering any MFA challenge, and spawn the run
/// loop, returning the live [`Session`].
///
/// `start_location` is the `start` wire string the avatar logs in at (`"last"`
/// for almost every case; a fixed `"uri:Region&x&y&z"` for a case that must be
/// co-located with an in-world resource).
///
/// `cache_dir` is the per-account inventory disk-cache directory, or `None` to
/// leave the inventory disk cache off (what every case but `inventory-cache-skip`
/// passes). When `Some`, the runtime caches the agent's inventory tree there
/// across the session's [`Session::disconnect`]/[`Session::relogin`] cycle.
///
/// # Errors
///
/// Returns a [`TestFailure`] if the login URI is invalid, the start location
/// cannot be parsed, MFA is required but unavailable, or the login fails.
#[expect(
    clippy::too_many_arguments,
    reason = "the login parameters are all independent scalars threaded from the runner; \
              a wrapper struct would only relocate them without simplifying the call"
)]
pub async fn login(
    grid: Grid,
    avatar: &Avatar,
    channel: &str,
    version: &str,
    start_location: &str,
    state_dir: &Path,
    force: bool,
    cache_dir: Option<PathBuf>,
) -> Result<Session, TestFailure> {
    connect_and_spawn(
        grid,
        avatar,
        channel,
        version,
        start_location,
        state_dir,
        force,
        cache_dir,
    )
    .await
}

/// Perform the XML-RPC login, spawn the run loop and its drains, and assemble a
/// live [`Session`]. This is the shared core of [`login`] and
/// [`Session::relogin`].
///
/// `state_dir` and `force` are retained on the returned session so a later
/// [`Session::relogin`] can honour the aditi login cooldown. This function does
/// not itself enforce the cooldown — the runner gates the initial logins and
/// [`Session::relogin`] waits it out for reconnections.
///
/// # Errors
///
/// Returns a [`TestFailure`] if the login URI is invalid, the start location
/// cannot be parsed, MFA is required but unavailable, or the login fails.
#[expect(
    clippy::too_many_arguments,
    reason = "the login parameters are all independent scalars threaded from the runner; \
              a wrapper struct would only relocate them without simplifying the call"
)]
async fn connect_and_spawn(
    grid: Grid,
    avatar: &Avatar,
    channel: &str,
    version: &str,
    start_location: &str,
    state_dir: &Path,
    force: bool,
    cache_dir: Option<PathBuf>,
) -> Result<Session, TestFailure> {
    let login_uri_text = avatar
        .login_uri()
        .map_or_else(|| grid.default_login_uri().to_owned(), str::to_owned);
    let login_uri: url::Url = login_uri_text
        .parse()
        .map_err(|error: url::ParseError| TestFailure::Login(error.to_string()))?;
    let start: StartLocation =
        start_location
            .parse()
            .map_err(|error: sl_client_tokio::StartLocationParseError| {
                TestFailure::Login(error.to_string())
            })?;
    let mut request = LoginRequest::new(
        avatar.first().to_owned(),
        avatar.last().to_owned(),
        avatar.password().expose().to_owned(),
        start,
        channel.to_owned(),
        version.to_owned(),
    );
    let mut already_logged_in_retries: u8 = 0;
    let mut client = loop {
        let params = LoginParams {
            login_uri: login_uri.clone(),
            request: request.clone(),
        };
        match Client::connect(params).await {
            Ok(client) => break client,
            Err(sl_client_tokio::Error::MfaChallenge(challenge)) => {
                tracing::info!(
                    "multi-factor authentication required: {}",
                    challenge.message
                );
                let token = avatar
                    .acquire_mfa()
                    .map_err(|error| TestFailure::Auth(error.to_string()))?
                    .ok_or(TestFailure::MfaRequired)?;
                request = request.with_mfa(token.expose(), challenge.mfa_hash);
            }
            // A stale presence from a prior session that did not log out cleanly
            // (the OpenSim no-`LogoutReply` quirk) rejects the next login as
            // "already logged in" — but that rejected attempt evicts the ghost,
            // so a retry succeeds. Only OpenSim: Second Life may flag rapid
            // repeated login attempts as suspicious, so there we surface the
            // rejection unchanged rather than retrying.
            Err(sl_client_tokio::Error::LoginRejected {
                kind: LoginRejectKind::AlreadyLoggedIn,
                reason,
                message,
            }) if grid == Grid::Opensim
                && already_logged_in_retries < ALREADY_LOGGED_IN_MAX_RETRIES =>
            {
                already_logged_in_retries = already_logged_in_retries.saturating_add(1);
                tracing::warn!(
                    "login rejected as already-logged-in ({reason}: {message}); the rejected \
                     attempt evicts the stale presence — retrying (attempt {})",
                    already_logged_in_retries.saturating_add(1)
                );
                tokio::time::sleep(ALREADY_LOGGED_IN_RETRY_DELAY).await;
            }
            Err(other) => return Err(TestFailure::Client(other)),
        }
    };

    // Enable diagnostics so a case can observe protocol anomalies (e.g. a
    // logout that never received its `LogoutReply`); they are off by default.
    client.set_diagnostics(true);

    // Enable the inventory disk cache when the case asked for one (only
    // `inventory-cache-skip` does). The runtime then loads the cache before the
    // login skeleton and reconciles it, so version-matching folders stay loaded
    // across a relogin instead of being refetched.
    if let Some(dir) = cache_dir.as_ref() {
        client.set_inventory_cache_config(InventoryCacheConfig {
            enabled: true,
            ..InventoryCacheConfig::default()
        });
        client.set_directories(ClientDirectories {
            agent_cache_dir: Some(dir.clone()),
            ..ClientDirectories::default()
        });
    }

    // Capture the region capability map so a case can drive a TextureStore off
    // the live `GetTexture` cap. The reporter fires at startup and each region
    // change; a drain keeps the shared map current.
    let caps = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let (caps_tx, mut caps_rx) = mpsc::channel::<std::collections::HashMap<String, String>>(4);
    client.set_caps_reporter(caps_tx);
    let caps_sink = Arc::clone(&caps);
    let _caps_drain = tokio::spawn(async move {
        while let Some(map) = caps_rx.recv().await {
            match caps_sink.lock() {
                Ok(mut current) => *current = map,
                Err(poisoned) => *poisoned.into_inner() = map,
            }
        }
    });

    let agent_id = client.agent_id();
    let region_handle = client.region_handle();
    let circuit_id = client.root_circuit_id();
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, mut diag_rx) = mpsc::channel::<Diagnostic>(64);
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let diag_sink = Arc::clone(&diagnostics);
    // Drain diagnostics to the log (so a full channel never stalls the run
    // loop) and into the shared buffer a case can inspect.
    let _drain = tokio::spawn(async move {
        while let Some(diagnostic) = diag_rx.recv().await {
            tracing::debug!("diagnostic: {diagnostic:?}");
            match diag_sink.lock() {
                Ok(mut buffer) => buffer.push(diagnostic),
                Err(poisoned) => poisoned.into_inner().push(diagnostic),
            }
        }
    });
    // Forward the run loop's bounded event channel into an unbounded one,
    // continuously, so the run loop never blocks pushing an event even while the
    // case is reading a *different* session. Without this, any session whose
    // events go unread (the non-awaited avatar in a multi-avatar case) stalls its
    // run loop once its 256-slot channel fills, freezing its command transmission
    // and packet decoding until the case happens to read it. Mirrors the
    // diagnostic-channel drain above.
    let (events_tx, events_rx) = mpsc::unbounded_channel::<Event>();
    let _forward = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if events_tx.send(event).is_err() {
                break;
            }
        }
    });
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));
    Ok(Session {
        agent_id,
        region_handle,
        circuit_id,
        events: events_rx,
        commands: command_tx,
        diagnostics,
        run,
        grid,
        avatar: avatar.clone(),
        channel: channel.to_owned(),
        version: version.to_owned(),
        start_location: start_location.to_owned(),
        state_dir: state_dir.to_path_buf(),
        force,
        cache_dir,
        connected: true,
        caps,
    })
}

/// The live session(s) and metrics collector handed to a test body.
#[expect(
    clippy::module_name_repetitions,
    reason = "`TestContext` is the established public name for this type"
)]
#[derive(Debug)]
pub struct TestContext {
    /// The grid under test.
    grid: Grid,
    /// The primary logged-in session.
    primary: Session,
    /// The secondary session, for two-account tests.
    secondary: Option<Session>,
    /// The tertiary session, for three-account tests.
    tertiary: Option<Session>,
    /// The metrics the test writes.
    metrics: Metrics,
    /// Environment-specific fixtures (e.g. a pre-made group) for this grid.
    fixtures: Fixtures,
    /// Whether the test declared its run complete or partial.
    completeness: Completeness,
    /// The note explaining a partial run.
    completeness_note: Option<String>,
}

impl TestContext {
    /// Build a context around the given live session(s) and grid fixtures.
    #[must_use]
    pub fn new(
        grid: Grid,
        primary: Session,
        secondary: Option<Session>,
        tertiary: Option<Session>,
        fixtures: Fixtures,
    ) -> Self {
        Self {
            grid,
            primary,
            secondary,
            tertiary,
            metrics: Metrics::new(),
            fixtures,
            completeness: Completeness::Complete,
            completeness_note: None,
        }
    }

    /// The grid under test.
    #[must_use]
    pub const fn grid(&self) -> Grid {
        self.grid
    }

    /// The `index`-th pre-made group configured for this grid, if any. When
    /// present, the group cases reuse it (by position) instead of creating a
    /// throwaway group per run (see [`crate::fixtures`] for why this matters on
    /// Second Life).
    #[must_use]
    pub fn premade_group(&self, index: usize) -> Option<GroupKey> {
        self.fixtures.premade_group(index)
    }

    /// The configured second avatar whose profile the `avatar-properties` case
    /// reads, if any. Needed only on Second Life (which has no built-in second
    /// avatar); OpenSim falls back to the local secondary test avatar.
    #[must_use]
    pub const fn other_avatar(&self) -> Option<AgentKey> {
        self.fixtures.other_avatar()
    }

    /// The configured fetchable mesh asset the `mesh-fetch-http` case pulls, if
    /// any. When absent the case scans the region's object stream for a
    /// mesh-shaped prim instead.
    #[must_use]
    pub const fn mesh_asset(&self) -> Option<MeshKey> {
        self.fixtures.mesh_asset()
    }

    /// The primary session.
    pub const fn primary(&mut self) -> &mut Session {
        &mut self.primary
    }

    /// The secondary session, if this is a two-account test.
    pub const fn secondary(&mut self) -> Option<&mut Session> {
        self.secondary.as_mut()
    }

    /// The tertiary session, if this is a three-account test.
    pub const fn tertiary(&mut self) -> Option<&mut Session> {
        self.tertiary.as_mut()
    }

    /// The metrics collector to record measurements into.
    pub const fn metrics(&mut self) -> &mut Metrics {
        &mut self.metrics
    }

    /// Mark the run as partial (truncated or aborted), with a reason; the
    /// reporter will then not compare its counts against a complete run's.
    pub fn mark_partial(&mut self, reason: &str) {
        self.completeness = Completeness::Partial;
        self.completeness_note = Some(reason.to_owned());
    }

    /// Decompose the context into its parts for the runner: the metrics,
    /// completeness, note, and the session(s) to log out.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        Metrics,
        Completeness,
        Option<String>,
        Session,
        Option<Session>,
        Option<Session>,
    ) {
        (
            self.metrics,
            self.completeness,
            self.completeness_note,
            self.primary,
            self.secondary,
            self.tertiary,
        )
    }
}

/// Sanitize an avatar label into a filesystem-safe stem for its cooldown file.
fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// The cooldown timestamp file for an avatar under the state directory.
#[must_use]
pub fn cooldown_path(state_dir: &Path, avatar_label: &str) -> PathBuf {
    state_dir
        .join("aditi-last-login")
        .join(format!("{}.timestamp", sanitize_label(avatar_label)))
}

/// Enforce, then refresh, the aditi login cooldown for `avatar_label`.
///
/// When `force` is false and the last login for this avatar was within the
/// `ADITI_LOGIN_COOLDOWN` window, returns [`TestFailure::Cooldown`]. Otherwise
/// stamps the current time and returns `Ok(())`.
///
/// # Errors
///
/// Returns [`TestFailure::Cooldown`] if the cooldown is active, or
/// [`TestFailure::State`] if the timestamp cannot be written.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "wall-clock instant/duration subtraction here cannot overflow in practice"
)]
pub fn enforce_cooldown(
    state_dir: &Path,
    avatar_label: &str,
    force: bool,
) -> Result<(), TestFailure> {
    let path = cooldown_path(state_dir, avatar_label);
    if !force
        && let Ok(text) = fs_err::read_to_string(&path)
        && let Ok(previous) = OffsetDateTime::parse(text.trim(), &Rfc3339)
    {
        let elapsed = OffsetDateTime::now_utc() - previous;
        if elapsed < ADITI_LOGIN_COOLDOWN {
            let remaining = (ADITI_LOGIN_COOLDOWN - elapsed).whole_seconds().max(0);
            return Err(TestFailure::Cooldown {
                avatar: avatar_label.to_owned(),
                remaining_secs: remaining,
            });
        }
    }
    stamp_login(&path)
}

/// Wait out, then refresh, the aditi login cooldown for `avatar_label`.
///
/// Unlike [`enforce_cooldown`], which *fails* when the cooldown is still active,
/// this *sleeps* the remaining window and then proceeds — so an in-test
/// reconnection ([`Session::relogin`]) honours the rate limit instead of either
/// bypassing it or aborting the run. When `force` is true the wait is skipped
/// (mirroring the initial login); per the project rule, do not force aditi.
///
/// # Errors
///
/// Returns [`TestFailure::State`] if the timestamp cannot be written.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "wall-clock instant/duration subtraction here cannot overflow in practice"
)]
pub async fn wait_out_cooldown(
    state_dir: &Path,
    avatar_label: &str,
    force: bool,
) -> Result<(), TestFailure> {
    let path = cooldown_path(state_dir, avatar_label);
    if !force
        && let Ok(text) = fs_err::read_to_string(&path)
        && let Ok(previous) = OffsetDateTime::parse(text.trim(), &Rfc3339)
    {
        let elapsed = OffsetDateTime::now_utc() - previous;
        if elapsed < ADITI_LOGIN_COOLDOWN {
            // Add a one-second margin so the next stamp is unambiguously past the
            // window, then sleep.
            let remaining = (ADITI_LOGIN_COOLDOWN - elapsed).whole_seconds().max(0);
            let secs = u64::try_from(remaining).unwrap_or(0).saturating_add(1);
            tracing::info!("waiting out aditi login cooldown for {avatar_label}: {secs}s");
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
    }
    stamp_login(&path)
}

/// Write the current time as the last-login stamp at `path`, creating the parent
/// directory if needed. Shared by [`enforce_cooldown`] and [`wait_out_cooldown`].
///
/// # Errors
///
/// Returns [`TestFailure::State`] if the directory or file cannot be written.
fn stamp_login(path: &Path) -> Result<(), TestFailure> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent).map_err(|error| TestFailure::State(error.to_string()))?;
    }
    let stamp = now_rfc3339()?;
    fs_err::write(path, stamp).map_err(|error| TestFailure::State(error.to_string()))?;
    Ok(())
}

/// The current UTC time as an RFC 3339 string.
///
/// # Errors
///
/// Returns [`TestFailure::State`] if the timestamp cannot be formatted.
pub fn now_rfc3339() -> Result<String, TestFailure> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| TestFailure::State(error.to_string()))
}

/// A test failure: any reason a conformance test did not pass.
#[derive(Debug, thiserror::Error)]
pub enum TestFailure {
    /// The login could not be performed.
    #[error("login error: {0}")]
    Login(String),
    /// Acquiring an MFA token failed.
    #[error("MFA error: {0}")]
    Auth(String),
    /// The grid required MFA but the avatar has no `mfa_command`.
    #[error("multi-factor authentication required but no mfa_command configured")]
    MfaRequired,
    /// A wait for an expected event timed out.
    #[error("{0}")]
    Timeout(String),
    /// The session disconnected unexpectedly.
    #[error("disconnected: {0}")]
    Disconnected(String),
    /// An assertion in the test body did not hold.
    #[error("{0}")]
    Assertion(String),
    /// The underlying client errored.
    #[error("client error: {0}")]
    Client(#[from] sl_client_tokio::Error),
    /// The run task panicked.
    #[error("run task join error: {0}")]
    Join(String),
    /// The aditi login cooldown is still active for this avatar.
    #[error(
        "aditi cooldown active for {avatar}: {remaining_secs}s remaining (use --force to override)"
    )]
    Cooldown {
        /// The avatar still cooling down.
        avatar: String,
        /// Seconds remaining before another login is allowed.
        remaining_secs: i64,
    },
    /// Local harness state (cooldown stamp) could not be read or written.
    #[error("harness state error: {0}")]
    State(String),
}

#[cfg(test)]
mod tests {
    use super::{cooldown_path, enforce_cooldown, sanitize_label, wait_out_cooldown};
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// A process-unique scratch directory for a cooldown test, removed on drop.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        /// Create a fresh, empty scratch directory keyed by test name and pid.
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sl-conformance-{name}-{}", std::process::id()));
            let _removed = fs_err::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _removed = fs_err::remove_dir_all(&self.0);
        }
    }

    /// Labels are sanitised into filesystem-safe stems.
    #[test]
    fn label_sanitisation() {
        assert_eq!(sanitize_label("primary"), "primary");
        assert_eq!(sanitize_label("Alice Resident"), "Alice_Resident");
        assert_eq!(sanitize_label("a/b:c"), "a_b_c");
    }

    /// The cooldown path nests under the state dir by avatar.
    #[test]
    fn cooldown_path_layout() {
        let path = cooldown_path(Path::new("/state"), "primary");
        assert!(path.ends_with("aditi-last-login/primary.timestamp"));
    }

    /// With no prior stamp there is nothing to wait for: `wait_out_cooldown`
    /// returns promptly and records a fresh stamp.
    #[tokio::test]
    async fn wait_out_cooldown_is_immediate_without_a_prior_stamp() {
        let scratch = ScratchDir::new("wait-noprior");
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            wait_out_cooldown(&scratch.0, "primary", false),
        )
        .await;
        assert!(matches!(result, Ok(Ok(()))), "should not wait or error");
        assert!(
            cooldown_path(&scratch.0, "primary").exists(),
            "a fresh login stamp should be written"
        );
    }

    /// `force` skips the wait even when a stamp was just written (an un-forced
    /// call would otherwise block for the full cooldown window).
    #[tokio::test]
    async fn wait_out_cooldown_force_skips_the_wait() {
        let scratch = ScratchDir::new("wait-force");
        assert!(
            enforce_cooldown(&scratch.0, "primary", false).is_ok(),
            "initial stamp should be written"
        );
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            wait_out_cooldown(&scratch.0, "primary", true),
        )
        .await;
        assert!(
            matches!(result, Ok(Ok(()))),
            "force should return without waiting out the cooldown"
        );
    }
}
