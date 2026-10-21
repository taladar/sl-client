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

use std::collections::HashMap;
use std::sync::Arc;

use sl_proto::{
    AgentKey, InventoryKey, RegionCoordinates, ServerEvent, SimSession, TransactionId, Wearable,
    WearableType,
};
use sl_types::lsl::Vector;
use uuid::Uuid;

use crate::imitates::{HomeNotice, HomeNotices, HomePolicy};
use crate::inventory::LegacyUdpInventory;
use crate::world::SceneFixtures;

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
    /// What an `AgentWearablesRequest` is answered with ([`WearablesReply`],
    /// which follows [`ImitatedGrid`](crate::ImitatedGrid)).
    pub wearables_reply: WearablesReply,
}

/// What a simulator answers an `AgentWearablesRequest` with.
///
/// Measured by the `outfits` census (2026-10-20,
/// `book/src/gridspec/appearance.md` § Outfits): OpenSim answers the outfit
/// it holds, which the last `AgentIsNowWearing` set and a new login keeps;
/// Second Life answers four fixed items whatever is worn — the ids the
/// reference viewer sends in its own dummy `AgentIsNowWearing`
/// (`LLAgentWearables::sendDummyAgentWearablesUpdate`), "the same as returned
/// by the modified sim" — and keeps the outfit in the Current Outfit Folder
/// alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WearablesReply {
    /// The outfit the simulator holds.
    #[default]
    Outfit,
    /// The four placeholder items, for wearable types 1 to 4, each naming
    /// its fixed asset ([`PLACEHOLDER_ASSET_IDS`]).
    Placeholders,
}

/// The assets Second Life names for the four placeholder items, in order:
/// fixed, and served by nothing the census found (`outfits`, 2026-10-20).
pub const PLACEHOLDER_ASSET_IDS: [Uuid; 4] = [
    Uuid::from_u128(0xd3d2_5329_42a7_4526_a622_7db0_c33d_72c3),
    Uuid::from_u128(0x57ca_535e_1f23_49e5_a478_a443_a538_0564),
    Uuid::from_u128(0xbb10_89a8_3060_4722_9692_8b8e_d1ab_7a29),
    Uuid::from_u128(0x0965_6fd9_a624_491d_9370_919a_a721_aa09),
];

impl WearablesReply {
    /// The wearables the reply states, given the outfit the simulator holds.
    #[must_use]
    pub fn stated(self, held: &[Wearable]) -> Vec<Wearable> {
        match self {
            Self::Outfit => held.to_vec(),
            Self::Placeholders => (1_u8..)
                .zip(
                    Wearable::PLACEHOLDER_ITEM_IDS
                        .into_iter()
                        .zip(PLACEHOLDER_ASSET_IDS),
                )
                .map(|(wearable_type, (id, asset))| Wearable {
                    item_id: InventoryKey::from(id),
                    asset_id: Some(asset),
                    wearable_type: WearableType::from_code(wearable_type),
                })
                .collect(),
        }
    }
}

/// A home an account set: the region, and where in it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredHome {
    /// The region's id.
    pub(crate) region_id: Uuid,
    /// Where in the region.
    pub(crate) position: RegionCoordinates,
    /// The direction to face on arrival.
    pub(crate) look_at: Vector,
}

/// Every account's home, once it has set one: grid-wide, since a home is the
/// account's and not the region's, and shared by every session of the grid.
#[derive(Debug, Clone, Default)]
pub(crate) struct Homes(Arc<parking_lot::Mutex<HashMap<AgentKey, StoredHome>>>);

impl Homes {
    /// The home `agent` set, if it has.
    pub(crate) fn get(&self, agent: AgentKey) -> Option<StoredHome> {
        self.0.lock().get(&agent).cloned()
    }

    /// Records `home` as `agent`'s.
    pub(crate) fn set(&self, agent: AgentKey, home: StoredHome) {
        let _previous = self.0.lock().insert(agent, home);
    }
}

/// Where a session's Set-Home is stored and by which rule: the grid's homes,
/// the region the session is of, and the grid's home policy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HomeStore<'a> {
    /// Every account's home.
    pub(crate) homes: &'a Homes,
    /// The session's region.
    pub(crate) region_id: Uuid,
    /// Who may set home where, and what the grid says about it
    /// ([`ImitatedGrid::home_policy`](crate::ImitatedGrid::home_policy)).
    pub(crate) policy: &'a HomePolicy,
}

/// What a Set-Home came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetHome {
    /// The location was stored.
    Stored,
    /// The set-home rule refused it.
    Refused,
    /// The position is outside the region.
    Outside,
    /// The request named no location.
    NoLocation,
}

impl SetHome {
    /// What `notices` says about this outcome.
    pub(crate) const fn notice(self, notices: &HomeNotices) -> HomeNotice {
        match self {
            Self::Stored => notices.stored,
            Self::Refused => notices.refused,
            Self::Outside => notices.outside,
            Self::NoLocation => notices.no_location,
        }
    }
}

