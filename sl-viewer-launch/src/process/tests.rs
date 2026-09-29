//! The stop escalation against stand-in viewers: shell scripts that honour
//! `SIGTERM` the way a viewer's logout does, or ignore it the way a hung one
//! would.

use core::time::Duration;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use fs_err::PathExt as _;
use pretty_assertions::assert_eq;

use super::{Ending, RunningViewer, is_executable, run, stop_all};
use crate::launch::Launch;

/// The boxed error every test in this module reports through.
type TestError = Box<dyn core::error::Error>;

/// A viewer that logs out when asked: on `SIGTERM` it writes `logged-out` and
/// exits, taking its sleeping child with it. It writes `ready` once the trap
/// is in place, so a test never signals it before it can answer.
const LOGS_OUT: &str = "trap 'kill $!; echo out > logged-out; exit 0' TERM; \
                        sleep 120 & touch ready; wait";

/// A hung viewer: it ignores `SIGTERM` (as does the `sleep` it becomes, since
/// an ignored signal stays ignored across `exec`), so only `SIGKILL` ends it.
const HANGS: &str = "trap '' TERM; touch ready; exec sleep 120";

/// A scratch directory of this test's own, empty.
fn scratch(name: &str) -> Result<PathBuf, TestError> {
    let dir = std::env::temp_dir().join(format!("sl-viewer-launch-{name}-{}", std::process::id()));
    if fs_err::metadata(&dir).is_ok() {
        fs_err::remove_dir_all(&dir)?;
    }
    fs_err::create_dir_all(&dir)?;
    Ok(dir)
}

/// A launch running `sh -c script` in `dir`, logging into it.
fn launch(dir: &Path, name: &str, script: &str) -> Launch {
    Launch::new(name, "sh", dir.join(format!("{name}.log")))
        .args(["-c", &format!("cd '{}' && {script}", dir.display())])
}

/// Wait until the stand-in in `dir` has installed its trap.
fn wait_ready(dir: &Path) -> Result<(), TestError> {
    let since = Instant::now();
    while fs_err::metadata(dir.join("ready")).is_err() {
        if since.elapsed() > Duration::from_secs(30) {
            return Err(format!("{} never became ready", dir.display()).into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// The ordinary path: the viewer ends its own run, and its output is in its
/// log rather than on the terminal.
#[test]
fn a_viewer_that_exits_on_its_own_is_waited_for() -> Result<(), TestError> {
    let dir = scratch("exits")?;
    let flag = AtomicBool::new(false);
    let ran = run(
        &launch(&dir, "viewer", "echo hello; exit 3"),
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
    let flag = AtomicBool::new(false);
    let ran = run(
        &launch(&dir, "viewer", LOGS_OUT),
        Duration::from_secs(2),
        &flag,
    )?;
    assert_eq!(ran.ending, Ending::AskedToQuit);
    assert!(ran.duration < Duration::from_secs(30));
    assert!(dir.join("logged-out").fs_err_try_exists()?);
    fs_err::remove_dir_all(&dir)?;
    Ok(())
}

/// An interrupt is acted on where the deadline would have been, so a run
/// stopped by hand still leaves the grid session clean.
#[test]
fn an_interrupt_asks_the_viewer_to_quit_early() -> Result<(), TestError> {
    let dir = scratch("interrupted")?;
    let mut viewer = RunningViewer::spawn(&launch(&dir, "viewer", LOGS_OUT))?;
    wait_ready(&dir)?;
    let flag = AtomicBool::new(true);
    assert_eq!(viewer.wait(Duration::from_secs(600), &flag)?, None);
    let ran = viewer.stop(Duration::from_secs(30))?;
    assert_eq!(ran.ending, Ending::AskedToQuit);
    assert!(
        ran.duration < Duration::from_secs(30),
        "an interrupt should not wait out the deadline"
    );
    fs_err::remove_dir_all(&dir)?;
    Ok(())
}

/// Two viewers alive at once are stopped together, each gracefully, in one
/// grace rather than two.
#[test]
fn several_viewers_are_stopped_gracefully_in_parallel() -> Result<(), TestError> {
    let dirs = [scratch("parallel-a")?, scratch("parallel-b")?];
    let mut viewers = Vec::new();
    for (dir, name) in dirs.iter().zip(["a", "b"]) {
        viewers.push(RunningViewer::spawn(&launch(dir, name, LOGS_OUT))?);
    }
    for dir in &dirs {
        wait_ready(dir)?;
    }
    let names: Vec<String> = viewers
        .iter()
        .map(|viewer| viewer.name().to_owned())
        .collect();
    assert_eq!(names, ["a", "b"]);
    let ran = stop_all(viewers, Duration::from_secs(30));
    assert_eq!(ran.len(), 2);
    for (result, dir) in ran.into_iter().zip(&dirs) {
        assert_eq!(result?.ending, Ending::AskedToQuit);
        assert!(
            dir.join("logged-out").fs_err_try_exists()?,
            "{} did not log out",
            dir.display()
        );
        fs_err::remove_dir_all(dir)?;
    }
    Ok(())
}

/// A hung viewer is killed once the grace is out — and only then — and the
/// hung one does not hold up the one beside it.
#[test]
fn a_hung_viewer_is_killed_after_the_grace() -> Result<(), TestError> {
    let dirs = [scratch("hung")?, scratch("beside-hung")?];
    let [hung_dir, polite_dir] = &dirs;
    let hung = RunningViewer::spawn(&launch(hung_dir, "hung", HANGS))?;
    let polite = RunningViewer::spawn(&launch(polite_dir, "polite", LOGS_OUT))?;
    for dir in &dirs {
        wait_ready(dir)?;
    }
    let grace = Duration::from_secs(1);
    let since = Instant::now();
    let ran = stop_all(vec![hung, polite], grace);
    assert!(since.elapsed() >= grace, "killed before the grace was out");
    let endings: Vec<Ending> = ran
        .into_iter()
        .map(|result| result.map(|ran| ran.ending))
        .collect::<Result<_, _>>()?;
    assert_eq!(endings, [Ending::Killed, Ending::AskedToQuit]);
    for dir in &dirs {
        fs_err::remove_dir_all(dir)?;
    }
    Ok(())
}

/// A viewer dropped while running — a test body that panicked — is still
/// asked to log out, rather than left running or killed.
#[test]
fn a_dropped_viewer_is_stopped_gracefully() -> Result<(), TestError> {
    let dir = scratch("dropped")?;
    let viewer = RunningViewer::spawn(&launch(&dir, "viewer", LOGS_OUT))?;
    wait_ready(&dir)?;
    drop(viewer);
    assert!(
        dir.join("logged-out").fs_err_try_exists()?,
        "the dropped viewer was not asked to log out"
    );
    fs_err::remove_dir_all(&dir)?;
    Ok(())
}

/// A viewer that already ended is reported, not signalled: its pid is not
/// its own any more.
#[test]
fn stopping_an_ended_viewer_reports_how_it_ended() -> Result<(), TestError> {
    let dir = scratch("ended")?;
    let mut viewer = RunningViewer::spawn(&launch(&dir, "viewer", "exit 7"))?;
    let never = AtomicBool::new(false);
    assert_eq!(
        viewer.wait(Duration::from_secs(30), &never)?,
        Some(Ending::Exited(7))
    );
    assert_eq!(viewer.try_ending()?, Some(Ending::Exited(7)));
    assert_eq!(
        viewer.stop(Duration::from_secs(30))?.ending,
        Ending::Exited(7)
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
    assert!(!flag.load(core::sync::atomic::Ordering::Relaxed));
    Ok(())
}
