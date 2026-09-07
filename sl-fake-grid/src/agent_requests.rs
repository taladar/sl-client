//! The agent-directed asks a simulator answers about the agent itself: what it
//! is wearing, where it just set its home, and what becomes of the deprecated
//! paths it reaches for.
//!
//! These have nothing in common with the world fixtures ([`crate::world`]) —
//! none of them is about the region's content — but they share a shape: each
//! is a policy question about *this* session's agent, answered either from
//! state the session already holds or from the [`AgentPolicy`] the grid was
//! built with.
//!
//! The estate commands themselves moved to [`crate::estate`] when the fake grid
//! learned to answer more than one of them; what is left of the estate here is
//! [`AgentPolicy::estate_manager`], the gate they are all behind.

use std::time::Instant;

use sl_proto::{AgentKey, ServerEvent, SimSession, TransactionId};

use crate::inventory::LegacyUdpInventory;
use crate::world::SceneFixtures;

/// The alert a simulator answers a stored Set-Home with. OpenSim's
/// `LandManagementModule` notes that the text has to be exactly this, or the
/// reference viewer does not save its home screenshot.
const HOME_SET: &str = "Home position set.";

/// The alert a simulator answers a refused Set-Home with, verbatim from
/// OpenSim's `LandManagementModule`.
const HOME_REFUSED: &str = "You are not allowed to set your home location in this parcel.";

/// The `ErrorMessage` a refused deprecated inventory fetch carries.
const LEGACY_INVENTORY_REFUSED: &str = "The UDP inventory fetch is deprecated on this grid; use the \
     FetchInventoryDescendents2 capability.";

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
    /// How the deprecated UDP inventory fetch is answered
    /// ([`LegacyUdpInventory`], which follows [`ImitatedGrid`](crate::ImitatedGrid)).
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
        // The deprecated UDP inventory fetch, and which of the three roads
        // this grid takes is the flavour's answer, not this module's: a grid
        // imitating OpenSim still serves it out of the session's own inventory
        // tree, and one imitating Second Life does not have the path at all.
        ServerEvent::RequestInventoryDescendents {
            folder_id,
            owner_id,
            sort_order,
            fetch_folders,
            fetch_items,
        } => match policy.legacy_udp_inventory {
            LegacyUdpInventory::Ignored => {}
            LegacyUdpInventory::Served => {
                match sim.send_inventory_descendents(
                    *folder_id,
                    *owner_id,
                    *sort_order,
                    *fetch_folders,
                    *fetch_items,
                    now,
                ) {
                    // A folder this grid's inventory does not have. A real
                    // simulator answers nothing either; the client is holding
                    // an id from somewhere else.
                    Ok(false) => tracing::debug!(
                        "a UDP inventory fetch named {folder_id:?}, which this grid does not have"
                    ),
                    Ok(true) => {}
                    Err(error) => {
                        tracing::warn!("serving the deprecated inventory fetch failed: {error}");
                    }
                }
            }
            LegacyUdpInventory::Refused => {
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
        },
        _other => {}
    }
}
