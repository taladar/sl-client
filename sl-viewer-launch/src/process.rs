//! Running viewers, and getting them to stop.
//!
//! # Why this is not `Child::kill`
//!
//! A viewer that is killed never sends its `LogoutRequest`, and the simulator
//! goes on believing the avatar is logged in. The bill for that is not paid by
//! the run that was killed — it is paid by the *next* one, which fails to log in
//! until the stale presence times out, with a failure that looks exactly like a
//! viewer bug and costs an afternoon to not find.
//!
//! So a viewer is asked to stop, in this order:
//!
//! 1. `SIGTERM`. Both this workspace's viewer (`install_termination_handler`)
//!    and Firestorm (`LLApp`'s handler) turn it into the same graceful logout
//!    their Quit menu takes.
//! 2. The logout grace: the seconds a viewer needs to send `LogoutRequest`,
//!    hear the reply, save what it saves, and exit.
//! 3. `SIGKILL`, and only then. It is a lost run, and the log says so.
//!
//! A [`RunningViewer`] dropped while its viewer still runs is stopped the same
//! way, so a harness whose test body panics still logs every viewer out.
//!
//! # Why the deadline is not a `wait()` with a timeout
//!
//! There is no such thing in the standard library, and an async runtime would
//! buy nothing: a run is minutes long, so a poll every quarter second is both
//! cheap and — unlike a blocking wait — interruptible, which is what lets
//! `Ctrl-C` in the terminal reach the child as a signal rather than orphaning
//! it. Several viewers are stopped in parallel, one thread each
//! ([`stop_all`]), so a run of N viewers waits out one grace, not N.

use core::time::Duration;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::launch::Launch;

/// How often a running child is looked at. Short enough that a `Ctrl-C` is acted
/// on promptly, long enough to cost nothing over a run of minutes.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long a signalled viewer is given to log out and exit before `SIGKILL`.
///
/// Generous on purpose: it covers a `LogoutRequest`, the simulator's reply, the
/// viewer's own settings save and its window teardown, and the cost of being
/// wrong is asymmetric — waiting ten seconds too long costs ten seconds, while
/// killing one second too early costs the next run.
pub const LOGOUT_GRACE: Duration = Duration::from_secs(45);

/// How a viewer's run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// It exited on its own, with this status code.
    Exited(i32),
    /// It was killed by this signal — without being asked first, or while
    /// asked by a signal other than the `SIGTERM` (a crash during its logout,
    /// a `SIGKILL` from outside this crate).
    Signalled(i32),
    /// It was asked to quit, and did: it exited, or the `SIGTERM` ended it.
    /// A viewer that died of another signal after the request is
    /// [`Signalled`](Self::Signalled), not this.
    AskedToQuit,
    /// It was asked to quit, ignored the request for the whole grace, and was
    /// killed. The grid session it leaves behind may block the next login.
    Killed,
}

impl Ending {
    /// Whether the viewer ended without being pushed. Says nothing about whether
    /// the run *worked* — that is for whatever the viewer wrote to say.
    #[must_use]
    pub const fn was_voluntary(self) -> bool {
        matches!(self, Self::Exited(_) | Self::Signalled(_))
    }
}

/// What one viewer's run cost and how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    /// How it ended.
    pub ending: Ending,
    /// How long it ran.
    pub duration: Duration,
}

