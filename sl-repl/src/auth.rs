//! Credentials, secret redaction, and MFA-token acquisition.
//!
//! A REPL session logs in as one of several avatars defined in a TOML
//! credentials file. Each avatar carries its login name, a redacting
//! [`Secret`] password, an optional grid / login URI, and — for grids that
//! require multi-factor authentication — an optional shell `mfa_command` whose
//! output is a one-time token.
//!
//! Second Life's MFA tokens are TOTP codes valid for a wall-clock-aligned
//! 30-second window. [`acquire_mfa_token`] runs the command, but first waits out
//! the tail of the current window when too little of it remains (see
//! [`mfa_window_guard_secs`](Avatar::mfa_window_guard_secs)) so the token the
//! viewer submits still has enough validity left to survive the login
//! round-trip.
//!
//! Secrets (the password and any acquired token) never reach a log: [`Secret`]
//! redacts itself in both [`Debug`](core::fmt::Debug) and
//! [`Display`](core::fmt::Display), and the only way to read the underlying
//! string is the explicit [`Secret::expose`].

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

/// The TOTP window length, in seconds, used by Second Life MFA tokens.
const TOTP_WINDOW_SECS: u64 = 30;

/// Default value for [`Avatar::mfa_window_guard_secs`] when the credentials file
/// omits it: require at least this many seconds left in the current TOTP window
/// before using its token, else wait for the next window.
const DEFAULT_MFA_WINDOW_GUARD_SECS: u64 = 5;

/// A secret string (a password or an acquired MFA token) that never reveals
/// itself through [`Debug`](core::fmt::Debug), [`Display`](core::fmt::Display),
/// or therefore any `tracing` log.
///
/// Read the underlying value only at the point it is genuinely needed (building
/// the login request) via [`Secret::expose`].
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct Secret(String);

/// The redaction marker rendered in place of a [`Secret`]'s contents.
const REDACTED: &str = "[redacted]";

impl Secret {
    /// Wrap a string as a redacting secret.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrow the underlying secret string.
    ///
    /// This is the only way to read the value; call it only where the secret is
    /// actually consumed (e.g. building a login body) and never pass the result
    /// to a logger.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for Secret {
    /// Render the secret as the `REDACTED` marker, never its contents.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Secret").field(&REDACTED).finish()
    }
}

impl core::fmt::Display for Secret {
    /// Render the secret as the `REDACTED` marker, never its contents.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(REDACTED)
    }
}

/// One avatar's login credentials, as deserialized from a `[avatars.<name>]`
/// table in the credentials file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Avatar {
    /// The avatar's first (account) name.
    first: String,
    /// The avatar's last name (`Resident` for modern single-name accounts).
    last: String,
    /// The account password.
    password: Secret,
    /// An optional grid nickname (e.g. `agni`, `aditi`) the binary may map to a
    /// login URI; ignored when [`login_uri`](Self::login_uri) is set.
    #[serde(default)]
    grid: Option<String>,
    /// An optional explicit XML-RPC login URI, overriding any
    /// [`grid`](Self::grid).
    #[serde(default)]
    login_uri: Option<String>,
    /// An optional shell command whose stdout is a one-time MFA (TOTP) token.
    #[serde(default)]
    mfa_command: Option<String>,
    /// The minimum seconds that must remain in the current TOTP window before
    /// its token is used; below this the acquisition waits for the next window.
    #[serde(default)]
    mfa_window_guard_secs: Option<u64>,
}

impl Avatar {
    /// The avatar's first (account) name.
    #[must_use]
    pub fn first(&self) -> &str {
        &self.first
    }

    /// The avatar's last name.
    #[must_use]
    pub fn last(&self) -> &str {
        &self.last
    }

    /// The account password.
    #[must_use]
    pub const fn password(&self) -> &Secret {
        &self.password
    }

    /// The optional grid nickname.
    #[must_use]
    pub fn grid(&self) -> Option<&str> {
        self.grid.as_deref()
    }

    /// The optional explicit login URI.
    #[must_use]
    pub fn login_uri(&self) -> Option<&str> {
        self.login_uri.as_deref()
    }

    /// The optional MFA token command.
    #[must_use]
    pub fn mfa_command(&self) -> Option<&str> {
        self.mfa_command.as_deref()
    }

    /// The effective TOTP-window guard in seconds, defaulting to
    /// `DEFAULT_MFA_WINDOW_GUARD_SECS` when unset.
    #[must_use]
    pub fn mfa_window_guard_secs(&self) -> u64 {
        self.mfa_window_guard_secs
            .unwrap_or(DEFAULT_MFA_WINDOW_GUARD_SECS)
    }

