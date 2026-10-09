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
//! The fake grid has no physics, so "within view" is what a fixture can state
//! — which regions touch, as [`NeighbourPolicy`] decides — narrowed by the one
//! thing the client does state: its draw distance, in every `AgentUpdate`.
//! Each live grid reads that distance its own way
//! ([`crate::ImitatedGrid::neighbour_policy`]), announcing a neighbour it
//! comes to reach and retiring one it stops reaching, and the fake grid does
//! as the flavour it imitates.

use std::collections::BTreeMap;
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

/// The draw distance an agent is taken to have until an `AgentUpdate` of its
/// own says otherwise: the reference viewer's default.
const DEFAULT_FAR_M: f32 = 256.0;

/// How far an `AgentUpdate`'s draw distance has to move before the neighbours
/// are looked at again. A viewer repeats the same figure every update; this
/// only keeps a float's noise from reading as a change.
const FAR_CHANGE_M: f32 = 0.5;

/// Announces every neighbour of `shared`'s region that a draw distance of
/// `far` metres reaches to its client, and opens a child session in each: the
/// `EnableSimulator` + `EstablishAgentCommunication` pair a simulator sends a
/// freshly rooted agent — and sends again for a neighbour a wider draw
/// distance brings back into view.
///
/// A region the agent already has a session in is skipped — the region it just
/// walked out of is a neighbour of the one it walked into, and its circuit is
/// still open (now as a child), so announcing it again would hand the client a
/// second simulator for the same region handle.
///
/// Failures are logged rather than propagated: an announcement that does not
/// happen costs the client a neighbour, not its session.
pub(crate) async fn announce_neighbours(core: &Arc<GridCore>, shared: &SharedSim, far: f32) {
    let (seq, region_index, ids, agent_id, avatar) = {
        let state = shared.state.lock().await;
        if !state.sim.is_root_agent() {
            return;
        }
        let at = crate::chat::agent_position(&state.sim);
        (
            state.seq,
            state.region,
            state.ids,
            state.avatar.agent_id,
            (at.x, at.y),
        )
    };
    let Some(account) = core.account_by_agent(agent_id).cloned() else {
        return;
    };
    let region_size = core.neighbour_policy.states_region_size.then_some((
        sl_proto::STANDARD_REGION_SIZE_METRES,
        sl_proto::STANDARD_REGION_SIZE_METRES,
    ));
    for neighbour in core.neighbours_in_view(region_index, far, avatar) {
        // A root that ended meanwhile — logged out, or kicked by a second
        // login — has no client left to announce a neighbour to, and a child
        // session opened for it now would outlive the login it belonged to.
        if shared.is_closed() {
            return;
        }
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
                session.enqueue_enable_simulator(handle, sim, region_size);
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

/// Looks at the neighbours of `shared`'s region again after its agent's draw
/// distance changed to `far`: announces the ones it now reaches, and notes in
/// `retiring` when each one it no longer reaches is due to go — at once on
/// OpenSim, fifty seconds on on Second Life
/// ([`crate::imitates::NeighbourViewPolicy::retire_delay`]). A neighbour that
/// is reached again before it went is kept.
async fn review_neighbours(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    far: f32,
    retiring: &mut BTreeMap<usize, tokio::time::Instant>,
) {
    let (region_index, agent_id, avatar) = {
        let state = shared.state.lock().await;
        if !state.sim.is_root_agent() {
            retiring.clear();
            return;
        }
        let at = crate::chat::agent_position(&state.sim);
        (state.region, state.avatar.agent_id, (at.x, at.y))
    };
    announce_neighbours(core, shared, far).await;
    let reached = core.neighbours_in_view(region_index, far, avatar);
    let now = tokio::time::Instant::now();
    for neighbour in core.neighbours_of(region_index) {
        if reached.contains(&neighbour) {
            retiring.remove(&neighbour);
        } else if core.session_of(agent_id, neighbour).await.is_some() {
            // A delay too long to add to the clock never comes due.
            if let Some(due) = now.checked_add(core.neighbour_policy.retire_delay) {
                retiring.entry(neighbour).or_insert(due);
            }
        }
    }
}

/// Retires the child session in each region of `retiring` whose time has
/// come: the `DisableSimulator` a simulator sends down a child circuit once
/// the agent's draw distance has stopped reaching the region.
async fn retire_out_of_view(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    retiring: &mut BTreeMap<usize, tokio::time::Instant>,
) {
    let (agent_id, is_root) = {
        let state = shared.state.lock().await;
        (state.avatar.agent_id, state.sim.is_root_agent())
    };
    if !is_root {
        // The agent walked or teleported away; what it holds is the new
        // root's to decide.
        retiring.clear();
        return;
    }
    let now = tokio::time::Instant::now();
    let due: Vec<usize> = retiring
        .iter()
        .filter(|(_region, at)| **at <= now)
        .map(|(region, _at)| *region)
        .collect();
    for region in due {
        retiring.remove(&region);
        let Some(child) = core.session_of(agent_id, region).await else {
            continue;
        };
        let (seq, child_is_root) = {
            let state = child.state.lock().await;
            (state.seq, state.sim.is_root_agent())
        };
        if child_is_root {
            continue;
        }
        if let Err(error) = child
            .with_sim(|session| session.retire_circuit(child.now()))
            .await
        {
            tracing::warn!("retiring the out-of-view child session {seq} failed: {error}");
        }
        core.remove_session(seq).await;
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
        // The draw distance the agent last stated, and the neighbours it no
        // longer reaches with when each is due to be retired.
        let mut far = DEFAULT_FAR_M;
        let mut retiring: BTreeMap<usize, tokio::time::Instant> = BTreeMap::new();
        loop {
            if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
                break;
            }
            let next_retirement = retiring.values().min().copied();
            let received = tokio::select! {
                received = events.recv() => received,
                () = async {
                    match next_retirement {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    retire_out_of_view(&core, &shared, &mut retiring).await;
                    continue;
                }
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
                    // A new root region: what was due to go was due from the
                    // region the agent left.
                    retiring.clear();
                    announce_neighbours(&core, &shared, far).await;
                }
                Ok(sl_proto::ServerEvent::AgentUpdate(update))
                    if (update.far - far).abs() > FAR_CHANGE_M =>
                {
                    far = update.far;
                    review_neighbours(&core, &shared, far, &mut retiring).await;
                }
                Ok(sl_proto::ServerEvent::LoggedOut) => {
                    retire_children_on_logout(&core, &shared).await;
                    return;
                }
                Ok(sl_proto::ServerEvent::Disconnected | sl_proto::ServerEvent::CircuitRetired)
                | Err(broadcast::error::RecvError::Closed) => break,
                Ok(_other) => {}
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!("the neighbour announcer missed {missed} events");
                }
            }
        }
        // A logout closes the session in the same flush that reports it, so
        // the close can win the `select!` above with the `LoggedOut` still
        // queued behind it: read what is left before deciding it was not one.
        loop {
            match events.try_recv() {
                Ok(sl_proto::ServerEvent::LoggedOut) => {
                    retire_children_on_logout(&core, &shared).await;
                    return;
                }
                Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(
                    broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed,
                ) => return,
            }
        }
    })
}

