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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// The calendar instant a seeded grid's wall clock starts at,
/// 2023-11-14T22:13:20Z — the creation date the grid stamps a minted
/// inventory item with, for the same reason.
pub const SEEDED_EPOCH_SECONDS: u64 = 1_700_000_000;

/// The grid's wall clock: the calendar time it stamps into what it says — a
/// login's `seconds_since_epoch`, the region's time message, an instant
/// message's or a group notice's date, an access pass's expiry.
///
/// It is not a second clock. It is the grid's [`Now`] counted from one
/// anchor: the calendar instant the grid started at, paired with the instant
/// its clock read then. So it moves exactly as the injected clock does —
/// frozen under a paused tokio timer, an hour ahead on a clock skewed an hour
/// — and a grid that reads it twice from one start reads the same times. The
/// anchor is the machine's time when the grid starts, unless
/// [`crate::FakeGridBuilder::epoch`] names one, or the grid is seeded
/// ([`crate::FakeGridBuilder::deterministic`]), when it is
/// [`SEEDED_EPOCH_SECONDS`].
#[derive(Clone)]
pub struct WallClock {
    /// The grid's clock.
    now: Now,
    /// The calendar instant the grid started at.
    epoch: SystemTime,
    /// What [`Self::now`] read when the grid started.
    at: Instant,
}

impl core::fmt::Debug for WallClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WallClock")
            .field("epoch", &self.epoch)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl Default for WallClock {
    /// The machine's clock, from the machine's time now: what a world built
    /// outside a grid reads.
    fn default() -> Self {
        Self::starting_at(system_clock(), SystemTime::now())
    }
}

impl WallClock {
    /// A wall clock reading `epoch` now, and moving as `now` does.
    #[must_use]
    pub fn starting_at(now: Now, epoch: SystemTime) -> Self {
        let at = now();
        Self { now, epoch, at }
    }

    /// The calendar time now.
    #[must_use]
    pub fn read(&self) -> SystemTime {
        let elapsed = (self.now)().saturating_duration_since(self.at);
        self.epoch.checked_add(elapsed).unwrap_or(self.epoch)
    }

    /// How long after the UNIX epoch it is now (zero before it).
    fn since_unix_epoch(&self) -> Duration {
        self.read()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
    }

    /// The calendar time now as whole UNIX seconds.
    #[must_use]
    pub fn unix_seconds(&self) -> u64 {
        self.since_unix_epoch().as_secs()
    }

    /// The calendar time now as UNIX microseconds.
    #[must_use]
    pub fn unix_micros(&self) -> u64 {
        u64::try_from(self.since_unix_epoch().as_micros()).unwrap_or(u64::MAX)
    }

    /// The calendar time now as the `u32` UNIX seconds an instant message
    /// carries.
    #[must_use]
    pub fn unix_seconds_u32(&self) -> u32 {
        u32::try_from(self.unix_seconds()).unwrap_or(u32::MAX)
    }

    /// The calendar time now as the `i32` `time_t` an inventory item's
    /// creation date or an access-list entry carries.
    #[must_use]
    pub fn unix_seconds_i32(&self) -> i32 {
        i32::try_from(self.unix_seconds()).unwrap_or(i32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use pretty_assertions::assert_eq;

    use super::*;

    /// The wall clock moves exactly as the grid's clock does, from its
    /// anchor.
    #[test]
    fn the_wall_clock_follows_the_grid_clock() {
        let start = Instant::now();
        let offset = Arc::new(Mutex::new(Duration::ZERO));
        let read = Arc::clone(&offset);
        let now: Now = Arc::new(move || {
            start
                .checked_add(read.lock().map_or(Duration::ZERO, |guard| *guard))
                .unwrap_or(start)
        });
        let epoch = UNIX_EPOCH
            .checked_add(Duration::from_secs(SEEDED_EPOCH_SECONDS))
            .unwrap_or(UNIX_EPOCH);
        let wall = WallClock::starting_at(now, epoch);
        assert_eq!(wall.unix_seconds(), SEEDED_EPOCH_SECONDS);
        if let Ok(mut guard) = offset.lock() {
            *guard = Duration::from_millis(90_500);
        }
        assert_eq!(wall.unix_seconds(), SEEDED_EPOCH_SECONDS + 90);
        assert_eq!(
            wall.unix_micros(),
            SEEDED_EPOCH_SECONDS * 1_000_000 + 90_500_000
        );
    }
}