    /// Acquire this avatar's MFA token, if it has an
    /// [`mfa_command`](Self::mfa_command).
    ///
    /// Returns `Ok(None)` when no command is configured (the grid needs no MFA),
    /// otherwise runs [`acquire_mfa_token`] with this avatar's
    /// [`mfa_window_guard_secs`](Self::mfa_window_guard_secs) and returns the
    /// resulting [`Secret`].
    ///
    /// # Errors
    ///
    /// Propagates any [`AuthError`] from [`acquire_mfa_token`].
    pub fn acquire_mfa(&self) -> Result<Option<Secret>, AuthError> {
        match self.mfa_command.as_deref() {
            Some(command) => acquire_mfa_token(command, self.mfa_window_guard_secs()).map(Some),
            None => Ok(None),
        }
    }
}

/// A whole credentials file: a set of named avatars plus an optional default
/// selection.
///
/// ```toml
/// default_avatar = "alice"
///
/// [avatars.alice]
/// first = "Alice"
/// last = "Resident"
/// password = "hunter2"
/// grid = "agni"
/// mfa_command = "oathtool --totp -b ABCDEF234567"
/// mfa_window_guard_secs = 5
/// ```
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    /// The avatar to use when none is requested on the command line; ignored if
    /// the file defines exactly one avatar.
    #[serde(default)]
    default_avatar: Option<String>,
    /// Each named avatar credential, keyed by its file label.
    avatars: BTreeMap<String, Avatar>,
}

impl Credentials {
    /// Parse credentials from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Parse`] if the TOML is malformed or does not match
    /// the expected shape.
    pub fn from_toml_str(text: &str) -> Result<Self, AuthError> {
        toml::from_str(text).map_err(|error| AuthError::Parse(error.to_string()))
    }

    /// Load and parse credentials from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Io`] if the file cannot be read, or
    /// [`AuthError::Parse`] if its contents are not valid credentials.
    pub fn load(path: &Path) -> Result<Self, AuthError> {
        let text =
            std::fs::read_to_string(path).map_err(|error| AuthError::Io(error.to_string()))?;
        Self::from_toml_str(&text)
    }

    /// Select an avatar by name, falling back to the configured default or — if
    /// the file defines exactly one avatar — that sole avatar.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::UnknownAvatar`] when a requested or default name is
    /// not present, or [`AuthError::NoAvatarSelected`] when no name was given,
    /// no default is configured, and the file does not define exactly one
    /// avatar.
    pub fn select(&self, name: Option<&str>) -> Result<&Avatar, AuthError> {
        if let Some(requested) = name.or(self.default_avatar.as_deref()) {
            return self
                .avatars
                .get(requested)
                .ok_or_else(|| AuthError::UnknownAvatar(requested.to_owned()));
        }
        let mut iter = self.avatars.values();
        match (iter.next(), iter.next()) {
            (Some(only), None) => Ok(only),
            _ => Err(AuthError::NoAvatarSelected),
        }
    }

    /// The names of every avatar defined in the file, in sorted order.
    #[must_use]
    pub fn avatar_names(&self) -> Vec<&str> {
        self.avatars.keys().map(String::as_str).collect()
    }
}

/// Compute how long to wait before acquiring a TOTP token so the token carries
/// at least `guard_secs` of its window into the login round-trip.
///
/// Given the current Unix time `now_secs` and the guard, returns the duration to
/// sleep: [`Duration::ZERO`] when the current 30-second window still has at
/// least `guard_secs` remaining, otherwise the time left until the next window
/// boundary (so the command runs at the start of a fresh window).
const fn window_wait(now_secs: u64, guard_secs: u64) -> Duration {
    let into_window = now_secs % TOTP_WINDOW_SECS;
    let remaining = TOTP_WINDOW_SECS.saturating_sub(into_window);
    if remaining < guard_secs {
        Duration::from_secs(remaining)
    } else {
        Duration::ZERO
    }
}

