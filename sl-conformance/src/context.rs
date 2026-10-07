//! Login, session driving, and the per-avatar aditi cooldown guard.
//!
//! Each test runs against a freshly logged-in [`Session`] (one or two of them).
//! [`login`] performs the XML-RPC login, answering an MFA challenge via the
//! avatar's `mfa_command`, and spawns the client run loop. [`TestContext`] hands
//! the live session(s) and a [`Metrics`] collector to the test body.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sl_client_tokio::{
    AgentKey, CircuitId, CircuitProbe, Client, ClientDirectories, Command, Diagnostic, Event,
    ExperienceKey, GroupKey, InventoryCacheConfig, LoginAccount, LoginParams, LoginRejectKind,
    LoginRequest, MeshKey, RegionHandle, StartLocation, Uuid,
};
use sl_repl::{Avatar, CooldownError, LoginCooldown};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::fixtures::Fixtures;
use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::record::Completeness;

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

/// A cloned handle to a session's command channel that can be used where
/// `async` is not available.
///
/// The case that matters is a `Drop` guard restoring grid state its case
/// mutated: when the case body is cancelled (the runner's overall timeout) or
/// unwinds, the guard runs with no executor to await on and no session left to
/// borrow. The command it queues is picked up by the still-running run loop and
/// goes out ahead of the runner's logout, which is queued after it.
#[derive(Clone, Debug)]
pub struct Commander {
    /// Outbound commands to the run loop, cloned from the session.
    commands: mpsc::Sender<Command>,
}

impl Commander {
    /// Queue `command` for the run loop without awaiting, reporting whether it
    /// was accepted (`false` when the run loop has stopped or its queue is
    /// full).
    #[must_use]
    pub fn try_send(&self, command: Command) -> bool {
        self.commands.try_send(command).is_ok()
    }
}

/// A logged-in client session: the spawned run loop plus its event and command
/// channels.
#[derive(Debug)]
pub struct Session {
    /// The agent's own id, available after login.
    agent_id: Option<AgentKey>,
    /// What the login response said the account is entitled to, available after
    /// login. Captured here rather than read off [`Event::Account`] because a
    /// case that waits for anything else first — a region handshake, say —
    /// discards that event on the way past, and a case that has to know an
    /// upload's price before it sends the upload cannot afford to have missed
    /// it.
    login_account: Option<LoginAccount>,
    /// The whole parsed login response, captured at connect for the same
    /// reason: every section a grid sent, which `login-options` records.
    login_success: Option<Box<sl_client_tokio::LoginSuccess>>,
    /// The agent's login session id, available after login. Needed by a case
    /// that hand-builds a raw wire message for
    /// [`Command::Send`] (most messages carry an
    /// `AgentData` block of agent id + session id the simulator validates).
    login_session_id: Option<Uuid>,
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
    /// The per-avatar login-cooldown stamps, so [`Session::relogin`] can honour
    /// the aditi cooldown rather than bypass it (the initial logins are gated
    /// by the runner).
    cooldown: LoginCooldown,
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
    /// The login request's `options` list, or `None` for the client's default.
    /// Retained so a [`Session::relogin`] asks for the same fields; the
    /// `login-options` case changes it between logins with
    /// [`Session::relogin_with_options`].
    options: Option<Vec<String>>,
    /// The capability names the seed requests ask for, or `None` for the
    /// client's default list. Retained like [`options`](Self::options); the
    /// `seed-capabilities` case changes it with
    /// [`Session::relogin_requesting_capabilities`].
    capabilities: Option<Vec<String>>,
    /// Each neighbour region's capability map as its seed answered, keyed by
    /// the neighbour's simulator address.
    neighbour_caps: Arc<Mutex<NeighbourCapabilityMaps>>,
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

    /// The names of every capability the current region granted.
    #[must_use]
    pub fn capability_names(&self) -> std::collections::BTreeSet<String> {
        let caps = self
            .caps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        caps.keys().cloned().collect()
    }

