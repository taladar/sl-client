//! Running one viewer, and getting it to stop.
//!
//! # Why this is not `Child::kill`
//!
//! A viewer that is killed never sends its `LogoutRequest`, and the simulator
//! goes on believing the avatar is logged in. The bill for that is not paid by
//! the run that was killed — it is paid by the *next* one, which fails to log in
//! until the stale presence times out, with a failure that looks exactly like a
//! viewer bug and costs an afternoon to not find.
//!
//! So a run that overruns is asked to stop, in this order:
//!
//! 1. `SIGTERM`. Both viewers turn it into the same graceful logout their Quit
//!    menu takes — Firestorm through `LLApp`'s handler, this viewer through
//!    `install_termination_handler`.
//! 2. The logout grace: the seconds a viewer needs to send `LogoutRequest`,
//!    hear the reply, save what it saves, and exit.
//! 3. `SIGKILL`, and only then. It is a lost run, and the report says so.
//!
//! Both viewers also end their own runs this way without being asked — capture,
//! log out, exit — so the ordinary path here is "wait, and it exits". The
//! escalation exists for the run that hangs, and the ordering exists so that the
//! run after it still works.
//!
//! # Why the deadline is not a `wait()` with a timeout
//!
//! There is no such thing in the standard library, and adding an async runtime
//! for it would buy nothing: a run is minutes long and there is exactly one
//! child, so a poll every quarter second is both cheap and — unlike a blocking
//! wait — interruptible, which is what lets `Ctrl-C` in the terminal reach the
//! child as a signal rather than orphaning it.

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
const LOGOUT_GRACE: Duration = Duration::from_secs(45);

/// How a viewer's run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// It exited on its own, with this status code.
    Exited(i32),
    /// It was killed by this signal without being asked first — which for these
    /// viewers means something outside this runner killed it.
    Signalled(i32),
    /// It overran its deadline, was asked to quit, and did.
    AskedToQuit,
    /// It overran its deadline, was asked to quit, ignored the request, and was
    /// killed. The grid session it leaves behind may block the next login.
    Killed,
}