/// Run an MFA token command, first waiting out the tail of the current TOTP
/// window when fewer than `guard_secs` of it remain.
///
/// The command is executed via `sh -c`; its trimmed stdout is the token. The
/// returned [`Secret`] is redacting, and neither the token nor the command's
/// output is ever logged here.
///
/// # Errors
///
/// Returns [`AuthError::Clock`] if the system clock is before the Unix epoch,
/// [`AuthError::Io`] if the command cannot be spawned, [`AuthError::MfaFailed`]
/// if it exits non-zero or its output is not valid UTF-8, or
/// [`AuthError::MfaEmpty`] if its (trimmed) output is empty.
pub fn acquire_mfa_token(command: &str, guard_secs: u64) -> Result<Secret, AuthError> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_error| AuthError::Clock)?
        .as_secs();
    let wait = window_wait(now_secs, guard_secs);
    if !wait.is_zero() {
        tracing::info!(
            "waiting {}s for the next MFA window before running the token command",
            wait.as_secs()
        );
        std::thread::sleep(wait);
    }
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|error| AuthError::Io(error.to_string()))?;
    if !output.status.success() {
        return Err(AuthError::MfaFailed(format!(
            "MFA command exited with {}",
            output.status
        )));
    }
    let token = String::from_utf8(output.stdout)
        .map_err(|_error| AuthError::MfaFailed("MFA command output was not UTF-8".to_owned()))?;
    let token = token.trim();
    if token.is_empty() {
        return Err(AuthError::MfaEmpty);
    }
    Ok(Secret::new(token.to_owned()))
}

/// An error loading credentials or acquiring an MFA token.
#[expect(
    clippy::module_name_repetitions,
    reason = "`AuthError` reads best as this module's public error name"
)]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The credentials file could not be read.
    #[error("could not read credentials file: {0}")]
    Io(String),
    /// The credentials file was not valid TOML or did not match the schema.
    #[error("could not parse credentials: {0}")]
    Parse(String),
    /// A requested or default avatar name is not defined in the file.
    #[error("no avatar named `{0}` in the credentials file")]
    UnknownAvatar(String),
    /// No avatar was requested, no default is set, and the file does not define
    /// exactly one avatar.
    #[error("no avatar selected and no unambiguous default available")]
    NoAvatarSelected,
    /// The system clock is set before the Unix epoch.
    #[error("system clock is before the Unix epoch")]
    Clock,
    /// The MFA command failed to run or returned an error.
    #[error("MFA command failed: {0}")]
    MfaFailed(String),
    /// The MFA command produced no token.
    #[error("MFA command produced an empty token")]
    MfaEmpty,
}

#[cfg(test)]
mod tests {
    use super::{
        AuthError, Avatar, Credentials, Duration, Secret, TOTP_WINDOW_SECS, acquire_mfa_token,
        window_wait,
    };
    use pretty_assertions::assert_eq;

    /// Parse credentials, surfacing any error as a test failure string.
    fn parse(toml: &str) -> Result<Credentials, String> {
        Credentials::from_toml_str(toml).map_err(|error| format!("{error:?}"))
    }

