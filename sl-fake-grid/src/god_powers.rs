//! God powers: what a grid answers an agent that asks to become a god
//! (`RequestGodlikePowers`) or to stop being one, and what its
//! `UntrustedSimulatorMessage` capability answers a post.
//!
//! Measured by the `god-tools` census on 2026-10-18
//! (`book/src/gridspec/estate.md` § God tools):
//!
//! - **OpenSim** grants an agent its *rights* level — a god account's user
//!   level (200 and up, where the grid allows grid gods), else 220 for the
//!   region's estate owner (`region_owner_is_god`) — as a `GrantGodlikePowers`
//!   carrying the level asked for or `0`, sent only when that level differs
//!   from the one the viewer was last told (`GodController`). An agent with no
//!   rights asking for them is told so in an agent alert,
//!   "/Request for god powers denied", and nothing when it gives them up.
//! - **Second Life** answers a resident's request with an `AlertMessage` keyed
//!   `SpecialPowersRequestFailedLogged`, and giving powers up with nothing. No
//!   test account is a god there, so a god's grant is not measured, and this
//!   flavour refuses every account.
//!
//! The capability: OpenSim answers a `GodKickUser` whose god session is not
//! the god's presence `401`, any other `200` with no body, and a message it
//! does not know `400`; Second Life answers any `GodKickUser` `200` with an
//! `undef` body, and a message it does not know `404` "Not Found". What the
//! kick then does is the roadmap's `server-fake-grid-god-tools`.

use std::time::Instant;

use sl_proto::{AgentKey, AlertInfo, ServerEvent, SimSession};

/// The god level OpenSim gives a region's estate owner in its own region
/// (`GodController.ImplicitGodLevels.RegionOwner`).
pub const ESTATE_OWNER_GOD_LEVEL: u8 = 220;

/// The lowest level OpenSim counts as a god (`GodController.CanBeGod`).
pub const GOD_LEVEL_FLOOR: u8 = 200;

/// OpenSim's refusal, as its `GodsModule` words it.
const OPENSIM_DENIED: &str = "/Request for god powers denied";

/// Second Life's refusal: the text and the catalogue key it arrives under.
const SECOND_LIFE_DENIED: &str = "Request for special powers failed. This request has been logged.";

/// The key of [`SECOND_LIFE_DENIED`].
const SECOND_LIFE_DENIED_KEY: &str = "SpecialPowersRequestFailedLogged";

/// How a grid answers a request for god powers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowersAnswer {
    /// OpenSim: by the agent's rights, a grant only when the level changes,
    /// an agent alert for a refusal.
    ByRights,
    /// Second Life, as a resident is answered: every request for powers
    /// refused in a keyed alert, giving them up unanswered.
    Refused,
}

/// One session's god state: the level its agent may hold and the level its
/// viewer was last told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GodMode {
    /// The level the agent's rights give it; below [`GOD_LEVEL_FLOOR`] for an
    /// agent that may not be a god.
    pub rights: u8,
    /// The level the viewer was last granted (`0` until a grant).
    pub told: u8,
}

impl GodMode {
    /// The god state of an agent whose account is at `account_level`, and who
    /// is the estate owner where `estate_owner`, on a grid answering by
    /// `answer`.
    #[must_use]
    pub const fn at_login(answer: PowersAnswer, account_level: u8, estate_owner: bool) -> Self {
        let rights = match answer {
            PowersAnswer::ByRights if account_level >= GOD_LEVEL_FLOOR => account_level,
            PowersAnswer::ByRights if estate_owner => ESTATE_OWNER_GOD_LEVEL,
            PowersAnswer::Refused | PowersAnswer::ByRights => 0,
        };
        Self { rights, told: 0 }
    }
}

/// Answers a drained `RequestGodlikePowers` as `answer` says, keeping the
/// level told in `mode`. Any other event is left alone.
pub(crate) fn answer_god_request(
    answer: PowersAnswer,
    mode: &mut GodMode,
    agent_id: AgentKey,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) {
    let ServerEvent::RequestGodlikePowers { godlike, .. } = event else {
        return;
    };
    let outcome = match answer {
        PowersAnswer::Refused if *godlike => sim.send_alert_message(
            SECOND_LIFE_DENIED,
            &[AlertInfo {
                message: SECOND_LIFE_DENIED_KEY.to_owned(),
                extra_params: String::new(),
            }],
            &[],
            now,
        ),
        PowersAnswer::Refused => Ok(()),
        PowersAnswer::ByRights => {
            let level = if *godlike && mode.rights >= GOD_LEVEL_FLOOR {
                mode.rights
            } else {
                0
            };
            let granted = if level == mode.told {
                Ok(())
            } else {
                mode.told = level;
                sim.send_grant_godlike_powers(level, now)
            };
            if *godlike && mode.rights < GOD_LEVEL_FLOOR {
                granted.and_then(|()| {
                    sim.send_agent_alert_message(agent_id, false, OPENSIM_DENIED, now)
                })
            } else {
                granted
            }
        }
    };
    if let Err(error) = outcome {
        tracing::warn!("answering a request for god powers failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{ESTATE_OWNER_GOD_LEVEL, GodMode, PowersAnswer};

    /// OpenSim's rights: a god account's level, else the estate owner's 220,
    /// else none; Second Life refuses everybody.
    #[test]
    fn rights_follow_the_account_and_the_estate() {
        let rights = |answer, level, owner| GodMode::at_login(answer, level, owner).rights;
        assert_eq!(rights(PowersAnswer::ByRights, 250, false), 250);
        assert_eq!(rights(PowersAnswer::ByRights, 250, true), 250);
        assert_eq!(
            rights(PowersAnswer::ByRights, 0, true),
            ESTATE_OWNER_GOD_LEVEL
        );
        assert_eq!(rights(PowersAnswer::ByRights, 100, false), 0);
        assert_eq!(rights(PowersAnswer::Refused, 250, true), 0);
    }
}
