//! Neighbouring regions: which regions border which, and the child agent a
//! root arrival opens in each of them.
//!
//! A simulator does not wait for an avatar to reach a border. The moment the
//! agent is rooted it tells the client about every region within view
//! (`EnableSimulator` + `EstablishAgentCommunication` over the event queue),
//! the client opens a **child** circuit to each, and those regions start
//! streaming their scene. That is why a neighbour's ground and objects are
//! already drawn before you walk into it — and why a border crossing is a
//! *promotion* of a circuit that is already open rather than a connection made
//! on the spot ([`crate::crossing`]).
//!
//! The fake grid has no view distance and no physics, so "within view" is
//! reduced to the one thing a fixture can state: which regions touch, as
//! [`NeighbourPolicy`] decides.

use std::sync::Arc;

use sl_types::key::AgentKey;
use tokio::sync::broadcast;

use crate::driver::SharedSim;
use crate::error::Error;
use crate::runtime::{GridCore, SessionRole};

/// Which regions a region announces to an arriving agent.
///
/// The default is [`Adjacent`](Self::Adjacent), because that is what a real
/// grid does with a default view distance and it is what makes a border
/// crossing possible at all. The other two exist for tests: [`None`](Self::None)
/// so a fixture can prove the announcement is what opens a child circuit, and
/// [`Named`](Self::Named) so a scene can wire up a topology the grid
/// coordinates do not describe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NeighbourPolicy {
    /// Every other region whose grid coordinates are within one slot on both
    /// axes — the eight regions surrounding this one, of which the grid serves
    /// however many it was built with.
    #[default]
    Adjacent,
    /// No neighbours: nothing is announced, and every crossing out of this
    /// region is refused.
    None,
    /// Exactly the regions named here, adjacent or not. A name the grid does
    /// not serve is ignored.
    Named(Vec<String>),
}

/// How far apart, in region slots, two regions may be and still touch.
const ADJACENT_SLOTS: u32 = 1;

/// Whether the grid coordinates `(ax, ay)` and `(bx, by)` are within one
/// region slot of each other on both axes — the eight-way adjacency an
/// avatar can walk across. A region is not its own neighbour.
#[must_use]
pub(crate) fn touches(a: (u32, u32), b: (u32, u32)) -> bool {
    if a == b {
        return false;
    }
    a.0.abs_diff(b.0) <= ADJACENT_SLOTS && a.1.abs_diff(b.1) <= ADJACENT_SLOTS
}

/// Announces every neighbour of `shared`'s region to its client and opens a
/// child session in each: the `EnableSimulator` + `EstablishAgentCommunication`
/// pair a simulator sends a freshly rooted agent.
///
/// A region the agent already has a session in is skipped — the region it just
/// walked out of is a neighbour of the one it walked into, and its circuit is
/// still open (now as a child), so announcing it again would hand the client a
/// second simulator for the same region handle.
///
/// Failures are logged rather than propagated: an announcement that does not
/// happen costs the client a neighbour, not its session.
pub(crate) async fn announce_neighbours(core: &Arc<GridCore>, shared: &SharedSim) {
    let (seq, region_index, ids, agent_id) = {
        let state = shared.state.lock().await;
        if !state.sim.is_root_agent() {
            return;
        }
        (state.seq, state.region, state.ids, state.avatar.agent_id)
    };
    let Some(account) = core.account_by_agent(agent_id).cloned() else {
        return;
    };
    for neighbour in core.neighbours_of(region_index) {
        if core.session_of(agent_id, neighbour).await.is_some() {
            continue;
        }
        let prepared = match core
            .prepare_region_session(&account, neighbour, ids, None, SessionRole::Child)
            .await
        {
            Ok(prepared) => prepared,
            Err(error) => {
                tracing::warn!("preparing a child session for a neighbour failed: {error}");
                continue;
            }
        };
        core.activate_session(&prepared).await;
        let sim = prepared.udp_addr;
        let seed = prepared.seed_url.to_string();
        let Some(handle) = core
            .region(neighbour)
            .map(crate::runtime::RegionEntry::handle)
        else {
            continue;
        };
        shared
            .with_sim(|session| {
                session.enqueue_enable_simulator(handle, sim);
                session.enqueue_establish_agent_communication(sim, &seed);
            })
            .await;
        tracing::info!(
            "announced neighbour {:?} to session {seq} as child session {}",
            prepared.region_name,
            prepared.seq
        );
    }
}