impl Ending {
    /// Whether the viewer ended without being pushed. Says nothing about whether
    /// the run *worked* — that is what the status file is for.
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

/// Why a viewer could not be run at all.
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
    #[error("waiting for the viewer: {0}")]
    Wait(#[source] std::io::Error),
}

/// Run one viewer to completion, escalating if it overruns `deadline`.
///
/// `interrupted` is the runner's own `Ctrl-C` flag: when it is raised the child
/// is asked to quit immediately rather than at the deadline, so an interrupted
/// run still leaves the grid session clean.
///
/// The viewer's standard output and error go to its log file rather than to the
/// terminal: two viewers' logs interleaved on one terminal are unreadable, and
/// the log is the file that says why a run failed.
///
/// # Errors
///
/// Returns [`Error`] when the viewer could not be started, its log could not be
/// opened, or waiting on it failed. A viewer that ran and failed is not an error
/// here — that is an [`Ending`] and a status file.
pub fn run(
    launch: &Launch,
    deadline: Duration,
    interrupted: &Arc<AtomicBool>,
) -> Result<Ran, Error> {
    let log = fs_err::File::create(&launch.log).map_err(|source| Error::Log {
        path: launch.log.display().to_string(),
        source,
    })?;
    let errors = log.try_clone().map_err(|source| Error::Log {
        path: launch.log.display().to_string(),
        source,
    })?;
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
    let mut child = command.spawn().map_err(|source| Error::Spawn {
        program: launch.program.display().to_string(),
        source,
    })?;
    tracing::info!(
        "{} started as pid {}; its log is {}",
        launch.viewer.name(),
        child.id(),
        launch.log.display()
    );

    if let Some(ending) = wait_until(&mut child, started, deadline, interrupted)? {
        return Ok(Ran {
            ending,
            duration: started.elapsed(),
        });
    }

    // Overran (or the operator interrupted): ask, wait, and only then kill.
    let reason = if interrupted.load(Ordering::Relaxed) {
        "interrupted"
    } else {
        "overran its deadline"
    };
    tracing::warn!(
        "{} {reason}; asking it to log out and quit",
        launch.viewer.name()
    );
    ask_to_quit(&child);
    let never = Arc::new(AtomicBool::new(false));
    if let Some(_ending) = wait_until(&mut child, Instant::now(), LOGOUT_GRACE, &never)? {
        return Ok(Ran {
            ending: Ending::AskedToQuit,
            duration: started.elapsed(),
        });
    }
    tracing::error!(
        "{} did not log out within {} s of being asked; killing it. The grid may \
         hold its session open, which can make the next run fail to log in",
        launch.viewer.name(),
        LOGOUT_GRACE.as_secs()
    );
    let _ignored = child.kill();
    let _reaped = child.wait().map_err(Error::Wait)?;
    Ok(Ran {
        ending: Ending::Killed,
        duration: started.elapsed(),
    })
}

/// Poll `child` until it exits, `limit` elapses since `since`, or the runner is
/// interrupted. `None` means it is still running.
fn wait_until(
    child: &mut Child,
    since: Instant,
    limit: Duration,
    interrupted: &Arc<AtomicBool>,
) -> Result<Option<Ending>, Error> {
    loop {
        if let Some(status) = child.try_wait().map_err(Error::Wait)? {
            return Ok(Some(ending_of(&status)));
        }
        if interrupted.load(Ordering::Relaxed) || since.elapsed() >= limit {
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Send `SIGTERM`, which both viewers turn into a graceful logout.
fn ask_to_quit(child: &Child) {
    let Ok(pid) = i32::try_from(child.id()) else {
        return;
    };
    if let Err(error) = kill(Pid::from_raw(pid), Signal::SIGTERM) {
        tracing::warn!("could not signal pid {pid}: {error}");
    }
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
/// A runner that dies on `Ctrl-C` takes the in-process grid with it and leaves
/// two viewers logged into nothing, which is the same stranded-session problem
/// from the other end. With this, an interrupt reaches the running viewer as a
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
mod tests {
    use core::time::Duration;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use pretty_assertions::assert_eq;

    use super::{Ending, is_executable, run};
    use crate::launch::{Launch, Viewer};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A launch running `program args…`, logging into `dir`.
    fn launch(dir: &std::path::Path, program: &str, args: &[&str]) -> Launch {
        Launch {
            viewer: Viewer::SlClient,
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: Vec::new(),
            artefacts: dir.to_path_buf(),
            log: dir.join("viewer.log"),
        }
    }

    /// A scratch directory of this test's own.
    fn scratch(name: &str) -> Result<std::path::PathBuf, TestError> {
        let dir = std::env::temp_dir().join(format!("sl-crosscheck-{name}-{}", std::process::id()));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The ordinary path: the viewer ends its own run, and its output is in its
    /// log rather than on the terminal.
    #[test]
    fn a_viewer_that_exits_on_its_own_is_waited_for() -> Result<(), TestError> {
        let dir = scratch("exits")?;
        let flag = Arc::new(AtomicBool::new(false));
        let ran = run(
            &launch(&dir, "sh", &["-c", "echo hello; exit 3"]),
            Duration::from_secs(30),
            &flag,
        )?;
        assert_eq!(ran.ending, Ending::Exited(3));
        assert!(ran.ending.was_voluntary());
        let log = fs_err::read_to_string(dir.join("viewer.log"))?;
        assert!(
            log.contains("hello"),
            "the viewer's output should be in its log"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A viewer that overruns is **asked** to quit and gets the chance to take
    /// it: the whole point of the escalation is that this step exists, and a
    /// viewer that honours `SIGTERM` is never killed.
    #[test]
    fn an_overrunning_viewer_is_asked_before_it_is_killed() -> Result<(), TestError> {
        let dir = scratch("asked")?;
        let flag = Arc::new(AtomicBool::new(false));
        // Sleeps, but exits when asked — a viewer that logs out on SIGTERM.
        let ran = run(
            &launch(&dir, "sh", &["-c", "trap 'exit 0' TERM; sleep 120 & wait"]),
            Duration::from_millis(500),
            &flag,
        )?;
        assert_eq!(ran.ending, Ending::AskedToQuit);
        assert!(ran.duration < Duration::from_secs(30));
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// An interrupt is acted on where the deadline would have been, so a run
    /// stopped by hand still leaves the grid session clean.
    #[test]
    fn an_interrupt_asks_the_viewer_to_quit_early() -> Result<(), TestError> {
        let dir = scratch("interrupted")?;
        let flag = Arc::new(AtomicBool::new(true));
        let ran = run(
            &launch(&dir, "sh", &["-c", "trap 'exit 0' TERM; sleep 120 & wait"]),
            Duration::from_secs(600),
            &flag,
        )?;
        assert_eq!(ran.ending, Ending::AskedToQuit);
        assert!(
            ran.duration < Duration::from_secs(30),
            "an interrupt should not wait out the deadline"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A missing or unexecutable viewer is knowable before a grid is started.
    #[test]
    fn a_missing_viewer_is_not_executable() -> Result<(), TestError> {
        let dir = scratch("executable")?;
        assert!(!is_executable(&dir.join("no-such-viewer")));
        assert!(!is_executable(&dir), "a directory is not a viewer");
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// The interrupt flag starts down, so a run is not cut short by its own
    /// installation.
    #[test]
    fn the_interrupt_flag_starts_down() -> Result<(), TestError> {
        let flag = super::interrupt_flag()?;
        assert!(!flag.load(Ordering::Relaxed));
        Ok(())
    }
}
