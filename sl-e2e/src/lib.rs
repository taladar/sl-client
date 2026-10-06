//! The end-to-end stage: an in-process fake grid, several real viewers
//! logged into it, and the handles a test drives them with.
//!
//! A test names its viewers, and [`StageBuilder::run`] does the rest, once
//! per backend:
//!
//! 1. starts a fresh `sl-fake-grid` (the stock scene's region unless told
//!    otherwise) with an account `Stage <label>` per viewer — or, on a live
//!    grid, takes one account of its credentials file per viewer;
//! 2. starts each viewer — as the real binary, `--headless
//!    --automation-socket`, confined to its directory through
//!    `sl-viewer-launch` ([`Backend::Process`]), or as the viewer's own
//!    builder's App on an in-process host ([`Backend::InProcess`]) — and
//!    waits until each has logged in and its scene has settled;
//! 3. hands the body a [`Stage`]: a `sl-viewer-driver` handle per viewer, the
//!    grid, markers ([`Stage::mark`], [`Stage::wait_marker`]), and
//!    [`Stage::relog`], which logs a viewer out and back in on the same
//!    directories — what must survive a relog is tested that way;
//! 4. takes it all down — also when the body failed or panicked — by asking
//!    each viewer to log out, and fails the test if one would not or if the
//!    fake grid still holds a session afterwards.
//!
//! `SL_E2E_BACKEND=process|in-process|both` picks the backends (unset is
//! both). `SL_E2E_WATCH=1` opens each viewer process's `--watch` window, for
//! a person to follow a run (the process backend only). `SL_E2E_GRID=fake|opensim|aditi` picks the grid (unset is a fresh
//! fake grid): on a live grid the viewers log in as the accounts of its
//! credentials file ([`live`]), there is no grid-control handle, and a test
//! that needs one — or anything else the grid cannot give ([`Need`]) — is
//! skipped with its reason. aditi logins wait out the login cooldown
//! `sl-conformance` shares (`sl_repl::LoginCooldown`). Nothing assumes one
//! viewer: every handle is addressed by label, and every viewer has its own
//! directory, account and log.
//!
//! Artifacts land in `<target>/e2e/<test>/<grid>/<backend>/`: `grid.log`
//! (the grid and the test), and per viewer `<label>/viewer.log`,
//! `<label>/failures/` (the driver's failure artifacts) and `<label>/state/`.
//! A relogged viewer process writes its next session's output to
//! `<label>/viewer.<n>.log`; an in-process viewer's sessions share
//! `viewer.log`.
//! A machine with no GPU adapter skips a stage loudly.

pub mod backend;
mod error;
mod gpu;
pub mod grid;
pub mod live;
mod logs;
pub mod need;
mod stage;
pub mod watch;

pub use backend::{BACKEND_VARIABLE, Backend};
pub use error::{BodyError, StageError};
pub use grid::{GRID_VARIABLE, Grid};
pub use need::Need;
pub use stage::{FIRST_NAME, PASSWORD, Stage, StageBuilder};
pub use watch::WATCH_VARIABLE;
