//! Border crossings: handing an agent from the region it is standing in to the
//! one next door, without a teleport screen.
//!
//! # Why this is scripted
//!
//! A real crossing is a consequence of *movement*: the simulator watches the
//! avatar's position, sees it leave the region rectangle, and hands it over.
//! The fake grid runs no physics and claims no movement authority — it takes the
//! client's word for where the avatar is and never argues — so nothing here can
//! notice a border being reached. A crossing is therefore something a test (or a
//! scripted timeline) *asks for*: [`crate::FakeGrid::cross_agent`].
//!
//! # The wire sequence
//!
//! It mirrors OpenSim's `EntityTransferModule.CrossAgentIntoNewRegionMain`, and
//! differs from a teleport (`teleport.rs`) in three ways that all matter to a
//! viewer:
//!
//! - **No teleport screen.** No `TeleportStart`, no progress keys, no
//!   `TeleportFinish` — one `CrossedRegion` event and the client promotes a
//!   circuit it already holds. The scene is *kept* and re-based onto the new
//!   origin rather than torn down and rebuilt.
//! - **The destination circuit is already open.** It was announced as a
//!   neighbour when the agent arrived where it is now
//!   ([`crate::neighbours`]), which is why the region across the border is
//!   already drawn. The crossing reuses that child session; only a crossing
//!   into a region the announcement missed opens one on the spot.
//! - **The source is not retired.** It becomes a *child* agent
//!   (`SimSession::make_child_agent`) and keeps streaming, because the region
//!   you just walked out of is still in front of you. A teleport's source is
//!   retired outright with `DisableSimulator`; a crossing retires only the
//!   children that have dropped out of view
//!   (`neighbours::retire_distant_children`).
//!
//! The departing avatar's object is deliberately **not** killed on the source
//! circuit — see `SimSession::make_child_agent` for why.

use std::sync::Arc;
use std::time::Duration;

use sl_proto::{CrossedRegionInfo, ServerEvent};
use sl_types::lsl::Vector;
use sl_types::map::RegionCoordinates;

use crate::driver::SharedSim;
use crate::error::Error;
use crate::neighbours;
use crate::runtime::{CrossingNotice, GridCore, SessionRole};

/// How long the grid waits for the client to complete its movement into the
/// region across the border before it gives the crossing up.
///
/// Shorter than the teleport budget: a crossing promotes a circuit the client
/// already holds, so the only thing being waited for is one `CrossedRegion`
/// making the round trip and one `CompleteAgentMovement` coming back.
pub const CROSSING_ARRIVAL_TIMEOUT: Duration = Duration::from_secs(15);