/// Applies the grid's set-home rule to a Set-Home of `position` by `agent_id`,
/// storing it where the rule allows: the land's owner may set home on it, and
/// on a grid that lets them an estate manager may set home anywhere in the
/// region (OpenSim's `IsAdministrator`).
pub(crate) fn set_home(
    estate_manager: bool,
    store: HomeStore<'_>,
    world: &SceneFixtures,
    agent_id: AgentKey,
    position: RegionCoordinates,
    look_at: &Vector,
) -> SetHome {
    let Some(parcel) = world.parcel_at(position.x(), position.y()) else {
        return SetHome::Outside;
    };
    let allowed = parcel.owner.uuid() == agent_id.uuid()
        || (store.policy.managers_may_set_home && estate_manager);
    if !allowed {
        return SetHome::Refused;
    }
    store.homes.set(
        agent_id,
        StoredHome {
            region_id: store.region_id,
            position,
            look_at: look_at.clone(),
        },
    );
    SetHome::Stored
}

/// Tells `agent_id` `notice`, unless the grid leaves a repeat of the last
/// notice unsaid and `last` is that notice, told within the quiet.
pub(crate) fn tell(
    sim: &mut SimSession,
    agent_id: AgentKey,
    notice: HomeNotice,
    quiet: (
        Option<std::time::Duration>,
        &mut Option<(Instant, HomeNotice)>,
    ),
    now: Instant,
) {
    let (quiet, last) = quiet;
    if let (Some(quiet), Some((at, told))) = (quiet, *last)
        && told == notice
        && now.saturating_duration_since(at) < quiet
    {
        return;
    }
    if notice != HomeNotice::Nothing {
        *last = Some((now, notice));
    }
    let sent = match notice {
        HomeNotice::Nothing => Ok(()),
        HomeNotice::AgentAlert(message) => {
            sim.send_agent_alert_message(agent_id, false, message, now)
        }
        HomeNotice::Alert { message, key } => {
            let info: Vec<sl_proto::AlertInfo> = key
                .map(|key| sl_proto::AlertInfo {
                    message: key.to_owned(),
                    extra_params: String::new(),
                })
                .into_iter()
                .collect();
            sim.send_alert_message(message, &info, &[], now)
        }
    };
    if let Err(error) = sent {
        tracing::warn!("answering a set-home request failed: {error}");
    }
}

/// Answers one drained [`ServerEvent`] about the agent itself. Anything this
/// module has no answer for is left alone.
pub(crate) fn answer_agent_request(
    policy: AgentPolicy,
    homes: HomeStore<'_>,
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
            let worn = policy.wearables_reply.stated(worn);
            if let Err(error) = sim.send_agent_wearables_update(serial, &worn, now) {
                tracing::warn!("answering an agent wearables request failed: {error}");
            }
        }
        // A legacy name lookup (`UUIDNameRequest`), answered from the same
        // people store `GetDisplayNames` serves: every account of the grid and
        // every avatar the scene names. An id nobody knows goes unanswered, as
        // a simulator leaves it.
        ServerEvent::AvatarNamesRequested(ids) => {
            let names: Vec<sl_proto::AvatarName> = ids
                .iter()
                .filter_map(|id| sim.display_name(AgentKey::from(*id)))
                .map(|record| sl_proto::AvatarName {
                    id: record.id,
                    first_name: record.legacy_first_name.clone(),
                    last_name: record.legacy_last_name.clone(),
                })
                .collect();
            if !names.is_empty()
                && let Err(error) = sim.send_avatar_names(&names, now)
            {
                tracing::warn!("answering a name request failed: {error}");
            }
        }
        // Set-Home over UDP: the grid's rule, and what it says of each
        // outcome. OpenSim answers every one, which makes it the one
        // deterministic way to provoke an `AgentAlertMessage` there.
        ServerEvent::SetStartLocation {
            position, look_at, ..
        } => {
            let outcome = set_home(
                policy.estate_manager,
                homes,
                world,
                agent_id,
                *position,
                look_at,
            );
            // A repeat over UDP was not measured: every one is told.
            tell(
                sim,
                agent_id,
                outcome.notice(&homes.policy.udp),
                (None, &mut None),
                now,
            );
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

#[cfg(test)]
mod tests {
    use super::{PLACEHOLDER_ASSET_IDS, WearablesReply};
    use pretty_assertions::assert_eq;
    use sl_proto::{InventoryKey, Wearable, WearableType};
    use uuid::Uuid;

    /// The worn shape, as a simulator holds it.
    fn held() -> Vec<Wearable> {
        vec![Wearable {
            item_id: InventoryKey::from(Uuid::from_u128(1)),
            asset_id: Some(Uuid::from_u128(2)),
            wearable_type: WearableType::Shape,
        }]
    }

    /// OpenSim states the outfit it holds; Second Life its four placeholders
    /// whatever is worn, which the client recognises as no outfit at all.
    #[test]
    fn each_flavour_states_its_own_wearables() {
        assert_eq!(WearablesReply::Outfit.stated(&held()), held());
        let placeholders = WearablesReply::Placeholders.stated(&held());
        assert!(Wearable::is_placeholder_outfit(&placeholders));
        assert_eq!(
            placeholders
                .iter()
                .map(|worn| (worn.wearable_type, worn.asset_id))
                .collect::<Vec<_>>(),
            vec![
                (WearableType::Skin, PLACEHOLDER_ASSET_IDS.first().copied()),
                (WearableType::Hair, PLACEHOLDER_ASSET_IDS.get(1).copied()),
                (WearableType::Eyes, PLACEHOLDER_ASSET_IDS.get(2).copied()),
                (WearableType::Shirt, PLACEHOLDER_ASSET_IDS.get(3).copied()),
            ]
        );
    }
}
