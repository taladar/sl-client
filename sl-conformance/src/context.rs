//! Login, session driving, and the per-avatar aditi cooldown guard.
//!
//! Each test runs against a freshly logged-in [`Session`] (one or two of them).
//! [`login`] performs the XML-RPC login, answering an MFA challenge via the
//! avatar's `mfa_command`, and spawns the client run loop. [`TestContext`] hands
//! the live session(s) and a [`Metrics`] collector to the test body.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sl_client_tokio::{
    AgentKey, Client, Command, Diagnostic, Event, LoginParams, LoginRequest, StartLocation,
};
use sl_repl::Avatar;
use time::format_description::well_known::Rfc3339;
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::record::Completeness;

/// How long an aditi avatar must wait between logins, to avoid rate-limiting.
const ADITI_LOGIN_COOLDOWN: TimeDuration = TimeDuration::seconds(120);

/// How long [`Session::logout`] waits for a clean `LoggedOut` before forcing the
/// run loop down.
const LOGOUT_GRACE: Duration = Duration::from_secs(15);

/// A logged-in client session: the spawned run loop plus its event and command
/// channels.
#[derive(Debug)]
pub struct Session {
    /// The agent's own id, available after login.
    agent_id: Option<AgentKey>,
    /// Inbound events from the run loop.
    events: mpsc::Receiver<Event>,
    /// Outbound commands to the run loop.
    commands: mpsc::Sender<Command>,
    /// The spawned `Client::run` task.
    run: JoinHandle<Result<(), sl_client_tokio::Error>>,
}

impl Session {
    /// The agent's own id, if login reported one.
    #[must_use]
    pub const fn agent_id(&self) -> Option<AgentKey> {
        self.agent_id
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
        let events = &mut self.events;
        let wait = async {
            loop {
                match events.recv().await {
                    None => {
                        return Err(TestFailure::Disconnected("event channel closed".to_owned()));
                    }
                    Some(event) => {
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
}

/// Log in to `grid` as `avatar`, answering any MFA challenge, and spawn the run
/// loop, returning the live [`Session`].
///
/// # Errors
///
/// Returns a [`TestFailure`] if the login URI is invalid, the start location
/// cannot be parsed, MFA is required but unavailable, or the login fails.
pub async fn login(
    grid: Grid,
    avatar: &Avatar,
    channel: &str,
    version: &str,
) -> Result<Session, TestFailure> {
    let login_uri_text = avatar
        .login_uri()
        .map_or_else(|| grid.default_login_uri().to_owned(), str::to_owned);
    let login_uri: url::Url = login_uri_text
        .parse()
        .map_err(|error: url::ParseError| TestFailure::Login(error.to_string()))?;
    let start: StartLocation =
        "last"
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
            Err(other) => return Err(TestFailure::Client(other)),
        }
    };

    let agent_id = client.agent_id();
    let (event_tx, event_rx) = mpsc::channel::<Event>(256);
    let (command_tx, command_rx) = mpsc::channel::<Command>(16);
    let (diag_tx, mut diag_rx) = mpsc::channel::<Diagnostic>(64);
    // Drain diagnostics to the log so a full channel never stalls the run loop.
    let _drain = tokio::spawn(async move {
        while let Some(diagnostic) = diag_rx.recv().await {
            tracing::debug!("diagnostic: {diagnostic:?}");
        }
    });
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));
    Ok(Session {
        agent_id,
        events: event_rx,
        commands: command_tx,
        run,
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
    /// The metrics the test writes.
    metrics: Metrics,
    /// Whether the test declared its run complete or partial.
    completeness: Completeness,
    /// The note explaining a partial run.
    completeness_note: Option<String>,
}

impl TestContext {
    /// Build a context around the given live session(s).
    #[must_use]
    pub fn new(grid: Grid, primary: Session, secondary: Option<Session>) -> Self {
        Self {
            grid,
            primary,
            secondary,
            metrics: Metrics::new(),
            completeness: Completeness::Complete,
            completeness_note: None,
        }
    }

    /// The grid under test.
    #[must_use]
    pub const fn grid(&self) -> Grid {
        self.grid
    }

    /// The primary session.
    pub const fn primary(&mut self) -> &mut Session {
        &mut self.primary
    }

    /// The secondary session, if this is a two-account test.
    pub const fn secondary(&mut self) -> Option<&mut Session> {
        self.secondary.as_mut()
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
    ) {
        (
            self.metrics,
            self.completeness,
            self.completeness_note,
            self.primary,
            self.secondary,
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
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent).map_err(|error| TestFailure::State(error.to_string()))?;
    }
    let stamp = now_rfc3339()?;
    fs_err::write(&path, stamp).map_err(|error| TestFailure::State(error.to_string()))?;
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
    use super::{cooldown_path, sanitize_label};
    use pretty_assertions::assert_eq;
    use std::path::Path;

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
}