    /// Select an avatar, surfacing any error as a test failure string.
    fn select<'a>(creds: &'a Credentials, name: Option<&str>) -> Result<&'a Avatar, String> {
        creds.select(name).map_err(|error| format!("{error:?}"))
    }

    /// A secret hides its contents in `Debug`/`Display` but `expose` returns
    /// them.
    #[test]
    fn secret_redacts_in_debug_and_display() {
        let secret = Secret::new("hunter2".to_owned());
        assert_eq!(secret.expose(), "hunter2");
        let debug = format!("{secret:?}");
        let display = format!("{secret}");
        assert!(
            !debug.contains("hunter2"),
            "Debug must not contain the secret: {debug}"
        );
        assert!(
            !display.contains("hunter2"),
            "Display must not contain the secret: {display}"
        );
        assert!(
            debug.contains("redacted"),
            "Debug should show the redaction marker: {debug}"
        );
    }

    /// A struct that embeds a `Secret` redacts it transitively via derived
    /// `Debug`.
    #[test]
    fn embedded_secret_redacts_transitively() -> Result<(), String> {
        let toml = "\
[avatars.alice]
first = \"Alice\"
last = \"Resident\"
password = \"topsecret\"
";
        let creds = parse(toml)?;
        let avatar = select(&creds, None)?;
        let debug = format!("{avatar:?}");
        assert!(
            !debug.contains("topsecret"),
            "avatar Debug must not leak the password: {debug}"
        );
        assert_eq!(avatar.password().expose(), "topsecret");
        Ok(())
    }

    /// A full window remaining means no wait.
    #[test]
    fn window_wait_zero_at_boundary() {
        // now % 30 == 0 → 30s remain → never wait.
        assert_eq!(window_wait(0, 5), Duration::ZERO);
        assert_eq!(window_wait(30, 5), Duration::ZERO);
        assert_eq!(window_wait(60, 5), Duration::ZERO);
    }

    /// Plenty of window left means no wait.
    #[test]
    fn window_wait_zero_when_ample() {
        // now % 30 == 20 → 10s remain ≥ guard 5 → no wait.
        assert_eq!(window_wait(20, 5), Duration::ZERO);
        // exactly equal to the guard counts as enough (remaining == guard).
        assert_eq!(window_wait(25, 5), Duration::ZERO);
    }

    /// Too little window left waits to the next boundary.
    #[test]
    fn window_wait_waits_to_next_boundary() {
        // now % 30 == 27 → 3s remain < guard 5 → wait 3s.
        assert_eq!(window_wait(27, 5), Duration::from_secs(3));
        // now % 30 == 29 → 1s remain < guard 5 → wait 1s.
        assert_eq!(window_wait(29, 5), Duration::from_secs(1));
        // a guard equal to the whole window always waits unless on a boundary.
        assert_eq!(
            window_wait(1, TOTP_WINDOW_SECS),
            Duration::from_secs(TOTP_WINDOW_SECS.saturating_sub(1))
        );
    }

    /// A guard of zero never waits.
    #[test]
    fn window_wait_guard_zero_never_waits() {
        assert_eq!(window_wait(29, 0), Duration::ZERO);
        assert_eq!(window_wait(1, 0), Duration::ZERO);
    }

    /// `acquire_mfa_token` runs the command and returns its trimmed output. A
    /// guard of zero skips the window wait, so the test is fast and hermetic.
    #[test]
    fn acquire_runs_command() -> Result<(), String> {
        let token = acquire_mfa_token("echo 654321", 0).map_err(|error| format!("{error:?}"))?;
        assert_eq!(token.expose(), "654321");
        Ok(())
    }

    /// A command that produces no output is an error, not an empty token.
    #[test]
    fn acquire_rejects_empty_token() {
        let result = acquire_mfa_token("true", 0);
        assert_eq!(result, Err(AuthError::MfaEmpty));
    }

    /// Multi-avatar selection honours the explicit name, the default, and the
    /// sole-avatar fallback, and rejects the ambiguous and unknown cases.
    #[test]
    fn select_resolves_avatars() -> Result<(), String> {
        let toml = "\
default_avatar = \"bob\"

[avatars.alice]
first = \"Alice\"
last = \"Resident\"
password = \"a\"

[avatars.bob]
first = \"Bob\"
last = \"Resident\"
password = \"b\"
";
        let creds = parse(toml)?;
        assert_eq!(select(&creds, Some("alice"))?.first(), "Alice");
        assert_eq!(select(&creds, None)?.first(), "Bob");
        assert_eq!(
            creds.select(Some("carol")),
            Err(AuthError::UnknownAvatar("carol".to_owned()))
        );
        assert_eq!(creds.avatar_names(), vec!["alice", "bob"]);

        let single = "\
[avatars.only]
first = \"Only\"
last = \"One\"
password = \"x\"
";
        let creds = parse(single)?;
        assert_eq!(select(&creds, None)?.first(), "Only");

        let ambiguous = "\
[avatars.x]
first = \"X\"
last = \"Y\"
password = \"x\"

[avatars.z]
first = \"Z\"
last = \"W\"
password = \"z\"
";
        let creds = parse(ambiguous)?;
        assert_eq!(creds.select(None), Err(AuthError::NoAvatarSelected));
        Ok(())
    }

    /// An avatar with no MFA command yields no token; one with a command runs
    /// it. The guard defaults to `DEFAULT_MFA_WINDOW_GUARD_SECS` when unset.
    #[test]
    fn avatar_mfa_command_drives_acquisition() -> Result<(), String> {
        let toml = "\
[avatars.plain]
first = \"Plain\"
last = \"Resident\"
password = \"p\"

[avatars.mfa]
first = \"Mfa\"
last = \"Resident\"
password = \"m\"
mfa_command = \"echo 111222\"
mfa_window_guard_secs = 0
";
        let creds = parse(toml)?;
        let plain = select(&creds, Some("plain"))?;
        assert_eq!(
            plain.acquire_mfa().map_err(|error| format!("{error:?}"))?,
            None
        );
        let mfa = select(&creds, Some("mfa"))?;
        assert_eq!(mfa.mfa_window_guard_secs(), 0);
        let token = mfa
            .acquire_mfa()
            .map_err(|error| format!("{error:?}"))?
            .ok_or_else(|| "expected a token".to_owned())?;
        assert_eq!(token.expose(), "111222");
        Ok(())
    }
}
