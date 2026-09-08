//! Inter-region teleport: the grid-side sequencing a sans-I/O
//! [`SimSession`] deliberately leaves to its driver.
//!
//! The wire sequence mirrors OpenSim's `EntityTransferModule`
//! (`TransferAgent_V2`): `TeleportStart` + progress on the source, a
//! **second** session in the destination region (a `SimSession` has its
//! region handle fixed at construction, so a teleport is always a new
//! socket/session/CAPS triple), a `TeleportFinish` on the source's event
//! queue naming that destination and its seed — and **nothing announcing the
//! destination ahead of it** — after which the client opens the destination's
//! circuit itself (`UseCircuitCode` + `CompleteAgentMovement`), and, only once
//! the destination saw the arrival, the source circuit's retirement with
//! `DisableSimulator`.
//!
//! The absent announcement is the part worth stating, because the grid used to
//! send one. `TransferAgent_V2` says so in the source — "New protocol: send TP
//! Finish directly, without prior ES or EAC. That's what happens in the Linden
//! grid" — and only the legacy `TransferAgent_V1` prefixes the finish with
//! `EnableSimulator` + `EstablishAgentCommunication`, and even then only for a
//! destination outside view range. A destination handed over as a child
//! circuit *before* the finish is indistinguishable, to the client, from a
//! neighbour it has been holding all along, which is how this grid used to make
//! every distant teleport keep the world it should have thrown away.
//!
//! A destination that is genuinely a **neighbour** was of course already
//! announced — by [`crate::neighbours`], as a neighbour, long before any
//! teleport — and this path reuses that session rather than opening a second
//! one for the same region handle.
//!
//! Two entry points share [`teleport_session`]: the per-session
//! responder task answering the client's own requests (location, landmark,
//! home, lure), and the explicit [`crate::FakeGrid::teleport_agent`].

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use sl_proto::{
    ArrivalPlacement, AssetSource as _, ServerEvent, SimSession, TeleportFinishInfo,
    teleport_strings,
};
use sl_types::map::{RegionCoordinates, TeleportFlags};
use sl_wire::{FakeParcelId, LandmarkAsset, SequenceNumber};
use tokio::sync::{broadcast, watch};

use crate::driver::SharedSim;
use crate::error::Error;
use crate::runtime::{GridCore, TeleportNotice};

/// How long the grid waits for the client to complete its movement into the
/// destination before it fails the teleport with `timeout_tport` and
/// abandons the destination session (OpenSim's `WaitForAgentArrivedAtDestination`
/// budget is in the same range).
pub const TELEPORT_ARRIVAL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the grid waits for the client to acknowledge the `TeleportStart`
/// before it sends the `TeleportFinish` anyway.
///
/// A real simulator does not wait for this ack, and does not have to: the
/// handover it performs between the start and the finish is tens of
/// milliseconds of real work (serialising the agent, posting it to the
/// destination simulator, waiting for that simulator to build it). Here the
/// destination is often a neighbour session that is already open, so the two
/// would leave microseconds apart — and they leave on *two transports*, the
/// start over UDP and the finish over the CAPS event queue, whose relative
/// order no client fixes: `sl-client-tokio`'s driver selects over its socket
/// and its event-queue channel without bias, so whichever is ready wins a coin
/// flip. Losing it reorders the client's own teleport phases to
/// finished-then-started. This wait is the happens-before edge that keeps them
/// in the order every viewer expects.
const TELEPORT_START_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// How often [`wait_for_start_ack`] re-reads the source session's outstanding
/// packets. Over loopback the ack lands within a poll or two.
const TELEPORT_START_ACK_POLL: Duration = Duration::from_millis(2);

/// What a teleport asks for: where, how the arrival is placed, and how it is
/// reported (flags + the progress key the viewer localises).
#[derive(Debug, Clone)]
pub(crate) struct TeleportRequest {
    /// The destination region's index in the grid's region table.
    pub(crate) region: usize,
    /// Where the agent lands.
    pub(crate) arrival: ArrivalPlacement,
    /// The `TeleportFlags` bitfield (how the teleport happened).
    pub(crate) flags: u32,
    /// The `TeleportProgress` key sent while the destination is prepared
    /// (`sending_dest` / `sending_home` / `sending_landmark`).
    pub(crate) progress: &'static str,
}

