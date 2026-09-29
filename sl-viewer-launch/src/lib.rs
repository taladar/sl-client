//! Launching Second Life viewer processes for a harness, and stopping them
//! without stranding their sessions.
//!
//! Two harnesses run the real viewer binary: the Firestorm cross-check
//! (`sl-crosscheck`), one viewer at a time to a deadline, and the end-to-end
//! stage, several at once under a test. Both need the same two things, and
//! both are easy to get subtly wrong:
//!
//! - **Confinement** ([`launch`]): a viewer's settings, caches and logs go into
//!   a directory of the run's own, never the operator's — this workspace's
//!   viewer rewrites its settings on the way out.
//! - **A graceful stop** ([`process`]): `SIGTERM`, which the viewer turns into
//!   a logout, then a logout grace, and only then `SIGKILL` — a killed viewer
//!   leaves the grid holding its session, and the *next* login fails.

pub mod launch;
pub mod process;

pub use launch::{Launch, ViewerDir, confined_env};
pub use process::{
    Ending, Error, LOGOUT_GRACE, Ran, RunningViewer, interrupt_flag, is_executable, run, stop_all,
};
