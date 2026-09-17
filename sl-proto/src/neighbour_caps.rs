//! Routing **object-addressed capability requests** to the region that knows
//! the object.
//!
//! A capability is a URL on one simulator, and a simulator only answers for the
//! objects in its own region. `GetObjectCost` (an object's land impact),
//! `GetObjectPhysicsData` and `ResourceCostSelected` name objects, so a batch
//! that includes an object across a region border has to be split: the part
//! the agent's own region holds goes to the root capability map, and each
//! neighbour's part to that neighbour's map — the one its seed capability
//! (`EstablishAgentCommunication`, surfaced as
//! [`Event::NeighborSeed`](crate::Event::NeighborSeed)) answers with. The
//! reference does the same (`llviewerobjectlist.cpp` `fetchObjectCosts`, which
//! groups the stale objects by `getRegion()`).
//!
//! [`NeighbourCaps`] is the sans-I/O half both runtimes share: it holds each
//! neighbour's capability map, decides where each part of a request goes
//! ([`NeighbourCaps::route`]), and parks a request whose neighbour map is still
//! being fetched. The runtimes own the HTTP.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_types::key::ObjectKey;
use sl_wire::{
    Llsd, SelectedResourceCost, WireError, parse_resource_cost_selected,
    resource_cost_selected_llsd,
};

use crate::command::Command;
use crate::session::{
    CAP_GET_OBJECT_COST, CAP_GET_OBJECT_PHYSICS_DATA, CAP_RESOURCE_COST_SELECTED, Session,
};

/// How long a request waits for a neighbour's capability map before the
/// neighbour's part is given up as [`UnroutableReason::TimedOut`]. Long enough
/// for a seed fetch that is retrying a transient failure; short enough that a
/// neighbour whose seed never arrives cannot hold a request for the rest of the
/// session.
pub const NEIGHBOUR_CAPS_WAIT: Duration = Duration::from_secs(60);

/// Which region's simulator answers for an object — see
/// [`Session::object_region`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRegion {
    /// The region the agent is in (also the answer for an object the session
    /// has not streamed).
    Root,
    /// The neighbouring region whose simulator is at this UDP address (its
    /// child circuit's key).
    Neighbour(SocketAddr),
}

/// The state of one neighbour's capability map.
#[derive(Debug, Clone)]
enum NeighbourMap {
    /// The seed has been POSTed; the map has not come back yet.
    Fetching,
    /// The neighbour served its map (capability name → URL).
    Fetched(HashMap<String, String>),
    /// The seed fetch failed, with a readable reason.
    Failed(String),
}

/// Why a neighbour's part of a request could not be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnroutableReason {
    /// The neighbour's capability map could not be fetched.
    FetchFailed(String),
    /// The neighbour served its map, but without this capability.
    CapNotAdvertised,
    /// The neighbour's map did not arrive within [`NEIGHBOUR_CAPS_WAIT`].
    TimedOut,
}

impl core::fmt::Display for UnroutableReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FetchFailed(reason) => {
                write!(
                    f,
                    "the neighbour's capabilities could not be fetched: {reason}"
                )
            }
            Self::CapNotAdvertised => {
                write!(f, "the neighbour does not advertise the capability")
            }
            Self::TimedOut => write!(
                f,
                "the neighbour's capabilities did not arrive within {} s",
                NEIGHBOUR_CAPS_WAIT.as_secs()
            ),
        }
    }
}

/// One capability POST a routed request resolves to: a URL and the objects its
/// body names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapRequest {
    /// The capability URL on the simulator that knows these objects.
    pub url: String,
    /// The objects this POST asks about.
    pub objects: Vec<ObjectKey>,
}

/// A neighbour's part of a request that cannot be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unroutable {
    /// The neighbouring simulator the objects are in.
    pub sim: SocketAddr,
    /// The objects that go unasked.
    pub objects: Vec<ObjectKey>,
    /// Why.
    pub reason: UnroutableReason,
}

/// Where the parts of an object-addressed capability request go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapRoute {
    /// A neighbour's capability map is still being fetched: hold the whole
    /// request ([`NeighbourCaps::park`]) and route it again later. Nothing is
    /// sent, so a summed reply is never computed from part of the selection.
    Wait,
    /// Every part is decided.
    Ready {
        /// The POSTs to make, one per region that has the capability. The
        /// agent's own region is asked about its objects only when its map
        /// advertises the capability, exactly as before neighbours were
        /// routed: a region without it (plain OpenSim) is not an error.
        requests: Vec<CapRequest>,
        /// Neighbour parts that cannot be sent, to be reported.
        unroutable: Vec<Unroutable>,
    },
}

