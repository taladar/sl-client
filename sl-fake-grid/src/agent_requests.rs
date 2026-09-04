//! The agent-directed asks a simulator answers about the agent itself: what it
//! is wearing, where it just set its home, the estate commands it is allowed
//! to issue, and the deprecated paths it is refused.
//!
//! These have nothing in common with the world fixtures ([`crate::world`]) —
//! none of them is about the region's content — but they share a shape: each
//! is a policy question about *this* session's agent, answered either from
//! state the session already holds or from the [`AgentPolicy`] the grid was
//! built with.

use std::time::Instant;

use sl_proto::{AgentKey, ServerEvent, SimSession, TransactionId};

use crate::world::SceneFixtures;

/// The alert a simulator answers a stored Set-Home with. OpenSim's
/// `LandManagementModule` notes that the text has to be exactly this, or the
/// reference viewer does not save its home screenshot.
const HOME_SET: &str = "Home position set.";

/// The alert a simulator answers a refused Set-Home with, verbatim from
/// OpenSim's `LandManagementModule`.
const HOME_REFUSED: &str = "You are not allowed to set your home location in this parcel.";

/// The estate command a viewer's "regenerate the map tile" nudge issues.
const REFRESH_MAP_VISIBILITY: &str = "refreshmapvisibility";

/// The alert OpenSim's `refreshmapvisibility` handler answers with once the
/// tile has been regenerated.
const MAP_REGENERATED: &str = "Terrain map generated";

/// The `ErrorMessage` a refused deprecated inventory fetch carries.
const LEGACY_INVENTORY_REFUSED: &str = "The UDP inventory fetch is deprecated on this grid; use the \
     FetchInventoryDescendents2 capability.";

/// How the simulator answers the deprecated UDP inventory fetch
/// (`FetchInventoryDescendents`).
///
/// The live grids take different roads — OpenSim still serves it, Second Life
/// silently drops it — and the fake grid can take either of the two a grid that
/// does *not* serve it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LegacyUdpInventory {
    /// Answer it with a `FeatureDisabled` naming the refused feature — what
    /// Second Life does for a message it has blacklisted, and what the
    /// reference viewer logs as its "Blacklisted Feature Response".
    ///
    /// The default, and the only road that produces something to assert:
    /// silence is indistinguishable from a lost packet, and the fake grid does
    /// not serve UDP inventory at all.
    #[default]
    Refused,
    /// Drop it silently — what Second Life empirically does to this particular
    /// deprecated fetch (aditi, 2026-08-12).
    Ignored,
}

/// What the grid permits an agent, and how it answers the deprecated paths.
///
/// Grid-wide but read per session, because the estate half is about *who* is
/// asking: an agent with no estate powers gets the silence OpenSim gives it,
/// which is a different observable from the alert an estate manager gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentPolicy {
    /// Whether this session's agent may issue estate commands
    /// ([`AccountConfig::estate_manager`](crate::AccountConfig)). OpenSim
    /// returns without a word from an estate command an agent has no power
    /// for, so this is the difference between an alert and nothing at all.
    pub estate_manager: bool,
    /// How the deprecated UDP inventory fetch is answered.
    pub legacy_udp_inventory: LegacyUdpInventory,
}

/// Answers one drained [`ServerEvent`] about the agent itself. Anything this
/// module has no answer for is left alone.
pub(crate) fn answer_agent_request(
    policy: AgentPolicy,
    world: &SceneFixtures,
    agent_id: AgentKey,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) {
    match event {
        // What the simulator holds the agent to be wearing. An account nothing
        // dressed is answered with an *empty* outfit rather than left silent:
        // "you are wearing nothing" is what a simulator says about a stripped
        // avatar, and a viewer that gets no answer waits out its own timeout
        // before concluding the same thing.
        ServerEvent::RequestAgentWearables => {
            let (serial, worn) = sim.agent_wearables();
            let worn = worn.to_vec();
            if let Err(error) = sim.send_agent_wearables_update(serial, &worn, now) {
                tracing::warn!("answering an agent wearables request failed: {error}");
            }
        }
        // Set-Home. *Every* outcome is answered — that is what makes it the
        // one deterministic way to provoke an `AgentAlertMessage` — and which
        // outcome depends on the rule OpenSim applies: the land's owner may
        // set home on it, and nobody else may.
        ServerEvent::SetStartLocation { position, .. } => {
            let owns_the_land = world
                .parcel_at(position.x(), position.y())
                .is_some_and(|parcel| parcel.owner.uuid() == agent_id.uuid());
            let message = if owns_the_land {
                HOME_SET
            } else {
                HOME_REFUSED
            };
            if let Err(error) = sim.send_agent_alert_message(agent_id, false, message, now) {
                tracing::warn!("answering a set-home request failed: {error}");
            }
        }
        // The one estate command the fake grid answers: the viewer's
        // "regenerate the map tile" nudge, whose reply is a broadcast-style
        // `AlertMessage` rather than an agent-addressed one.
        ServerEvent::EstateOwnerRequest { method, .. } if method == REFRESH_MAP_VISIBILITY => {
            if !policy.estate_manager {
                tracing::debug!("{REFRESH_MAP_VISIBILITY} from an agent with no estate powers");
                return;
            }
            // The fake grid's map tiles are static, so there is nothing to
            // regenerate and the answer is the success one. The cool-down and
            // generator-unavailable branches OpenSim also has are states this
            // grid cannot be in.
            if let Err(error) = sim.send_alert_message(MAP_REGENERATED, &[], &[], now) {
                tracing::warn!("answering an estate map regeneration failed: {error}");
            }
        }
        // The deprecated UDP inventory fetch. It reaches the simulator half as
        // a raw forward — nothing decodes it into a typed event, because no
        // grid worth speaking to still serves it.
        ServerEvent::ClientMessage(message)
            if matches!(**message, sl_wire::AnyMessage::FetchInventoryDescendents(_)) =>
        {
            if policy.legacy_udp_inventory == LegacyUdpInventory::Ignored {
                return;
            }
            if let Err(error) = sim.send_feature_disabled(
                &sl_proto::FeatureDisabled {
                    message: LEGACY_INVENTORY_REFUSED.to_owned(),
                    agent: agent_id,
                    transaction: TransactionId::from(uuid::Uuid::nil()),
                },
                now,
            ) {
                tracing::warn!("refusing the deprecated inventory fetch failed: {error}");
            }
        }
        _other => {}
    }
}