    /// The names of every capability each neighbour region granted, by the
    /// neighbour's simulator address — empty until a neighbour's seed answers.
    #[must_use]
    pub fn neighbour_capability_names(
        &self,
    ) -> Vec<(SocketAddr, std::collections::BTreeSet<String>)> {
        let maps = self
            .neighbour_caps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        maps.iter()
            .map(|(sim, map)| (*sim, map.keys().cloned().collect()))
            .collect()
    }

    /// The agent's own id, if login reported one.
    #[must_use]
    pub const fn agent_id(&self) -> Option<AgentKey> {
        self.agent_id
    }

    /// What the login response said the account is entitled to, if it said
    /// anything: the maturity trio, the package name, and — on Second Life —
    /// the benefits package holding the upload prices the grid charges.
    ///
    /// A stock OpenSim login carries no benefits package at all, so a case that
    /// needs a price from it must have an answer for `None` beyond failing.
    #[must_use]
    pub const fn login_account(&self) -> Option<&LoginAccount> {
        self.login_account.as_ref()
    }

    /// The whole parsed login response, if the login produced one.
    #[must_use]
    pub fn login_success(&self) -> Option<&sl_client_tokio::LoginSuccess> {
        self.login_success.as_deref()
    }

    /// The agent's login session id, if login reported one. Pairs with
    /// [`Session::agent_id`] to fill the `AgentData` block of a hand-built wire
    /// message sent via [`Command::Send`].
    #[must_use]
    pub const fn session_id(&self) -> Option<Uuid> {
        self.login_session_id
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

    /// The avatar's own first (account) name, from the login credentials. Cases
    /// that must supply a real, grid-present name at runtime — without baking an
    /// avatar name into the source or the record — derive their query from this
    /// (e.g. the `avatar-picker` name autocomplete searches for the agent's own
    /// name, which the grid is guaranteed to hold).
    #[must_use]
    pub fn avatar_first_name(&self) -> &str {
        self.avatar.first()
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

    /// A cloned handle to this session's command channel, for the one job
    /// [`Session::send`] cannot do: getting a last command out from a `Drop`
    /// guard, where there is no `await` and no borrow of the session left.
    #[must_use]
    pub fn commander(&self) -> Commander {
        Commander {
            commands: self.commands.clone(),
        }
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
        let options = self.options.clone();
        let capabilities = self.capabilities.clone();
        self.relogin_with(options, capabilities).await
    }

    /// [`Session::relogin`], asking every seed for `capabilities` (or the
    /// client's default list for `None`) — how a case surveys which
    /// capabilities a grid grants.
    ///
    /// # Errors
    ///
    /// As [`Session::relogin`].
    pub async fn relogin_requesting_capabilities(
        &mut self,
        capabilities: Option<Vec<String>>,
    ) -> Result<(), TestFailure> {
        let options = self.options.clone();
        self.relogin_with(options, capabilities).await
    }

    /// [`Session::relogin`], asking for `options` (or the client's default list
    /// for `None`) instead of what the previous login asked for — which is how
    /// a case maps which fields each option gates.
    ///
    /// # Errors
    ///
    /// As [`Session::relogin`].
    pub async fn relogin_with_options(
        &mut self,
        options: Option<Vec<String>>,
    ) -> Result<(), TestFailure> {
        let capabilities = self.capabilities.clone();
        self.relogin_with(options, capabilities).await
    }

    /// The relogin behind the three public forms, with both overrides.
    async fn relogin_with(
        &mut self,
        options: Option<Vec<String>>,
        capabilities: Option<Vec<String>>,
    ) -> Result<(), TestFailure> {
        let grid = self.grid;
        let avatar = self.avatar.clone();
        let channel = self.channel.clone();
        let version = self.version.clone();
        let start_location = self.start_location.clone();
        let cooldown = self.cooldown.clone();
        let force = self.force;
        let cache_dir = self.cache_dir.clone();
        if grid.needs_cooldown() {
            let label = avatar_label(&avatar);
            wait_out_cooldown(&cooldown, &label, force).await?;
        }
        *self = connect_and_spawn(LoginSpec {
            grid,
            avatar: &avatar,
            channel: &channel,
            version: &version,
            start_location: &start_location,
            cooldown: &cooldown,
            force,
            cache_dir,
            probe: CircuitProbe::Off,
            options,
            capabilities,
        })
        .await?;
        Ok(())
    }

    /// One login attempt in this session's avatar's name, made **beside** the
    /// session rather than in place of it — which is how a case asks a grid
    /// what it refuses: a wrong password, a name it has never heard of, a
    /// second login of an avatar that is already in world.
    ///
    /// The attempt is exactly one request and is never retried: a challenge or
    /// a refusal is the answer, handed back whole. On a grid that rate-limits
    /// logins the per-avatar cooldown is waited out first, as
    /// [`Session::relogin`] does, except for an attempt that
    /// [answers a challenge](LoginAttempt::answers_challenge) — the second half
    /// of one login, which a viewer sends at once.
    ///
    /// # Errors
    ///
    /// Returns a [`TestFailure`] if the request cannot be built, the cooldown
    /// stamp cannot be written, or the attempt fails for a reason that is not
    /// the grid's answer (HTTP, an unparsable response, the circuit).
    pub async fn attempt_login(&self, attempt: &LoginAttempt) -> Result<LoginAnswer, TestFailure> {
        let spec = LoginSpec {
            grid: self.grid,
            avatar: &self.avatar,
            channel: &self.channel,
            version: &self.version,
            start_location: attempt.start.as_deref().unwrap_or(&self.start_location),
            cooldown: &self.cooldown,
            force: self.force,
            cache_dir: None,
            probe: CircuitProbe::Off,
            options: self.options.clone(),
            capabilities: self.capabilities.clone(),
        };
        let (login_uri, mut request) = login_request(&spec)?;
        if let Some((first, last)) = &attempt.name {
            request.first_name.clone_from(first);
            request.last_name.clone_from(last);
        }
        if let Some(password) = &attempt.password {
            request.password.clone_from(password);
        }
        if let Some((token, mfa_hash)) = &attempt.mfa {
            request = request.with_mfa(token.clone(), mfa_hash.clone());
        }
        if self.grid.needs_cooldown() && !attempt.answers_challenge {
            let label = format!("{} {}", request.first_name, request.last_name);
            wait_out_cooldown(&self.cooldown, &label, self.force).await?;
        }
        match Client::connect(LoginParams { login_uri, request }).await {
            Ok(client) => Ok(LoginAnswer::Admitted(Box::new(spawn_session(client, spec)))),
            Err(sl_client_tokio::Error::MfaChallenge(challenge)) => {
                Ok(LoginAnswer::Challenged(challenge))
            }
            Err(sl_client_tokio::Error::LoginRejected { kind, failure }) => {
                Ok(LoginAnswer::Refused {
                    kind,
                    failure: *failure,
                })
            }
            Err(other) => Err(TestFailure::Client(other)),
        }
    }

    /// A one-time code for this session's avatar, from the credentials'
    /// `mfa_command`.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::MfaRequired`] if the avatar has no command, or
    /// [`TestFailure::Auth`] if it fails.
    pub fn acquire_mfa_token(&self) -> Result<String, TestFailure> {
        Ok(self
            .avatar
            .acquire_mfa()
            .map_err(|error| TestFailure::Auth(error.to_string()))?
            .ok_or(TestFailure::MfaRequired)?
            .expose()
            .to_owned())
    }
}

/// What one [`Session::attempt_login`] sends differently from the session's own
/// login. The default is the avatar's own name and password with no
/// multi-factor answer.
#[derive(Debug, Clone, Default)]
pub struct LoginAttempt {
    /// A first and last name to log in as instead of the avatar's.
    pub name: Option<(String, String)>,
    /// A password to send instead of the avatar's.
    pub password: Option<String>,
    /// A one-time code (possibly empty) and the `mfa_hash` to echo with it.
    pub mfa: Option<(String, Option<String>)>,
    /// Whether this attempt answers the challenge the previous one drew, so
    /// the login cooldown is not waited out between the two.
    pub answers_challenge: bool,
    /// A `start` wire string to log in at instead of the session's own — how a
    /// case that was placed at a fixed spot asks where the grid itself puts
    /// the avatar's next login (`"last"`).
    pub start: Option<String>,
}

/// What a grid answered one [`Session::attempt_login`] with.
#[derive(Debug)]
pub enum LoginAnswer {
    /// The grid let the login through; this is its live session, which the
    /// case must log out.
    Admitted(Box<Session>),
    /// The grid asked for a one-time code.
    Challenged(sl_client_tokio::MfaChallenge),
    /// The grid refused.
    Refused {
        /// How the client classifies the refusal.
        kind: LoginRejectKind,
        /// The refusal as the grid sent it.
        failure: sl_client_tokio::LoginFailure,
    },
}

/// The stable per-avatar label used for cooldown stamps: the avatar's
/// `First Last` identity (matches the runner's labelling).
fn avatar_label(avatar: &Avatar) -> String {
    format!("{} {}", avatar.first(), avatar.last())
}

/// One conformance login, named rather than passed positionally: at the call
/// sites the tail is `&cooldown, args.force, None` — three values whose types
/// say nothing about which is which.
#[derive(Debug, Clone)]
pub struct LoginSpec<'a> {
    /// The grid to log in to.
    pub grid: Grid,
    /// The account to log in as.
    pub avatar: &'a Avatar,
    /// The viewer channel reported at login.
    pub channel: &'a str,
    /// The version reported with it.
    pub version: &'a str,
    /// The `start` wire string the avatar logs in at (`"last"` for almost every
    /// case; a fixed `"uri:Region&x&y&z"` for a case that must be co-located
    /// with an in-world resource).
    pub start_location: &'a str,
    /// The per-avatar login-cooldown stamps; retained on the session so a
    /// later [`Session::relogin`] can honour the aditi login cooldown.
    pub cooldown: &'a LoginCooldown,
    /// Whether to log in despite an unexpired cooldown stamp; likewise
    /// retained.
    pub force: bool,
    /// The per-account inventory disk-cache directory, or `None` to leave the
    /// inventory disk cache off (what every case but `inventory-cache-skip`
    /// passes). When `Some`, the runtime caches the agent's inventory tree
    /// there across the session's [`Session::disconnect`] /
    /// [`Session::relogin`] cycle.
    pub cache_dir: Option<PathBuf>,
    /// What the session does to its circuits from the first datagram on —
    /// [`CircuitProbe::Observe`] for a case that reads the arrival burst
    /// ([`GridTest::probes_arrival`](crate::registry::GridTest::probes_arrival)),
    /// [`CircuitProbe::Off`] otherwise.
    pub probe: CircuitProbe,
    /// The login request's `options` list, or `None` for the client's default
    /// (what every case but `login-options` passes).
    pub options: Option<Vec<String>>,
    /// The capability names the seed requests ask for, or `None` for the
    /// client's default (what every case but `seed-capabilities` passes).
    pub capabilities: Option<Vec<String>>,
}

/// Log in as the spec says, answering any MFA challenge, and spawn the run
/// loop, returning the live [`Session`].
///
/// # Errors
///
/// Returns a [`TestFailure`] if the login URI is invalid, the start location
/// cannot be parsed, MFA is required but unavailable, or the login fails.
pub async fn login(spec: LoginSpec<'_>) -> Result<Session, TestFailure> {
    connect_and_spawn(spec).await
}

/// Perform the XML-RPC login, spawn the run loop and its drains, and assemble a
/// live [`Session`]. This is the shared core of [`login`] and
/// [`Session::relogin`].
///
/// [`LoginSpec::cooldown`] and [`LoginSpec::force`] are retained on the
/// returned session so a later [`Session::relogin`] can honour the aditi login
/// cooldown. This function does not itself enforce the cooldown — the runner
/// gates the initial logins and [`Session::relogin`] waits it out for
/// reconnections.
///
/// # Errors
///
/// Returns a [`TestFailure`] if the login URI is invalid, the start location
/// cannot be parsed, MFA is required but unavailable, or the login fails.
async fn connect_and_spawn(spec: LoginSpec<'_>) -> Result<Session, TestFailure> {
    let (login_uri, mut request) = login_request(&spec)?;
    let grid = spec.grid;
    let avatar = spec.avatar;
    let mut already_logged_in_retries: u8 = 0;
    let client = loop {
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
                failure,
            }) if grid == Grid::Opensim
                && already_logged_in_retries < ALREADY_LOGGED_IN_MAX_RETRIES =>
            {
                already_logged_in_retries = already_logged_in_retries.saturating_add(1);
                tracing::warn!(
                    "login rejected as already-logged-in ({}: {}); the rejected \
                     attempt evicts the stale presence — retrying (attempt {})",
                    failure.reason,
                    failure.message,
                    already_logged_in_retries.saturating_add(1)
                );
                tokio::time::sleep(ALREADY_LOGGED_IN_RETRY_DELAY).await;
            }
            Err(other) => return Err(TestFailure::Client(other)),
        }
    };

    Ok(spawn_session(client, spec))
}