/// How a teleport ended.
pub(crate) enum TeleportOutcome {
    /// The destination was the agent's own region: a `TeleportLocal` moved it
    /// in place, no new session.
    Local,
    /// The agent arrived in the destination session; the source is retired.
    Moved(SharedSim),
}

/// Runs one teleport for the agent of `source` (see the module docs for
/// the wire sequence). A same-region request is answered with
/// `TeleportLocal`.
///
/// # Errors
///
/// [`Error::NotRootAgent`] when the source's agent has not arrived,
/// [`Error::UnknownRegion`] for a bad region index, [`Error::UnknownAccount`]
/// for a session no account owns, [`Error::TeleportTimedOut`] when the
/// client never arrived (the client was told `timeout_tport`), and socket
/// errors binding the destination.
pub(crate) async fn teleport_session(
    core: &Arc<GridCore>,
    source: &SharedSim,
    request: TeleportRequest,
) -> Result<TeleportOutcome, Error> {
    let dest_region = core.region(request.region).ok_or(Error::UnknownRegion {
        region: request.region.to_string(),
    })?;
    let dest_handle = dest_region.handle();
    let dest_name = dest_region.config.name.clone();
    let sim_access = dest_region.config.maturity.to_sim_access();

    // Read what the destination session inherits, and reject a session
    // whose agent is not actually here.
    let (source_seq, source_region, ids, agent_id) = {
        let state = source.state.lock().await;
        if !state.sim.is_root_agent() {
            return Err(Error::NotRootAgent);
        }
        (state.seq, state.region, state.ids, state.avatar.agent_id)
    };
    let account = core
        .account_by_agent(agent_id)
        .cloned()
        .ok_or(Error::UnknownAccount)?;

    if source_region == request.region {
        source
            .with_sim(|sim| {
                let now = source.now();
                sim.send_teleport_start(request.flags, now)?;
                sim.send_teleport_local(
                    request.arrival.position,
                    request.arrival.look_at.clone(),
                    request.flags,
                    now,
                )
            })
            .await?;
        return Ok(TeleportOutcome::Local);
    }

    // The black screen goes up, and the viewer learns what is happening. The
    // start's sequence number is kept so the finish can be held behind the
    // client's acknowledgement of it ([`TELEPORT_START_ACK_TIMEOUT`]).
    let start_sequence = source
        .with_sim(|sim| {
            let now = source.now();
            let sequence = sim.next_outgoing_sequence();
            sim.send_teleport_start(request.flags, now)?;
            sim.send_teleport_progress(teleport_strings::RESOLVING, request.flags, now)?;
            sim.send_teleport_progress(request.progress, request.flags, now)?;
            Ok::<_, sl_proto::Error>(sequence)
        })
        .await?;

    // The destination session. A neighbour of the source is already open as a
    // child circuit ([`crate::neighbours`]) — reuse it, or the client is handed
    // two simulators for one region handle and streams the destination's scene
    // twice. Otherwise a fresh one, registered before the finish names it: the
    // client contacts the destination the moment the `TeleportFinish` arrives,
    // and an unregistered `/sim/<seq>/…` answers 404 to the seed it POSTs.
    let (dest, dest_seq, dest_addr, dest_seed, opened_here) =
        match core.session_of(agent_id, request.region).await {
            Some(shared) => {
                let (seq, addr, seed) = {
                    let state = shared.state.lock().await;
                    (state.seq, state.udp_addr, state.seed_url.to_string())
                };
                shared
                    .with_sim(|sim| {
                        sim.set_arrival_position(
                            request.arrival.position,
                            request.arrival.look_at.clone(),
                        );
                    })
                    .await;
                (shared, seq, addr, seed, false)
            }
            None => {
                let prepared = core
                    .prepare_region_session(
                        &account,
                        request.region,
                        ids,
                        Some(request.arrival.clone()),
                        crate::runtime::SessionRole::Root,
                    )
                    .await?;
                core.activate_session(&prepared).await;
                (
                    prepared.shared.clone(),
                    prepared.seq,
                    prepared.udp_addr,
                    prepared.seed_url.to_string(),
                    true,
                )
            }
        };
    // Subscribe before the finish goes out, or the arrival can slip past.
    let mut dest_events = dest.subscribe_events();

    let finish = TeleportFinishInfo {
        agent_id,
        location_id: sl_proto::TELEPORT_FINISH_LOCATION_ID,
        dest: dest_addr,
        region_handle: dest_handle,
        seed: dest_seed,
        sim_access,
        teleport_flags: request.flags,
        region_size: (
            sl_proto::STANDARD_REGION_SIZE_METRES,
            sl_proto::STANDARD_REGION_SIZE_METRES,
        ),
    };
    // Everything below reaches the client over the CAPS event queue, which
    // races the UDP `TeleportStart` above unless the client has already taken
    // delivery of it. Preparing the destination has run concurrently with the
    // ack, so on a healthy circuit this has usually already come back.
    if !wait_for_start_ack(source, start_sequence).await {
        tracing::debug!(
            "teleport of session {source_seq}: no TeleportStart ack within \
             {TELEPORT_START_ACK_TIMEOUT:?}; sending the finish regardless"
        );
    }

    source
        .with_sim(|sim| {
            let now = source.now();
            // The destination is **not** announced first. `TransferAgent_V2`
            // sends the finish on its own — "New protocol: send TP Finish
            // directly, without prior ES or EAC. That's what happens in the
            // Linden grid" — and only the legacy `TransferAgent_V1` puts an
            // `EnableSimulator` + `EstablishAgentCommunication` in front of it,
            // and then only for a destination outside view range. The reference
            // viewer needs no announcement either: `process_teleport_finish`
            // sends `UseCircuitCode` to the address the finish names whether or
            // not it already holds that region.
            //
            // The difference is not cosmetic. A client that is handed the
            // destination as a child circuit before the finish cannot tell a
            // teleport destination from a neighbour it has been holding all
            // along, so it keeps the departed world instead of dropping it —
            // which is exactly how this grid made every distant teleport report
            // `world_reset = false`. See the roadmap task
            // `viewer-teleport-never-resets-the-world`.
            sim.send_teleport_progress(teleport_strings::ARRIVING, request.flags, now)?;
            sim.enqueue_teleport_finish(&finish);
            Ok::<(), sl_proto::Error>(())
        })
        .await?;

    let mut shutdown_rx = dest.shutdown_rx.clone();
    let budget = core.handover_timeout.unwrap_or(TELEPORT_ARRIVAL_TIMEOUT);
    if !wait_for_arrival(&mut dest_events, &mut shutdown_rx, budget).await {
        tracing::warn!(
            "teleport of session {source_seq} to {dest_name:?} timed out; abandoning session \
             {dest_seq}"
        );
        // Only a session opened for this teleport is abandoned. A child circuit
        // that was already there is a neighbour the client still holds, and
        // tearing it down would punish it for a teleport that failed.
        if opened_here {
            core.remove_session(dest_seq).await;
            dest.with_sim(SimSession::abandon).await;
        }
        if let Err(error) = source
            .with_sim(|sim| sim.send_teleport_failed(teleport_strings::TIMEOUT_TPORT, source.now()))
            .await
        {
            tracing::warn!("reporting the teleport timeout failed: {error}");
        }
        return Err(Error::TeleportTimedOut);
    }

    // The avatar is in the destination: retire the source, which the
    // client now holds as a child circuit.
    if let Err(error) = source
        .with_sim(|sim| sim.retire_circuit(source.now()))
        .await
    {
        tracing::warn!("retiring the source circuit failed: {error}");
    }
    core.remove_session(source_seq).await;
    // The neighbours of the region left behind are neighbours no longer. A
    // crossing has always retired them; a teleport used to leave one open
    // circuit per region the agent had ever bordered, still streaming to a
    // client that has moved to the far side of the grid. The destination's own
    // neighbours are announced by its arrival, and this keeps only those.
    crate::neighbours::retire_distant_children(core, agent_id, request.region).await;
    tracing::info!(
        "teleport: {} {} moved from session {source_seq} to {dest_name} (session {dest_seq})",
        account.config.first_name,
        account.config.last_name,
    );
    // Only lagging subscribers error; the teleport is complete regardless.
    drop(core.teleports_tx.send(TeleportNotice {
        agent_id,
        from_seq: source_seq,
        to_seq: dest_seq,
        region_name: dest_name,
    }));
    Ok(TeleportOutcome::Moved(dest))
}

