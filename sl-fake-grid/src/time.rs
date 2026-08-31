//! The grid's clock, injectable so a test can drive it.
//!
//! Every grid-side stamp — the `now` each [`sl_proto::SimSession`] entry
//! point takes, the instant a session machine is created at, the
//! `EventQueueGet` hold deadline — is drawn from one [`Now`] closure held by
//! the grid core and by every live session's shared handle. The sans-I/O
//! core was always clock-injectable; before this the grid simply never
//! injected, calling [`Instant::now`] at a dozen scattered sites.
//!
//! The default is the system clock. A test that pauses tokio's timer passes
//! [`tokio_clock`] instead, so the machines and the timer tasks that fire
//! their deadlines agree on what time it is.

use std::sync::Arc;
use std::time::Instant;

/// The grid's source of "now": every instant the grid stamps a machine with
/// comes from one of these.
pub type Now = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The system clock ([`Instant::now`]) — what a grid uses unless
/// [`crate::FakeGridBuilder::clock`] says otherwise.
#[must_use]
pub fn system_clock() -> Now {
    Arc::new(Instant::now)
}

/// Tokio's timer clock, which `tokio::time::pause` freezes and
/// `tokio::time::advance` moves.
///
/// A test with paused time must hand the grid this one: otherwise the
/// session machines stamp their deadlines in real time while the timer tasks
/// that fire them sleep in virtual time, and the two never meet.
#[must_use]
pub fn tokio_clock() -> Now {
    Arc::new(|| tokio::time::Instant::now().into_std())
}
