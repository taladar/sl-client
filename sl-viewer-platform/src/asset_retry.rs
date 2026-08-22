//! Bounded exponential-backoff retry shared by the asset managers.
//!
//! A background asset fetch that fails (a transient `GetTexture` / `GetMesh` 503,
//! a connection reset, a decode blip) used to be terminal: the manager dropped the
//! request and never asked again, so a one-shot consumer — a terrain detail
//! texture, an avatar bake, a static mesh whose one object update already
//! arrived — was missing for the rest of the session, while the F3 pipeline
//! overlay showed nothing left to load (the store keeps only weak references, so a
//! failed entry with no strong holder is swept from its stats). This policy lets a
//! failed fetch be re-issued a bounded number of times with growing backoff, so a
//! transient failure recovers instead of stranding the asset.

/// The number of attempts (the initial fetch plus retries) after which a fetch is
/// given up on. With [`backoff_secs`] this spans roughly half a minute of
/// retrying — long enough to ride out a transient service blip without hammering a
/// genuinely-dead endpoint forever.
pub const MAX_RETRY_ATTEMPTS: u32 = 6;

/// The first retry's delay; each subsequent retry doubles it.
const BASE_BACKOFF_SECS: f64 = 0.5;

/// The ceiling on a single retry's delay.
pub const MAX_BACKOFF_SECS: f64 = 30.0;

/// The delay before the `attempts`-th retry (1-based): `0.5, 1, 2, 4, 8, 16, …`
/// seconds, doubling each time and capped at [`MAX_BACKOFF_SECS`]. `attempts` of
/// `0` is treated as the first wait.
#[must_use]
pub fn backoff_secs(attempts: u32) -> f64 {
    let steps = attempts.saturating_sub(1).min(16);
    let mut secs = BASE_BACKOFF_SECS;
    for _step in 0..steps {
        secs *= 2.0;
        if secs >= MAX_BACKOFF_SECS {
            break;
        }
    }
    secs.min(MAX_BACKOFF_SECS)
}

/// The per-asset retry bookkeeping: how many attempts have failed and when the
/// next one is due (in monotonic [`Time::elapsed_secs_f64`](bevy::time::Time::elapsed_secs_f64) seconds).
#[derive(Clone, Copy, Debug)]
pub struct RetryState {
    /// The number of failed attempts so far.
    pub attempts: u32,
    /// The monotonic time at which the next retry is due.
    pub next_at: f64,
}

impl RetryState {
    /// The retry state after another failed attempt at `now`, or `None` once the
    /// attempts are exhausted ([`MAX_RETRY_ATTEMPTS`]) and the fetch should be given
    /// up on. `previous` is the id's prior retry state, if it had already failed.
    #[must_use]
    pub fn after_failure(previous: Option<Self>, now: f64) -> Option<Self> {
        let attempts = previous.map_or(0, |state| state.attempts).saturating_add(1);
        if attempts >= MAX_RETRY_ATTEMPTS {
            return None;
        }
        Some(Self {
            attempts,
            next_at: now + backoff_secs(attempts),
        })
    }

    /// Whether the next retry is due at `now`.
    #[must_use]
    pub const fn due(&self, now: f64) -> bool {
        now >= self.next_at
    }

    /// Mark this retry as *issued*: keep the accumulated [`attempts`](Self::attempts)
    /// count but park [`next_at`](Self::next_at) at infinity so the entry is not
    /// selected as due again on its own. The re-issued fetch's result reschedules
    /// it — a failure feeds this state back through [`after_failure`](Self::after_failure)
    /// (incrementing the count, then giving up at [`MAX_RETRY_ATTEMPTS`]), a success
    /// clears it. Without preserving the count here the re-issue path dropped the
    /// retry state entirely, so every failure saw no prior state and reset to
    /// attempt 1 — the backoff looped forever at "retry 1/N" and never gave up.
    #[must_use]
    pub const fn issued(self) -> Self {
        Self {
            attempts: self.attempts,
            next_at: f64::INFINITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_RETRY_ATTEMPTS, RetryState, backoff_secs};
    use pretty_assertions::assert_eq;

    /// Floats compare with a tolerance (the workspace forbids exact `f64` equality).
    fn close(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-9
    }

    #[test]
    fn backoff_doubles_then_caps() {
        assert!(close(backoff_secs(0), 0.5));
        assert!(close(backoff_secs(1), 0.5));
        assert!(close(backoff_secs(2), 1.0));
        assert!(close(backoff_secs(3), 2.0));
        assert!(close(backoff_secs(4), 4.0));
        // Deep attempts saturate at the ceiling rather than growing unbounded.
        assert!(close(backoff_secs(20), 30.0));
    }

    #[test]
    fn after_failure_counts_up_then_gives_up() {
        // First failure schedules attempt 1, due after the base backoff.
        let Some(first) = RetryState::after_failure(None, 100.0) else {
            unreachable!("the first retry is scheduled");
        };
        assert_eq!(first.attempts, 1);
        assert!(close(first.next_at, 100.5));
        assert!(!first.due(100.4));
        assert!(first.due(100.5));
        // Each subsequent failure increments the count until exhaustion.
        let mut state = first;
        for expected in 2..MAX_RETRY_ATTEMPTS {
            let Some(next) = RetryState::after_failure(Some(state), 0.0) else {
                unreachable!("still within the retry budget");
            };
            state = next;
            assert_eq!(state.attempts, expected);
        }
        // The final failure exhausts the budget and gives up (no further retry).
        assert!(RetryState::after_failure(Some(state), 0.0).is_none());
    }

    #[test]
    fn issued_preserves_count_so_reissue_escalates_and_gives_up() {
        // Regression: the poll loop re-issued a due retry by *dropping* the retry
        // state, so the next failure saw no prior state (`after_failure(None)`) and
        // reset to attempt 1 — looping "retry 1/N" forever and never giving up
        // (observed live on aditi against a 503ing asset CDN). `issued()` keeps the
        // count while parking the entry so it is not re-selected as due on its own.
        let Some(mut state) = RetryState::after_failure(None, 0.0) else {
            unreachable!("the first retry is scheduled");
        };
        assert_eq!(state.attempts, 1);
        for expected in 2..MAX_RETRY_ATTEMPTS {
            // Model one poll cycle: the retry comes due and is issued (count kept,
            // parked so `due` is false), then the re-issued fetch fails again.
            let parked = state.issued();
            assert_eq!(parked.attempts, expected - 1);
            assert!(!parked.due(f64::MAX));
            let Some(next) = RetryState::after_failure(Some(parked), 0.0) else {
                unreachable!("still within the retry budget");
            };
            state = next;
            assert_eq!(state.attempts, expected);
        }
        // Once the budget is spent, an issued-then-failed retry finally gives up —
        // the loop terminates instead of running forever.
        assert!(RetryState::after_failure(Some(state.issued()), 0.0).is_none());
    }
}