/// The login endpoint and the request `spec` describes: the avatar's own name
/// and password, the spec's start location, channel, version and options.
fn login_request(spec: &LoginSpec<'_>) -> Result<(url::Url, LoginRequest), TestFailure> {
    let LoginSpec {
        grid,
        avatar,
        channel,
        version,
        start_location,
        options,
        ..
    } = spec;
    // The avatar's own URI wins; otherwise the grid's fixed address. The fake
    // grid has none — it binds an ephemeral port, and the credentials
    // `crate::fake::FakeGridHarness` synthesises carry the URI it bound — so an
    // avatar reaching here without one was not built by the harness.
    let login_uri_text = match avatar.login_uri() {
        Some(explicit) => explicit.to_owned(),
        None => grid
            .default_login_uri()
            .ok_or_else(|| {
                TestFailure::Login(format!(
                    "the {grid} grid has no fixed login URI and this avatar names none"
                ))
            })?
            .to_owned(),
    };
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
        (*channel).to_owned(),
        (*version).to_owned(),
    );
    if let Some(options) = options.as_ref() {
        request.options.clone_from(options);
    }
    Ok((login_uri, request))
}

/// Spawn the run loop and its drains around a connected `client`, and
/// assemble the live [`Session`] `spec` describes.
fn spawn_session(mut client: Client, spec: LoginSpec<'_>) -> Session {
    let LoginSpec {
        grid,
        avatar,
        channel,
        version,
        start_location,
        cooldown,
        force,
        cache_dir,
        probe,
        options,
        capabilities,
    } = spec;
    // Enable diagnostics so a case can observe protocol anomalies (e.g. a
    // logout that never received its `LogoutReply`); they are off by default.
    client.set_diagnostics(true);
    client.set_circuit_probe(probe);

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
    if let Some(capabilities) = capabilities.as_ref() {
        client.set_requested_capabilities(capabilities.iter().cloned());
    }
    // The neighbours' capability maps, as each seed answers.
    let neighbour_caps = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let (neighbour_tx, mut neighbour_rx) =
        mpsc::channel::<(SocketAddr, std::collections::HashMap<String, String>)>(8);
    client.set_neighbour_caps_reporter(neighbour_tx);
    let neighbour_sink = Arc::clone(&neighbour_caps);
    let _neighbour_drain = tokio::spawn(async move {
        while let Some((sim, map)) = neighbour_rx.recv().await {
            neighbour_sink
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(sim, map);
        }
    });
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
    let login_account = client.login_account().cloned();
    let login_success = client.login_success().cloned().map(Box::new);
    let login_session_id = client.session_id();
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
    Session {
        agent_id,
        login_account,
        login_success,
        login_session_id,
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
        cooldown: cooldown.clone(),
        force,
        cache_dir,
        options,
        capabilities,
        neighbour_caps,
        connected: true,
        caps,
    }
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
    /// The other end of the conversation, on [`Grid::FakeSl`] only.
    ///
    /// A live grid is something a case can only ask questions of; the fake grid
    /// is something a case can also *tell*. The handovers — a border crossing,
    /// a grid-initiated teleport — are decided by a simulator, so on a grid that
    /// simulates no movement the case has to make the decision itself.
    fake: Option<crate::fake::FakeControl>,
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
            fake: None,
        }
    }

    /// Attach the grid-side handle a [`Grid::FakeSl`] run drives the simulator
    /// with.
    #[must_use]
    pub fn with_fake(mut self, control: crate::fake::FakeControl) -> Self {
        self.fake = Some(control);
        self
    }

    /// The grid-side handle, on the fake grid only.
    ///
    /// `None` on a live grid, where nothing in the harness can speak as the
    /// simulator — which is why every case that reaches for this declares
    /// [`Grid::FakeSl`] as its only grid.
    #[must_use]
    pub const fn fake(&self) -> Option<&crate::fake::FakeControl> {
        self.fake.as_ref()
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

    /// Where a case that rezzes places its objects, if the fixtures name a
    /// build location (see [`Fixtures::build_position`]).
    #[must_use]
    pub fn build_position(&self) -> Option<sl_client_tokio::Vector> {
        self.fixtures.build_position()
    }

    /// The configured fetchable mesh asset the `mesh-fetch-http` case pulls, if
    /// any. When absent the case scans the region's object stream for a
    /// mesh-shaped prim instead.
    #[must_use]
    pub const fn mesh_asset(&self) -> Option<MeshKey> {
        self.fixtures.mesh_asset()
    }

    /// The configured stable experience the `experience-info` case resolves and
    /// searches for, if any. When absent the case records `partial` (the test
    /// avatar owns no experience and OpenSim has no experience backend).
    #[must_use]
    pub const fn experience(&self) -> Option<ExperienceKey> {
        self.fixtures.experience()
    }

    /// The regions the `parcel-ban-line` case visits after its login region,
    /// if the operator named any.
    #[must_use]
    pub fn ban_line_regions(&self) -> &[String] {
        self.fixtures.ban_line_regions()
    }

    /// The primary session.
    pub const fn primary(&mut self) -> &mut Session {
        &mut self.primary
    }

    /// The secondary session, if this is a two-account test.
    pub const fn secondary(&mut self) -> Option<&mut Session> {
        self.secondary.as_mut()
    }

    /// Both sessions of a two-account test at once, for a case that has to
    /// watch one avatar *while* the other acts — a wait on one session after
    /// the other has finished can say what arrived, but not when.
    pub const fn primary_and_secondary(&mut self) -> Option<(&mut Session, &mut Session)> {
        match self.secondary.as_mut() {
            Some(secondary) => Some((&mut self.primary, secondary)),
            None => None,
        }
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

/// Enforce, then refresh, the aditi login cooldown for `avatar_label`.
///
/// When `force` is false and the last login for this avatar was within the
/// [`ADITI_LOGIN_COOLDOWN`](sl_repl::ADITI_LOGIN_COOLDOWN) window, returns
/// [`TestFailure::Cooldown`]. Otherwise stamps the current time and returns
/// `Ok(())`. The stamps are the shared [`LoginCooldown`]'s, so a conformance
/// run and an end-to-end stage hold each other to the window too.
///
/// # Errors
///
/// Returns [`TestFailure::Cooldown`] if the cooldown is active, or
/// [`TestFailure::State`] if the timestamp cannot be written.
pub fn enforce_cooldown(
    cooldown: &LoginCooldown,
    avatar_label: &str,
    force: bool,
) -> Result<(), TestFailure> {
    let claimed = if force {
        cooldown.stamp(avatar_label)
    } else {
        cooldown.claim(avatar_label)
    };
    claimed.map_err(|error| match error {
        CooldownError::Active { avatar, remaining } => TestFailure::Cooldown {
            avatar,
            remaining_secs: remaining.as_secs().saturating_add(1),
        },
        other => TestFailure::State(other.to_string()),
    })
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
pub async fn wait_out_cooldown(
    cooldown: &LoginCooldown,
    avatar_label: &str,
    force: bool,
) -> Result<(), TestFailure> {
    let wait = cooldown.wait_time(avatar_label);
    if !force && !wait.is_zero() {
        tracing::info!(
            "waiting out aditi login cooldown for {avatar_label}: {}s",
            wait.as_secs()
        );
        tokio::time::sleep(wait).await;
    }
    cooldown
        .stamp(avatar_label)
        .map_err(|error| TestFailure::State(error.to_string()))
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

/// Each neighbour region's capability map (name → URL), keyed by the
/// neighbour's simulator address.
type NeighbourCapabilityMaps =
    std::collections::HashMap<SocketAddr, std::collections::HashMap<String, String>>;

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
    /// The case body itself panicked; the harness caught the unwind so the run
    /// could still be recorded and the avatars logged out (see
    /// [`crate::isolate`]).
    #[error("case panicked: {0}")]
    Panic(String),
    /// The aditi login cooldown is still active for this avatar.
    #[error(
        "aditi cooldown active for {avatar}: {remaining_secs}s remaining (use --force to override)"
    )]
    Cooldown {
        /// The avatar still cooling down.
        avatar: String,
        /// Seconds remaining before another login is allowed.
        remaining_secs: u64,
    },
    /// Local harness state (cooldown stamp) could not be read or written.
    #[error("harness state error: {0}")]
    State(String),
}

#[cfg(test)]
mod tests {
    use super::{enforce_cooldown, wait_out_cooldown};
    use sl_repl::LoginCooldown;
    use std::path::PathBuf;
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

    /// With no prior stamp there is nothing to wait for: `wait_out_cooldown`
    /// returns promptly and records a fresh stamp.
    #[tokio::test]
    async fn wait_out_cooldown_is_immediate_without_a_prior_stamp() {
        let scratch = ScratchDir::new("wait-noprior");
        let cooldown = LoginCooldown::under(&scratch.0);
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            wait_out_cooldown(&cooldown, "primary", false),
        )
        .await;
        assert!(matches!(result, Ok(Ok(()))), "should not wait or error");
        assert!(
            cooldown.stamp_path("primary").exists(),
            "a fresh login stamp should be written"
        );
    }

    /// `force` skips the wait even when a stamp was just written (an un-forced
    /// call would otherwise block for the full cooldown window), and an
    /// un-forced second claim is refused.
    #[tokio::test]
    async fn wait_out_cooldown_force_skips_the_wait() {
        let scratch = ScratchDir::new("wait-force");
        let cooldown = LoginCooldown::under(&scratch.0);
        assert!(
            enforce_cooldown(&cooldown, "primary", false).is_ok(),
            "initial stamp should be written"
        );
        assert!(
            enforce_cooldown(&cooldown, "primary", false).is_err(),
            "a second login within the window is refused"
        );
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            wait_out_cooldown(&cooldown, "primary", true),
        )
        .await;
        assert!(
            matches!(result, Ok(Ok(()))),
            "force should return without waiting out the cooldown"
        );
    }
}