/// A capability request naming objects, as the runtimes dispatch it — the
/// commands whose objects [`NeighbourCaps::route`] splits by region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectCapsCommand<'command> {
    /// The capability the command POSTs to.
    pub capability: &'static str,
    /// The objects it names.
    pub objects: &'command [ObjectKey],
}

impl<'command> ObjectCapsCommand<'command> {
    /// The object-addressed capability request `command` makes, or `None` for
    /// any other command.
    #[must_use]
    pub const fn of(command: &'command Command) -> Option<Self> {
        let (capability, objects) = match command {
            Command::RequestObjectCost { object_ids } => (CAP_GET_OBJECT_COST, object_ids),
            Command::RequestSelectedCost { object_ids, .. } => {
                (CAP_RESOURCE_COST_SELECTED, object_ids)
            }
            Command::RequestObjectPhysicsData { object_ids } => {
                (CAP_GET_OBJECT_PHYSICS_DATA, object_ids)
            }
            _ => return None,
        };
        Some(Self {
            capability,
            objects: objects.as_slice(),
        })
    }
}

/// The neighbouring regions' capability maps, and the object-addressed
/// requests waiting for one. One per session, owned by the runtime.
#[derive(Debug, Default)]
pub struct NeighbourCaps {
    /// Each neighbour's map, by its simulator address.
    maps: BTreeMap<SocketAddr, NeighbourMap>,
    /// Requests waiting for a neighbour's map, with when each was first routed.
    parked: Vec<(Command, Instant)>,
}

impl NeighbourCaps {
    /// Records that `sim`'s seed capability has been POSTed and its map is on
    /// its way. A neighbour re-announced (after a crossing, say) starts over.
    pub fn fetch_started(&mut self, sim: SocketAddr) {
        self.maps.insert(sim, NeighbourMap::Fetching);
    }

    /// Records the outcome of `sim`'s seed fetch: its map, or why it failed.
    pub fn fetched(&mut self, sim: SocketAddr, outcome: Result<HashMap<String, String>, String>) {
        let map = match outcome {
            Ok(map) => NeighbourMap::Fetched(map),
            Err(reason) => NeighbourMap::Failed(reason),
        };
        self.maps.insert(sim, map);
    }

    /// Forgets the maps of neighbours whose child circuit `session` no longer
    /// holds — including one just promoted to the root by a crossing, whose
    /// capabilities the runtime now fetches as the root region's.
    pub fn prune(&mut self, session: &Session) {
        self.maps.retain(|sim, _| session.has_neighbour(*sim));
    }

    /// Where the parts of `request` go. `root_caps` is the agent's own
    /// region's map; `parked_since` is when the request was first routed (now,
    /// for a fresh one), which bounds how long it may wait for a neighbour.
    #[must_use]
    pub fn route(
        &self,
        session: &Session,
        root_caps: &HashMap<String, String>,
        request: ObjectCapsCommand<'_>,
        parked_since: Instant,
        now: Instant,
    ) -> CapRoute {
        let mut groups: Vec<(ObjectRegion, Vec<ObjectKey>)> = Vec::new();
        for object in request.objects {
            let region = session.object_region(*object);
            match groups.iter_mut().find(|(known, _)| *known == region) {
                Some((_, objects)) => objects.push(*object),
                None => groups.push((region, vec![*object])),
            }
        }
        let expired = now.saturating_duration_since(parked_since) >= NEIGHBOUR_CAPS_WAIT;
        let mut requests = Vec::new();
        let mut unroutable = Vec::new();
        for (region, objects) in groups {
            let sim = match region {
                ObjectRegion::Root => {
                    if let Some(url) = root_caps.get(request.capability) {
                        requests.push(CapRequest {
                            url: url.clone(),
                            objects,
                        });
                    }
                    continue;
                }
                ObjectRegion::Neighbour(sim) => sim,
            };
            match self.maps.get(&sim) {
                Some(NeighbourMap::Fetched(map)) => match map.get(request.capability) {
                    Some(url) => requests.push(CapRequest {
                        url: url.clone(),
                        objects,
                    }),
                    None => unroutable.push(Unroutable {
                        sim,
                        objects,
                        reason: UnroutableReason::CapNotAdvertised,
                    }),
                },
                Some(NeighbourMap::Failed(reason)) => unroutable.push(Unroutable {
                    sim,
                    objects,
                    reason: UnroutableReason::FetchFailed(reason.clone()),
                }),
                Some(NeighbourMap::Fetching) | None if expired => unroutable.push(Unroutable {
                    sim,
                    objects,
                    reason: UnroutableReason::TimedOut,
                }),
                Some(NeighbourMap::Fetching) | None => return CapRoute::Wait,
            }
        }
        CapRoute::Ready {
            requests,
            unroutable,
        }
    }

    /// Holds `command` (first routed at `since`) until a neighbour's map
    /// arrives.
    pub fn park(&mut self, command: Command, since: Instant) {
        self.parked.push((command, since));
    }