/// Resolves a client teleport request into a [`TeleportRequest`], or the
/// `TeleportFailed` key to answer with.
async fn resolve_request(
    core: &GridCore,
    source: &SharedSim,
    event: &ServerEvent,
) -> Option<Result<TeleportRequest, &'static str>> {
    match event {
        ServerEvent::TeleportRequested {
            region_handle,
            position,
            look_at,
        } => Some(core.region_by_handle(*region_handle).map_or(
            Err(teleport_strings::INVALID_TPORT),
            |region| {
                Ok(TeleportRequest {
                    region,
                    arrival: ArrivalPlacement {
                        position: *position,
                        look_at: look_at.clone(),
                    },
                    flags: TeleportFlags::VIA_LOCATION,
                    progress: teleport_strings::SENDING_DEST,
                })
            },
        )),
        ServerEvent::TeleportViaLandmark { landmark: None } => {
            // Home: the account's start region, at its centre.
            let agent_id = source.state.lock().await.avatar.agent_id;
            Some(
                core.account_by_agent(agent_id)
                    .and_then(|account| core.start_region(account))
                    .map_or(Err(teleport_strings::INVALID_TPORT), |region| {
                        Ok(TeleportRequest {
                            region,
                            arrival: ArrivalPlacement::default(),
                            flags: TeleportFlags::VIA_HOME,
                            progress: teleport_strings::SENDING_HOME,
                        })
                    }),
            )
        }
        ServerEvent::TeleportViaLandmark {
            landmark: Some(landmark),
        } => {
            // The grid-wide store: a landmark is an asset like any other, and
            // the region the agent happens to be standing in has nothing to do
            // with whether the grid holds it.
            let body = core
                .assets
                .read()
                .get(*landmark)
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            let parsed = body.and_then(|text| sl_wire::parse_landmark(&text).ok());
            let resolved = parsed.and_then(|asset| {
                let region = match &asset {
                    LandmarkAsset::Regional { region_id, .. } => core.region_by_id(*region_id),
                    LandmarkAsset::Global(global) => global.split().and_then(|(grid, _)| {
                        core.region_by_handle(sl_wire::RegionHandle::from_grid(grid.x(), grid.y()))
                    }),
                }?;
                let position = asset.local_position()?;
                Some((region, position))
            });
            Some(resolved.map_or(
                Err(teleport_strings::NOLANDMARK_TPORT),
                |(region, position)| {
                    Ok(TeleportRequest {
                        region,
                        arrival: ArrivalPlacement {
                            position,
                            look_at: ArrivalPlacement::default().look_at,
                        },
                        flags: TeleportFlags::VIA_LANDMARK,
                        progress: teleport_strings::SENDING_LANDMARK,
                    })
                },
            ))
        }
        ServerEvent::TeleportViaLure {
            lure_id,
            teleport_flags,
        } => {
            // OpenSim packs the destination into the lure id (a "fake
            // parcel id": handle + position); an opaque id is taken as
            // the offering agent's id, landing next to them.
            let flags = *teleport_flags | TeleportFlags::VIA_LURE;
            let place = FakeParcelId::parse(lure_id.get());
            let target = match place {
                Some(place) => core.region_by_handle(place.region_handle).map(|region| {
                    (
                        region,
                        RegionCoordinates::new(
                            f32::from(place.x),
                            f32::from(place.y),
                            f32::from(place.z),
                        ),
                    )
                }),
                None => {
                    let lurer = sl_types::key::AgentKey::from(lure_id.get());
                    match core.root_session_of(lurer).await {
                        Some(shared) => {
                            let region = shared.state.lock().await.region;
                            Some((region, ArrivalPlacement::default().position))
                        }
                        None => None,
                    }
                }
            };
            Some(
                target.map_or(Err(teleport_strings::NO_HOST), |(region, position)| {
                    Ok(TeleportRequest {
                        region,
                        arrival: ArrivalPlacement {
                            position,
                            look_at: ArrivalPlacement::default().look_at,
                        },
                        flags,
                        progress: teleport_strings::SENDING_DEST,
                    })
                }),
            )
        }
        _ => None,
    }
}
/// Waits until the client has acknowledged the `TeleportStart` sent as
/// `sequence`, giving up after [`TELEPORT_START_ACK_TIMEOUT`] or on the grid
/// shutting down. Returns whether the ack arrived.
///
/// The caller proceeds either way: this orders two messages the client would
/// otherwise see in either order, and an ordering nicety must never be able to
/// strand a teleport. The session is read directly rather than through
/// [`SharedSim::with_sim`] because nothing here mutates it, and a flush per
/// poll would be pure churn.
async fn wait_for_start_ack(source: &SharedSim, sequence: SequenceNumber) -> bool {
    let Some(deadline) = tokio::time::Instant::now().checked_add(TELEPORT_START_ACK_TIMEOUT) else {
        return false;
    };
    let mut shutdown_rx = source.shutdown_rx.clone();
    loop {
        if !source.state.lock().await.sim.is_awaiting_ack(sequence) {
            return true;
        }
        if *shutdown_rx.borrow_and_update() {
            return false;
        }
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => return false,
            () = tokio::time::sleep(TELEPORT_START_ACK_POLL) => {}
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return false;
                }
            }
        }
    }
}