/// Walks the agent of `source` over the border into the region at
/// `region_index` — see the module docs for the sequence. Returns the
/// destination session.
///
/// # Errors
///
/// [`Error::NotRootAgent`] when the source's agent is not actually here,
/// [`Error::UnknownRegion`] for a bad region index, [`Error::NotAdjacent`] when
/// the destination does not border the source's region,
/// [`Error::UnknownAccount`] for a session no account owns,
/// [`Error::CrossingTimedOut`] when the client never arrived, and socket errors
/// binding a destination that has to be opened on the spot.
pub(crate) async fn cross_session(
    core: &Arc<GridCore>,
    source: &SharedSim,
    region_index: usize,
    position: RegionCoordinates,
    velocity: Vector,
) -> Result<SharedSim, Error> {
    let (source_seq, source_region, ids, agent_id, look_at) = {
        let state = source.state.lock().await;
        if !state.sim.is_root_agent() {
            return Err(Error::NotRootAgent);
        }
        (
            state.seq,
            state.region,
            state.ids,
            state.avatar.agent_id,
            state.sim.arrival_position().look_at.clone(),
        )
    };
    let source_name = core
        .region(source_region)
        .map_or_else(String::new, |entry| entry.config.name.clone());
    let dest = core.region(region_index).ok_or(Error::UnknownRegion {
        region: region_index.to_string(),
    })?;
    let dest_handle = dest.handle();
    let dest_name = dest.config.name.clone();
    if !core.neighbours_of(source_region).contains(&region_index) {
        return Err(Error::NotAdjacent {
            from: source_name,
            to: dest_name,
        });
    }
    let account = core
        .account_by_agent(agent_id)
        .cloned()
        .ok_or(Error::UnknownAccount)?;

    // The child circuit the neighbour announcement already opened, or — if the
    // policy or a race left none — one announced now. Either way the client
    // holds a circuit to `dest` by the time it reads the `CrossedRegion` that
    // follows in the same event-queue batch.
    let (destination, dest_seq, dest_addr, dest_seed, announced) =
        match core.session_of(agent_id, region_index).await {
            Some(shared) => {
                let (seq, addr, seed) = {
                    let state = shared.state.lock().await;
                    (state.seq, state.udp_addr, state.seed_url.to_string())
                };
                (shared, seq, addr, seed, false)
            }
            None => {
                let prepared = core
                    .prepare_region_session(&account, region_index, ids, None, SessionRole::Child)
                    .await?;
                core.activate_session(&prepared).await;
                let seed = prepared.seed_url.to_string();
                let addr = prepared.udp_addr;
                source
                    .with_sim(|sim| {
                        sim.enqueue_enable_simulator(dest_handle, addr);
                        sim.enqueue_establish_agent_communication(addr, &seed);
                    })
                    .await;
                (prepared.shared.clone(), prepared.seq, addr, seed, true)
            }
        };

    // Where the avatar lands, which is what the destination's
    // `AgentMovementComplete` will report back. The facing is carried over from
    // the region left behind: an avatar walking over a border does not turn.
    destination
        .with_sim(|sim| sim.set_arrival_position(position, look_at))
        .await;
    // Subscribe before announcing, or the arrival can slip past.
    let mut dest_events = destination.subscribe_events();

    source
        .with_sim(|sim| {
            sim.enqueue_crossed_region(&CrossedRegionInfo {
                agent_id,
                session_id: ids.session_id,
                region_handle: dest_handle,
                dest: dest_addr,
                seed: dest_seed,
                position,
                look_at: velocity,
                region_size: (
                    sl_proto::STANDARD_REGION_SIZE_METRES,
                    sl_proto::STANDARD_REGION_SIZE_METRES,
                ),
            });
        })
        .await;

    let mut shutdown_rx = destination.shutdown_rx.clone();
    if !wait_for_arrival(&mut dest_events, &mut shutdown_rx, CROSSING_ARRIVAL_TIMEOUT).await {
        tracing::warn!(
            "the crossing of session {source_seq} into {dest_name:?} timed out; the agent stays \
             where it was"
        );
        // A child that was already there is left alone — it is still a
        // neighbour, and the client still holds its circuit. Only one opened
        // for this crossing is taken back down.
        if announced {
            core.remove_session(dest_seq).await;
            destination.with_sim(sl_proto::SimSession::abandon).await;
        }
        return Err(Error::CrossingTimedOut);
    }

    source
        .with_sim(sl_proto::SimSession::make_child_agent)
        .await;
    neighbours::retire_distant_children(core, agent_id, region_index).await;
    tracing::info!(
        "crossing: {} {} walked from session {source_seq} into {dest_name} (session {dest_seq})",
        account.config.first_name,
        account.config.last_name,
    );
    // Only lagging subscribers error; the crossing is complete regardless.
    drop(core.crossings_tx.send(CrossingNotice {
        agent_id,
        from_seq: source_seq,
        to_seq: dest_seq,
        region_name: dest_name,
    }));
    Ok(destination)
}

/// Waits for the destination's `AgentArrived`, or gives up after `timeout` —
/// the crossing counterpart of the teleport's own wait, and closed / lagged /
/// shutting-down for the same reasons.
async fn wait_for_arrival(
    events: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
    timeout: Duration,
) -> bool {
    let Some(deadline) = tokio::time::Instant::now().checked_add(timeout) else {
        return false;
    };
    if *shutdown_rx.borrow_and_update() {
        return false;
    }
    loop {
        let received = tokio::select! {
            received = tokio::time::timeout_at(deadline, events.recv()) => received,
            _ = shutdown_rx.changed() => return false,
        };
        match received {
            Ok(Ok(ServerEvent::AgentArrived)) => return true,
            Ok(
                Ok(ServerEvent::Disconnected | ServerEvent::LoggedOut)
                | Err(tokio::sync::broadcast::error::RecvError::Closed),
            )
            | Err(_) => return false,
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(missed))) => {
                tracing::warn!("the crossing arrival wait missed {missed} events");
            }
        }
    }
}
