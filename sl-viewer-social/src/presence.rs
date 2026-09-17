//! Away and Do Not Disturb, and the idle timer behind auto-AFK.
//!
//! The two session modes every surface asks about: the auto-reply decides
//! whether to answer, the Comm menu ticks its toggles, a name tag badges the
//! own avatar.

use bevy::prelude::*;

/// How long the away state must have held before input clears it (the
/// reference's `LLAgent::MIN_AFK_TIME`) — without it, the mouse move that
/// happens to arrive one frame after the auto-AFK fires would cancel it.
pub const MIN_AFK_SECS: f32 = 10.0;

/// The live presence state: the two session modes and the timers behind
/// auto-AFK. The two autorespond modes are **not** here — they are their own
/// persisted settings, read straight from the store wherever they are needed.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the two modes are independent (either, both or neither can be on) and the three \
              remaining flags are per-mode bookkeeping — an enum would have to enumerate every \
              combination to say the same thing"
)]
#[derive(Resource, Debug, Default)]
pub struct PresenceState {
    /// Whether the avatar is away.
    away: bool,
    /// Whether Do Not Disturb is on.
    do_not_disturb: bool,
    /// Seconds since the last user input (the reference `gAwayTriggerTimer`).
    idle_secs: f32,
    /// Seconds the away state has held (the reference `gAwayTimer`), used for
    /// the clear debounce and the quit-after-AFK timeout.
    away_secs: f32,
    /// The away state last advertised to the simulator, so the animation
    /// request is sent on the edge only.
    advertised_away: bool,
    /// The Do Not Disturb state last advertised, likewise.
    advertised_dnd: bool,
    /// Whether *we* sat the avatar down on going away, so returning only stands
    /// it back up when it was our doing.
    sat_on_away: bool,
}

impl PresenceState {
    /// Advance the idle clock, and the away clock while away.
    pub const fn tick(&mut self, dt: f32) {
        self.idle_secs += dt;
        if self.away {
            self.away_secs += dt;
        }
    }

    /// Seconds since the last user input.
    #[must_use]
    pub const fn idle_secs(&self) -> f32 {
        self.idle_secs
    }

    /// Seconds the away state has held.
    #[must_use]
    pub const fn away_secs(&self) -> f32 {
        self.away_secs
    }

    /// Restart the idle clock alone — there is no session to be away in yet,
    /// so the away clock is not the caller's business.
    pub const fn reset_idle(&mut self) {
        self.idle_secs = 0.0;
    }

    /// The away state if it differs from what was last advertised, marking it
    /// advertised in the same step; `None` when the wire already agrees. Read
    /// and mark cannot be separated, or a failed send would leave the two
    /// permanently out of step.
    pub const fn take_away_edge(&mut self) -> Option<bool> {
        if self.away == self.advertised_away {
            return None;
        }
        self.advertised_away = self.away;
        Some(self.away)
    }

    /// The Do Not Disturb state on the same terms as [`Self::take_away_edge`].
    pub const fn take_dnd_edge(&mut self) -> Option<bool> {
        if self.do_not_disturb == self.advertised_dnd {
            return None;
        }
        self.advertised_dnd = self.do_not_disturb;
        Some(self.do_not_disturb)
    }

    /// Whether *we* sat the avatar down on going away.
    #[must_use]
    pub const fn sat_on_away(&self) -> bool {
        self.sat_on_away
    }

    /// Record whether we sat the avatar down on going away.
    pub const fn set_sat_on_away(&mut self, sat: bool) {
        self.sat_on_away = sat;
    }

    /// Whether the avatar is away.
    #[must_use]
    pub const fn is_away(&self) -> bool {
        self.away
    }

    /// Whether Do Not Disturb is on.
    #[must_use]
    pub const fn is_do_not_disturb(&self) -> bool {
        self.do_not_disturb
    }

    /// Set the away state, restarting the away clock on a rising edge. The wire
    /// writes are reconciled by `advertise_presence`.
    pub const fn set_away(&mut self, away: bool) {
        if self.away != away {
            self.away = away;
            self.away_secs = 0.0;
        }
    }

    /// Set the Do Not Disturb state. The wire writes and the toast queue's
    /// drain are reconciled by `advertise_presence` and the hosts that read
    /// [`is_do_not_disturb`](Self::is_do_not_disturb).
    pub const fn set_do_not_disturb(&mut self, busy: bool) {
        self.do_not_disturb = busy;
    }

    /// Note user input: reset the idle clock and, once away has held long
    /// enough to be real, clear it (the reference's `MIN_AFK_TIME` debounce).
    pub fn note_activity(&mut self) {
        if self.away && self.away_secs > MIN_AFK_SECS {
            self.set_away(false);
        }
        self.idle_secs = 0.0;
    }
}
