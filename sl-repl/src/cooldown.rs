//! The per-avatar login cooldown for a rate-limited grid (Second Life Beta,
//! aditi).
//!
//! A grid that flags an account for logging in too often must not see one
//! avatar log in more than once per [`ADITI_LOGIN_COOLDOWN`]. Every harness
//! that logs in unattended — the conformance runner, the end-to-end stage —
//! asks the same [`LoginCooldown`] before a login, so a run of one right
//! after a run of the other still waits: one guard, one set of stamps.
//!
//! The stamps live in the user's state directory
//! ([`LoginCooldown::shared`]), not in a checkout, so two worktrees of the
//! workspace share them too. Each is one file per avatar holding the RFC 3339
//! time of its last login.

use std::path::{Path, PathBuf};
use std::time::Duration;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// How long one avatar must wait between two logins to aditi.
pub const ADITI_LOGIN_COOLDOWN: Duration = Duration::from_secs(120);

/// How far past the window a waited-out login lands, so its stamp is
/// unambiguously outside the previous one's window.
const MARGIN: Duration = Duration::from_secs(1);

/// The per-avatar login stamps, and the window they are held to.
#[expect(
    clippy::module_name_repetitions,
    reason = "`LoginCooldown` is the name every harness imports it by"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCooldown {
    /// Where the stamps are.
    dir: PathBuf,
    /// How long after a login the next one must wait.
    window: Duration,
}

impl LoginCooldown {
    /// The guard every harness shares: stamps under the user's state
    /// directory (`$XDG_STATE_HOME/sl-client/login-cooldown` on Linux), held
    /// to [`ADITI_LOGIN_COOLDOWN`].
    ///
    /// # Errors
    ///
    /// [`CooldownError::NoStateDir`] when the platform names no home
    /// directory to find it under.
    pub fn shared() -> Result<Self, CooldownError> {
        let dirs = directories::ProjectDirs::from("net", "taladar", "sl-client")
            .ok_or(CooldownError::NoStateDir)?;
        let state = dirs.state_dir().unwrap_or_else(|| dirs.data_local_dir());
        Ok(Self::under(state.join("login-cooldown")))
    }

    /// A guard keeping its stamps in `dir`, held to
    /// [`ADITI_LOGIN_COOLDOWN`].
    #[must_use]
    pub fn under(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            window: ADITI_LOGIN_COOLDOWN,
        }
    }

    /// The same stamps held to another window — for a test that cannot wait
    /// two minutes.
    #[must_use]
    pub const fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    /// Where the stamps are.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The stamp file of `avatar` (its `First Last` name).
    #[must_use]
    pub fn stamp_path(&self, avatar: &str) -> PathBuf {
        let stem: String = avatar
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{stem}.timestamp"))
    }

    /// How long `avatar` must still wait before its next login: zero when it
    /// has no stamp, an unreadable one, or one older than the window.
    #[must_use]
    pub fn remaining(&self, avatar: &str) -> Duration {
        let Some(previous) = fs_err::read_to_string(self.stamp_path(avatar))
            .ok()
            .and_then(|text| OffsetDateTime::parse(text.trim(), &Rfc3339).ok())
        else {
            return Duration::ZERO;
        };
        let elapsed = OffsetDateTime::now_utc()
            .unix_timestamp()
            .saturating_sub(previous.unix_timestamp());
        // A stamp from the future (a clock that stepped back) counts as just
        // now, so the whole window is waited rather than none of it.
        let elapsed = Duration::from_secs(u64::try_from(elapsed).unwrap_or(0));
        self.window.saturating_sub(elapsed)
    }

    /// How long to sleep before logging `avatar` in, the margin included —
    /// zero when it may log in now. Sleep that long, then [`stamp`](Self::stamp).
    #[must_use]
    pub fn wait_time(&self, avatar: &str) -> Duration {
        let remaining = self.remaining(avatar);
        if remaining.is_zero() {
            Duration::ZERO
        } else {
            remaining.saturating_add(MARGIN)
        }
    }

    /// Record a login of `avatar` now.
    ///
    /// # Errors
    ///
    /// [`CooldownError::Io`] when the stamp cannot be written,
    /// [`CooldownError::Clock`] when the time cannot be formatted.
    pub fn stamp(&self, avatar: &str) -> Result<(), CooldownError> {
        let path = self.stamp_path(avatar);
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| CooldownError::Clock(error.to_string()))?;
        fs_err::create_dir_all(&self.dir).map_err(CooldownError::Io)?;
        fs_err::write(path, now).map_err(CooldownError::Io)
    }

    /// Let `avatar` log in now, and stamp it — or refuse while its cooldown
    /// runs.
    ///
    /// # Errors
    ///
    /// [`CooldownError::Active`] while the window of its last login is still
    /// open (nothing is stamped then), or as [`stamp`](Self::stamp).
    pub fn claim(&self, avatar: &str) -> Result<(), CooldownError> {
        let remaining = self.remaining(avatar);
        if !remaining.is_zero() {
            return Err(CooldownError::Active {
                avatar: avatar.to_owned(),
                remaining,
            });
        }
        self.stamp(avatar)
    }
}

