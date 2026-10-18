//! **God mode**: the agent's god level as the simulator last granted it, and
//! the tip the reference shows on every grant.
//!
//! A `GrantGodlikePowers` ([`SlSessionEvent::GodlikePowersGranted`]) is the
//! simulator's answer to a request for god powers or to giving them up. The
//! reference's `set_god_level` keeps the level and raises `EnteringGodMode`
//! (`LEVEL` the new level) for a level above zero and `LeavingGodMode`
//! (`LEVEL` the level being left) for zero. What each grid grants is in
//! `book/src/gridspec/estate.md` § God tools: OpenSim grants a god account its
//! user level and an estate owner 220 in its own region, and tells a resident
//! "Request for god powers denied" in an alert, which the alert host shows.

use bevy::prelude::*;

use sl_client_bevy::{SlEvent, SlSessionEvent};

use crate::notifications::{NotificationArgs, ShowNotification};

/// The agent's god level as the simulator last granted it (`0`: not a god);
/// the reference's `gAgent.getGodLevel()`.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AgentGodLevel(pub u8);

/// The tip a grant of `granted` raises for an agent that was at `previous`:
/// `EnteringGodMode` at the new level, or `LeavingGodMode` at the old one.
#[must_use]
pub fn god_mode_notice(previous: u8, granted: u8) -> ShowNotification {
    let (template, level) = if granted > 0 {
        ("EnteringGodMode", granted)
    } else {
        ("LeavingGodMode", previous)
    };
    let mut notice = ShowNotification::new(template);
    notice.args = NotificationArgs::from_pairs(vec![("LEVEL".to_owned(), level.to_string())]);
    notice
}

/// Keeps [`AgentGodLevel`] and raises the reference's tip on every grant.
pub fn ingest_god_level(
    mut events: MessageReader<SlEvent>,
    mut level: ResMut<AgentGodLevel>,
    mut show: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        if let SlSessionEvent::GodlikePowersGranted { god_level } = &event.0 {
            show.write(god_mode_notice(level.0, *god_level));
            level.0 = *god_level;
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::god_mode_notice;

    /// A grant above zero enters god mode at the granted level.
    #[test]
    fn a_grant_enters_god_mode_at_its_level() {
        let notice = god_mode_notice(0, 250);
        assert_eq!(notice.template, "EnteringGodMode");
        assert_eq!(
            notice.args.pairs(),
            [("LEVEL".to_owned(), "250".to_owned())]
        );
    }

    /// A grant of zero leaves god mode, naming the level that was left.
    #[test]
    fn a_zero_grant_leaves_at_the_old_level() {
        let notice = god_mode_notice(220, 0);
        assert_eq!(notice.template, "LeavingGodMode");
        assert_eq!(
            notice.args.pairs(),
            [("LEVEL".to_owned(), "220".to_owned())]
        );
    }
}