/// Waits for the destination's `AgentArrived`, or gives up after `timeout`
/// (also on a closed or hopelessly lagged event stream, and on the grid
/// shutting down — teardown must not wait out the arrival budget).
async fn wait_for_arrival(
    events: &mut broadcast::Receiver<ServerEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
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
                | Err(broadcast::error::RecvError::Closed),
            )
            | Err(_) => return false,
            Ok(Ok(_)) => {}
            Ok(Err(broadcast::error::RecvError::Lagged(missed))) => {
                tracing::warn!("teleport arrival wait missed {missed} events");
            }
        }
    }
}

/// The per-session responder: answers the client's own teleport requests
/// (`TeleportLocationRequest`, `TeleportLandmarkRequest`, `TeleportLureRequest`)
/// the way a simulator does, and exits when the session closes or the grid
/// shuts down. A request that resolves nowhere is refused with the matching
/// `TeleportFailed` key, so the viewer's teleport screen never hangs.
///
/// The closed and shutdown branches are load-bearing: this task owns a
/// [`SharedSim`], and therefore the session's own `events_tx`, so the
/// broadcast it awaits can never report `Closed` on its own.
///
/// Boxed: activating a session spawns a responder, and a responder's teleport
/// activates the destination session — the explicit `dyn Future` breaks the
/// otherwise infinitely recursive future type.
pub(crate) fn run_teleport_responder(
    core: Arc<GridCore>,
    shared: SharedSim,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
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
            let event = match received {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!("teleport responder missed {missed} events");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            if matches!(
                event,
                ServerEvent::Disconnected | ServerEvent::LoggedOut | ServerEvent::CircuitRetired
            ) {
                break;
            }
            let Some(resolved) = resolve_request(&core, &shared, &event).await else {
                continue;
            };
            match resolved {
                Ok(request) => match teleport_session(&core, &shared, request).await {
                    // The source session is retired (or the teleport was local):
                    // this responder's job is done either way for a move.
                    Ok(TeleportOutcome::Moved(_)) => break,
                    // A local hop keeps this session; a timed-out move was
                    // already reported to the client as `timeout_tport`.
                    Ok(TeleportOutcome::Local) | Err(Error::TeleportTimedOut) => {}
                    Err(error) => {
                        tracing::warn!("teleport failed: {error}");
                        let reason = match error {
                            Error::NotRootAgent => teleport_strings::INVALID_REGION_HANDOFF,
                            _ => teleport_strings::NO_HOST,
                        };
                        report_failure(&shared, reason).await;
                    }
                },
                Err(reason) => report_failure(&shared, reason).await,
            }
        }
    })
}

/// Answers a refused request with `TeleportFailed` (after the `TeleportStart`
/// a viewer expects before a failure).
async fn report_failure(shared: &SharedSim, reason: &'static str) {
    let result = shared
        .with_sim(|sim| {
            let now = shared.now();
            sim.send_teleport_start(0, now)?;
            sim.send_teleport_failed(reason, now)
        })
        .await;
    if let Err(error) = result {
        tracing::warn!("reporting a refused teleport failed: {error}");
    }
}