/// Retires every child session of the agent whose session `shared` just logged
/// out — OpenSim's `CloseChildAgents` on logout. A viewer sends its
/// `LogoutRequest` to the root region alone, so it is the grid that closes the
/// child agents watching from next door; otherwise they outlive the login.
///
/// Only a root's logout does this: a child session never receives one. And
/// only that login's children are retired, not the sessions of a login of the
/// same avatar that followed it.
async fn retire_children_on_logout(core: &Arc<GridCore>, shared: &SharedSim) {
    let (seq, agent_id, login) = {
        let state = shared.state.lock().await;
        (state.seq, state.avatar.agent_id, state.ids.session_id)
    };
    for child in core.sessions_of(agent_id).await {
        let (child_seq, is_root, child_login) = {
            let state = child.state.lock().await;
            (state.seq, state.sim.is_root_agent(), state.ids.session_id)
        };
        // Only the children of the login that just ended. The avatar may be
        // logging in again already, and the session that login was given is
        // no root agent until its movement completes: retired here, it
        // answered the new viewer's seed request with a 404.
        if child_seq == seq || is_root || child_login != login {
            continue;
        }
        if let Err(error) = child
            .with_sim(|session| session.retire_circuit(child.now()))
            .await
        {
            tracing::warn!("retiring child session {child_seq} after a logout failed: {error}");
        }
        core.remove_session(child_seq).await;
    }
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