    /// Takes every parked request, each with when it was first routed, for the
    /// runtime to route again (and park again if it still has to wait).
    pub fn take_parked(&mut self) -> Vec<(Command, Instant)> {
        core::mem::take(&mut self.parked)
    }

    /// Whether any request is parked.
    #[must_use]
    pub const fn has_parked(&self) -> bool {
        !self.parked.is_empty()
    }
}

/// Combines the `ResourceCostSelected` replies of the regions a selection
/// spans into the one reply the whole selection would have had: each summed
/// cost is added across the regions.
///
/// # Errors
///
/// Returns the decode error of the first reply that is not a
/// `ResourceCostSelected` body.
pub fn merge_selected_cost_replies(replies: &[Llsd]) -> Result<Llsd, WireError> {
    let mut total = SelectedResourceCost::default();
    for reply in replies {
        let cost = parse_resource_cost_selected(reply)?;
        total.physics += cost.physics;
        total.streaming += cost.streaming;
        total.simulation += cost.simulation;
    }
    Ok(resource_cost_selected_llsd(&total))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Instant;

    use pretty_assertions::assert_eq;
    use sl_types::key::ObjectKey;
    use sl_wire::{
        SelectedResourceCost, parse_resource_cost_selected, resource_cost_selected_llsd,
    };
    use uuid::Uuid;

    use super::{
        CapRequest, CapRoute, NeighbourCaps, ObjectCapsCommand, merge_selected_cost_replies,
    };
    use crate::command::Command;
    use crate::session::{CAP_GET_OBJECT_COST, CAP_RESOURCE_COST_SELECTED};
    use crate::{LoginParams, LoginRequest, Session, StartLocation};

    /// A boxed error so tests can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// A session with no circuit: every object is answered by the root region.
    fn session() -> Result<Session, TestError> {
        Ok(Session::new(LoginParams {
            login_uri: url::Url::parse("http://127.0.0.1:9000/")?,
            request: LoginRequest::new(
                "Test",
                "Resident",
                "secret",
                StartLocation::Last,
                "sl-proto-test",
                "0",
            ),
        }))
    }

    /// The costs of a selection that spans regions are the sum of each
    /// region's reply.
    #[test]
    fn selected_cost_replies_are_summed() -> Result<(), TestError> {
        let merged = merge_selected_cost_replies(&[
            resource_cost_selected_llsd(&SelectedResourceCost {
                physics: 1.0,
                streaming: 2.0,
                simulation: 0.5,
            }),
            resource_cost_selected_llsd(&SelectedResourceCost {
                physics: 3.0,
                streaming: 0.25,
                simulation: 1.5,
            }),
        ])?;
        assert_eq!(
            parse_resource_cost_selected(&merged)?,
            SelectedResourceCost {
                physics: 4.0,
                streaming: 2.25,
                simulation: 2.0,
            }
        );
        Ok(())
    }

    /// Only the three object-addressed capability commands are routed.
    #[test]
    fn only_object_capability_commands_are_classified() {
        let ids = vec![ObjectKey::from(Uuid::from_u128(1))];
        let selected = Command::RequestSelectedCost {
            object_ids: ids.clone(),
            roots: true,
        };
        assert_eq!(
            ObjectCapsCommand::of(&selected),
            Some(ObjectCapsCommand {
                capability: CAP_RESOURCE_COST_SELECTED,
                objects: &ids,
            })
        );
        assert_eq!(ObjectCapsCommand::of(&Command::Stand), None);
    }

    /// Objects of the agent's own region go to the root map when it has the
    /// capability, and are simply not asked about when it does not.
    #[test]
    fn root_objects_use_the_root_map_when_it_has_the_capability() -> Result<(), TestError> {
        let session = session()?;
        let caps = NeighbourCaps::default();
        let ids = vec![
            ObjectKey::from(Uuid::from_u128(1)),
            ObjectKey::from(Uuid::from_u128(2)),
        ];
        let command = Command::RequestObjectCost {
            object_ids: ids.clone(),
        };
        let request = ObjectCapsCommand::of(&command).ok_or("not an object capability")?;
        let now = Instant::now();
        let root = HashMap::from([(
            CAP_GET_OBJECT_COST.to_owned(),
            "https://root.example/cost".to_owned(),
        )]);
        assert_eq!(
            caps.route(&session, &root, request, now, now),
            CapRoute::Ready {
                requests: vec![CapRequest {
                    url: "https://root.example/cost".to_owned(),
                    objects: ids,
                }],
                unroutable: Vec::new(),
            }
        );
        assert_eq!(
            caps.route(&session, &HashMap::new(), request, now, now),
            CapRoute::Ready {
                requests: Vec::new(),
                unroutable: Vec::new(),
            }
        );
        Ok(())
    }
}