/// Why a viewer could not be run, or waited for.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The executable could not be started.
    #[error("starting {program}: {source}")]
    Spawn {
        /// The program that could not be started.
        program: String,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The log file could not be opened.
    #[error("opening the viewer log {path}: {source}")]
    Log {
        /// The log file.
        path: String,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Waiting on the child failed.
    #[error("waiting for {name}: {source}")]
    Wait {
        /// The viewer waited for.
        name: String,
        /// The underlying error.
        source: std::io::Error,
    },
}

/// One running viewer.
///
/// Several may be alive at once. Its viewer is stopped gracefully by
/// [`stop`](Self::stop), by [`stop_all`], or — if neither was called — when it
/// is dropped.
#[derive(Debug)]
pub struct RunningViewer {
    /// The launch's name.
    name: String,
    /// The child.
    child: Child,
    /// When it was spawned.
    started: Instant,
    /// How it ended, once it has — and once reaped, never signalled again: a
    /// reaped pid may already belong to some other process.
    ended: Option<Ending>,
}

impl RunningViewer {
    /// Start `launch`, its standard output and error going to its log file.
    ///
    /// The log rather than the terminal: several viewers' logs interleaved on
    /// one terminal are unreadable, and the log is the file that says why a run
    /// failed.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the log could not be opened or the viewer could
    /// not be started.
    pub fn spawn(launch: &Launch) -> Result<Self, Error> {
        let log_error = |source| Error::Log {
            path: launch.log.display().to_string(),
            source,
        };
        let log = fs_err::File::create(&launch.log).map_err(log_error)?;
        let errors = log.try_clone().map_err(log_error)?;
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(std::fs::File::from(log)))
            .stderr(Stdio::from(std::fs::File::from(errors)));
        for (key, value) in &launch.env {
            command.env(key, value);
        }
        let started = Instant::now();
        let child = command.spawn().map_err(|source| Error::Spawn {
            program: launch.program.display().to_string(),
            source,
        })?;
        tracing::info!(
            "{} started as pid {}; its log is {}",
            launch.name,
            child.id(),
            launch.log.display()
        );
        Ok(Self {
            name: launch.name.clone(),
            child,
            started,
            ended: None,
        })
    }

    /// The launch's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The viewer's process id.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// How the viewer ended, if it has — without waiting.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wait`] when the child's status could not be read.
    pub fn try_ending(&mut self) -> Result<Option<Ending>, Error> {
        if self.ended.is_none()
            && let Some(status) = self.child.try_wait().map_err(|source| Error::Wait {
                name: self.name.clone(),
                source,
            })?
        {
            self.ended = Some(ending_of(&status));
        }
        Ok(self.ended)
    }

    /// Wait for the viewer to end on its own, until `limit` has passed or
    /// `interrupted` is raised. `None` means it is still running.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wait`] when the child's status could not be read.
    pub fn wait(
        &mut self,
        limit: Duration,
        interrupted: &AtomicBool,
    ) -> Result<Option<Ending>, Error> {
        let since = Instant::now();
        loop {
            if let Some(ending) = self.try_ending()? {
                return Ok(Some(ending));
            }
            if interrupted.load(Ordering::Relaxed) || since.elapsed() >= limit {
                return Ok(None);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Stop the viewer: ask it to log out and quit, give it `grace` to do so,
    /// and only then kill it. A viewer that already ended is only reported.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Wait`] when the child's status could not be read.
    pub fn stop(mut self, grace: Duration) -> Result<Ran, Error> {
        let ending = self.stop_in_place(grace)?;
        Ok(Ran {
            ending,
            duration: self.started.elapsed(),
        })
    }

    /// [`stop`](Self::stop), on a viewer that stays owned — the body both it
    /// and `Drop` share.
    fn stop_in_place(&mut self, grace: Duration) -> Result<Ending, Error> {
        if let Some(ending) = self.try_ending()? {
            return Ok(ending);
        }
        self.ask_to_quit();
        let never = AtomicBool::new(false);
        if let Some(ending) = self.wait(grace, &never)? {
            // Only an exit, or the SIGTERM itself, is the viewer taking the
            // request. Death by any other signal while it was asked — a crash
            // during the logout, a SIGKILL from elsewhere — stays what it is,
            // or a crash on the way out would read as a clean stop.
            let ending = match ending {
                Ending::Signalled(signal) if Signal::try_from(signal) != Ok(Signal::SIGTERM) => {
                    ending
                }
                _ => Ending::AskedToQuit,
            };
            self.ended = Some(ending);
            return Ok(ending);
        }
        tracing::error!(
            "{} did not log out within {} s of being asked; killing it. The grid may \
             hold its session open, which can make the next run fail to log in",
            self.name,
            grace.as_secs()
        );
        let _ignored = self.child.kill();
        let _reaped = self.child.wait().map_err(|source| Error::Wait {
            name: self.name.clone(),
            source,
        })?;
        let ending = Ending::Killed;
        self.ended = Some(ending);
        Ok(ending)
    }

    /// Send `SIGTERM`, which the viewers turn into a graceful logout.
    fn ask_to_quit(&self) {
        let Ok(pid) = i32::try_from(self.child.id()) else {
            return;
        };
        if let Err(error) = kill(Pid::from_raw(pid), Signal::SIGTERM) {
            tracing::warn!("could not signal {} (pid {pid}): {error}", self.name);
        }
    }
}

impl Drop for RunningViewer {
    /// A viewer nobody stopped — a test that panicked, a harness that returned
    /// early with `?` — is stopped here, gracefully, so it neither outlives the
    /// harness nor strands its session.
    fn drop(&mut self) {
        if self.ended.is_some() {
            return;
        }
        tracing::warn!("{} was dropped while running; asking it to quit", self.name);
        if let Err(error) = self.stop_in_place(LOGOUT_GRACE) {
            tracing::error!("stopping {}: {error}", self.name);
        }
    }
}

/// Stop every viewer in `viewers` at once, each as [`RunningViewer::stop`]
/// does, and report each one's run in the same order.
///
/// In parallel because the grace is per viewer: stopped one after another, a
/// run of N viewers that ignore the request would wait N graces.
#[must_use]
pub fn stop_all(viewers: Vec<RunningViewer>, grace: Duration) -> Vec<Result<Ran, Error>> {
    std::thread::scope(|scope| {
        let stopping: Vec<_> = viewers
            .into_iter()
            .map(|viewer| scope.spawn(move || viewer.stop(grace)))
            .collect();
        stopping
            .into_iter()
            .map(|handle| match handle.join() {
                Ok(ran) => ran,
                Err(panic) => std::panic::resume_unwind(panic),
            })
            .collect()
    })
}

/// Run one viewer to completion, escalating if it overruns `deadline`.
///
/// `interrupted` is the runner's own `Ctrl-C` flag ([`interrupt_flag`]): when it
/// is raised the viewer is asked to quit immediately rather than at the
/// deadline, so an interrupted run still leaves the grid session clean.
///
/// # Errors
///
/// Returns [`Error`] when the viewer could not be started, its log could not be
/// opened, or waiting on it failed. A viewer that ran and failed is not an error
/// here — that is an [`Ending`], and whatever the viewer wrote.
pub fn run(launch: &Launch, deadline: Duration, interrupted: &AtomicBool) -> Result<Ran, Error> {
    let mut viewer = RunningViewer::spawn(launch)?;
    if viewer.wait(deadline, interrupted)?.is_none() {
        // Overran (or the operator interrupted): ask, wait, and only then kill.
        let reason = if interrupted.load(Ordering::Relaxed) {
            "interrupted"
        } else {
            "overran its deadline"
        };
        tracing::warn!("{} {reason}; asking it to log out and quit", launch.name);
    }
    viewer.stop(LOGOUT_GRACE)
}

/// Classify an exit status: an ordinary exit, or a signal nobody here sent.
fn ending_of(status: &std::process::ExitStatus) -> Ending {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _signal) => Ending::Exited(code),
        (None, Some(signal)) => Ending::Signalled(signal),
        // Neither a code nor a signal is not a state Unix produces; report it as
        // an unremarkable failure rather than inventing a category for it.
        (None, None) => Ending::Exited(-1),
    }
}

/// Install the runner's own `Ctrl-C` handling and return the flag it raises.
///
/// A runner that dies on `Ctrl-C` takes an in-process grid with it and leaves
/// its viewers logged into nothing, which is the same stranded-session problem
/// from the other end. With this, an interrupt reaches a running viewer as a
/// request to log out.
///
/// # Errors
///
/// Returns the registration error.
pub fn interrupt_flag() -> Result<Arc<AtomicBool>, std::io::Error> {
    let flag = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        let _registration = signal_hook::flag::register(signal, Arc::clone(&flag))?;
    }
    Ok(flag)
}

/// Whether `program` looks runnable, so a missing viewer is reported before a
/// grid is started rather than as a failed run twenty minutes later.
#[must_use]
pub fn is_executable(program: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    fs_err::metadata(program)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests;