/// Retires every child session of `agent_id` whose region is neither `region`
/// nor one of its neighbours — the `DisableSimulator` a simulator sends after
/// a crossing for the regions that have dropped out of view
/// (`ScenePresence.CloseChildAgents`).
///
/// Without it an agent that walks a long way accumulates one open circuit per
/// region it ever bordered, and the client keeps polling all of them.
pub(crate) async fn retire_distant_children(
    core: &Arc<GridCore>,
    agent_id: AgentKey,
    region: usize,
) {
    let mut keep = core.neighbours_of(region);
    keep.push(region);
    for shared in core.sessions_of(agent_id).await {
        let (seq, session_region, is_root) = {
            let state = shared.state.lock().await;
            (state.seq, state.region, state.sim.is_root_agent())
        };
        if is_root || keep.contains(&session_region) {
            continue;
        }
        if let Err(error) = shared
            .with_sim(|session| session.retire_circuit(shared.now()))
            .await
        {
            tracing::warn!("retiring the distant child session {seq} failed: {error}");
        }
        core.remove_session(seq).await;
    }
}

/// The per-session task that announces the region's neighbours the moment the
/// agent is rooted in it — a login's arrival, a teleport's, or a crossing's.
///
/// A task rather than part of the driver's flush rule because announcing binds
/// a socket and mints a capability surface for each neighbour, which is async
/// work, and the flush rule runs under the session lock.
///
/// Boxed for the same reason [`crate::teleport::run_teleport_responder`] is:
/// activating a session spawns this, and this activates the neighbours'
/// sessions.
pub(crate) fn run_neighbour_announcer(
    core: Arc<GridCore>,
    shared: SharedSim,
) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let mut events = shared.subscribe_events();
        let mut closed_rx = shared.closed_tx.subscribe();
        let mut shutdown_rx = shared.shutdown_rx.clone();
        loop {
            if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
                break;
            }
            let received = tokio::select! {
                received = events.recv() => received,
                changed = closed_rx.changed() => {
                    if changed.is_err() || *closed_rx.borrow() {
                        break;
                    }
                    continue;
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                    continue;
                }
            };
            match received {
                Ok(sl_proto::ServerEvent::AgentArrived) => {
                    announce_neighbours(&core, &shared).await;
                }
                Ok(
                    sl_proto::ServerEvent::Disconnected
                    | sl_proto::ServerEvent::LoggedOut
                    | sl_proto::ServerEvent::CircuitRetired,
                )
                | Err(broadcast::error::RecvError::Closed) => break,
                Ok(_other) => {}
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!("the neighbour announcer missed {missed} events");
                }
            }
        }
    })
}

/// The index of the region called `name`, or [`Error::UnknownRegion`].
pub(crate) fn region_index(core: &GridCore, name: &str) -> Result<usize, Error> {
    core.region_by_name(name)
        .ok_or_else(|| Error::UnknownRegion {
            region: name.to_owned(),
        })
}

#[cfg(test)]
mod test {
    use super::*;

    /// Eight-way adjacency, and never to itself.
    #[test]
    fn touching_is_eight_way_and_never_reflexive() {
        assert!(!touches((1000, 1000), (1000, 1000)));
        for offset in [
            (1, 0),
            (0, 1),
            (1, 1),
            (u32::MAX, 0),
            (0, u32::MAX),
            (u32::MAX, u32::MAX),
        ] {
            let other = (
                1000_u32.wrapping_add(offset.0),
                1000_u32.wrapping_add(offset.1),
            );
            assert!(
                touches((1000, 1000), other),
                "{other:?} should touch (1000, 1000)"
            );
        }
        assert!(!touches((1000, 1000), (1002, 1000)));
        assert!(!touches((1000, 1000), (1001, 1002)));
    }
}
