//! The end-to-end stage: an in-process fake grid, several real viewers
//! logged into it, and the handles a test drives them with.
//!
//! A test names its viewers, and [`StageBuilder::run`] does the rest, once
//! per backend:
//!
//! 1. starts a fresh `sl-fake-grid` (the stock scene's region unless told
//!    otherwise) with an account `Stage <label>` per viewer;
//! 2. starts each viewer — as the real binary, `--headless
//!    --automation-socket`, confined to its directory through
//!    `sl-viewer-launch` ([`Backend::Process`]), or as the viewer's own
//!    builder's App on an in-process host ([`Backend::InProcess`]) — and
//!    waits until each has logged in and its scene has settled;
//! 3. hands the body a [`Stage`]: a `sl-viewer-driver` handle per viewer, the
//!    grid, and markers ([`Stage::mark`], [`Stage::wait_marker`]);
//! 4. takes it all down — also when the body failed or panicked — by asking
//!    each viewer to log out, and fails the test if one would not or if the
//!    grid still holds a session afterwards.
//!
//! `SL_E2E_BACKEND=process|in-process|both` picks the backends (unset is
//! both). Nothing assumes one viewer: every handle is addressed by label, and
//! every viewer has its own directory, account and log.
//!
//! Artifacts land in `<target>/e2e/<test>/<backend>/`: `grid.log` (the grid
//! and the test), and per viewer `<label>/viewer.log`, `<label>/failures/`
//! (the driver's failure artifacts) and `<label>/state/`. A machine with no
//! GPU adapter skips a stage loudly.

pub mod backend;
mod error;
mod gpu;
mod logs;
mod stage;

pub use backend::{BACKEND_VARIABLE, Backend};
pub use error::{BodyError, StageError};
pub use stage::{FIRST_NAME, Stage, StageBuilder};