/// Why the cooldown refused a login or could not be kept.
#[expect(
    clippy::module_name_repetitions,
    reason = "`CooldownError` reads best as this module's public error name"
)]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CooldownError {
    /// The avatar logged in less than a window ago.
    #[error(
        "avatar {avatar} logged in less than the cooldown ago; wait {} s more",
        remaining.as_secs().saturating_add(1)
    )]
    Active {
        /// The avatar.
        avatar: String,
        /// How much of the window is left.
        remaining: Duration,
    },
    /// The platform names no directory to keep the stamps in.
    #[error("no home directory to keep the login-cooldown stamps under")]
    NoStateDir,
    /// A stamp could not be read or written.
    #[error("the login-cooldown stamp: {0}")]
    Io(#[source] std::io::Error),
    /// The time could not be formatted.
    #[error("formatting the login time: {0}")]
    Clock(String),
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use super::{CooldownError, LoginCooldown};

    /// A fresh stamp directory for one test.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sl-repl-cooldown-{name}-{}", std::process::id()));
        let _gone = fs_err::remove_dir_all(&dir);
        dir
    }

    /// An avatar with no stamp may log in; once claimed, it may not until the
    /// window has passed, and the refusal says how long is left. Another
    /// avatar is not held by the first one's stamp.
    #[test]
    fn a_claim_holds_that_avatar_for_the_window() -> Result<(), CooldownError> {
        let dir = scratch("claim");
        let guard = LoginCooldown::under(&dir);
        assert_eq!(guard.remaining("Test One"), Duration::ZERO);
        assert_eq!(guard.wait_time("Test One"), Duration::ZERO);
        guard.claim("Test One")?;
        let remaining = guard.remaining("Test One");
        assert!(
            remaining > Duration::from_secs(110) && remaining <= Duration::from_secs(120),
            "{remaining:?} left just after a login"
        );
        assert!(guard.wait_time("Test One") > remaining, "the margin");
        assert!(matches!(
            guard.claim("Test One"),
            Err(CooldownError::Active { avatar, .. }) if avatar == "Test One"
        ));
        guard.claim("Test Two")?;
        let _gone = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    /// Once the window has passed, the avatar may log in again.
    #[test]
    fn a_stamp_older_than_the_window_does_not_hold() -> Result<(), CooldownError> {
        let dir = scratch("expired");
        let guard = LoginCooldown::under(&dir).with_window(Duration::ZERO);
        guard.claim("Test One")?;
        guard.claim("Test One")?;
        let _gone = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    /// A name is a file stem with every character a path could misread
    /// replaced.
    #[test]
    fn the_stamp_file_is_named_after_the_avatar() {
        let guard = LoginCooldown::under("/stamps");
        assert_eq!(
            guard.stamp_path("Test One/../x"),
            std::path::PathBuf::from("/stamps/Test_One____x.timestamp")
        );
    }
}
