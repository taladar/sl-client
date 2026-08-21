//! Inter-region teleport: the grid-side sequencing a sans-I/O
//! [`SimSession`] deliberately leaves to its driver.
//!
//! The wire sequence mirrors OpenSim's `EntityTransferModule`
//! (`TransferAgent_V2`): `TeleportStart` + progress on the source, a
//! **second** session in the destination region (a `SimSession` has its
//! region handle fixed at construction, so a teleport is always a new
//! socket/session/CAPS triple), the event-queue trio `EnableSimulator` +
//! `EstablishAgentCommunication` (the client opens a child circuit and POSTs
//! the destination seed) + `TeleportFinish` (the client promotes the child
//! with `CompleteAgentMovement`), and — only once the destination saw the
//! arrival — the source circuit's retirement with `DisableSimulator`.
//!
//! Two entry points share [`teleport_session`]: the per-session
//! responder task answering the client's own requests (location, landmark,
//! home, lure), and the explicit [`crate::FakeGrid::teleport_agent`].

use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_proto::{
    ArrivalPlacement, AssetSource as _, ServerEvent, SimSession, TeleportFinishInfo,
    teleport_strings,
};
use sl_types::map::{RegionCoordinates, TeleportFlags};
use sl_wire::{FakeParcelId, LandmarkAsset};
use tokio::sync::broadcast;

use crate::driver::SharedSim;
use crate::error::Error;
use crate::runtime::{GridCore, TeleportNotice};

/// How long the grid waits for the client to complete its movement into the
/// destination before it fails the teleport with `timeout_tport` and
/// abandons the destination session (OpenSim's `WaitForAgentArrivedAtDestination`
/// budget is in the same range).
pub const TELEPORT_ARRIVAL_TIMEOUT: Duration = Duration::from_secs(30);

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
                let now = Instant::now();
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

    // The black screen goes up, and the viewer learns what is happening.
    source
        .with_sim(|sim| {
            let now = Instant::now();
            sim.send_teleport_start(request.flags, now)?;
            sim.send_teleport_progress(teleport_strings::RESOLVING, request.flags, now)?;
            sim.send_teleport_progress(request.progress, request.flags, now)
        })
        .await?;

    // The destination session, registered before it is announced: the
    // client POSTs the seed the moment `EstablishAgentCommunication`
    // arrives, and an unregistered `/sim/<seq>/…` answers 404.
    let prepared = core
        .prepare_region_session(&account, request.region, ids, Some(request.arrival))
        .await?;
    core.activate_session(&prepared).await;
    let dest = prepared.shared.clone();
    // Subscribe before announcing, or the arrival can slip past.
    let mut dest_events = dest.subscribe_events();

    let finish = TeleportFinishInfo {
        agent_id,
        location_id: sl_proto::TELEPORT_FINISH_LOCATION_ID,
        dest: prepared.udp_addr,
        region_handle: dest_handle,
        seed: prepared.seed_url.to_string(),
        sim_access,
        teleport_flags: request.flags,
        region_size: (
            sl_proto::STANDARD_REGION_SIZE_METRES,
            sl_proto::STANDARD_REGION_SIZE_METRES,
        ),
    };
    source
        .with_sim(|sim| {
            let now = Instant::now();
            sim.enqueue_enable_simulator(dest_handle, prepared.udp_addr);
            sim.enqueue_establish_agent_communication(
                prepared.udp_addr,
                prepared.seed_url.as_str(),
            );
            sim.send_teleport_progress(teleport_strings::ARRIVING, request.flags, now)?;
            sim.enqueue_teleport_finish(&finish);
            Ok::<(), sl_proto::Error>(())
        })
        .await?;

    if !wait_for_arrival(&mut dest_events, TELEPORT_ARRIVAL_TIMEOUT).await {
        tracing::warn!(
            "teleport of session {source_seq} to {dest_name:?} timed out; abandoning session {}",
            prepared.seq
        );
        core.remove_session(prepared.seq).await;
        dest.with_sim(SimSession::abandon).await;
        if let Err(error) = source
            .with_sim(|sim| {
                sim.send_teleport_failed(teleport_strings::TIMEOUT_TPORT, Instant::now())
            })
            .await
        {
            tracing::warn!("reporting the teleport timeout failed: {error}");
        }
        return Err(Error::TeleportTimedOut);
    }

    // The avatar is in the destination: retire the source, which the
    // client now holds as a child circuit.
    if let Err(error) = source
        .with_sim(|sim| sim.retire_circuit(Instant::now()))
        .await
    {
        tracing::warn!("retiring the source circuit failed: {error}");
    }
    core.remove_session(source_seq).await;
    tracing::info!(
        "teleport: {} {} moved from session {source_seq} to {dest_name} (session {})",
        account.config.first_name,
        account.config.last_name,
        prepared.seq
    );
    // Only lagging subscribers error; the teleport is complete regardless.
    drop(core.teleports_tx.send(TeleportNotice {
        agent_id,
        from_seq: source_seq,
        to_seq: prepared.seq,
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
            let body = {
                let state = source.state.lock().await;
                state
                    .assets
                    .get(*landmark)
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            };
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
/// Waits for the destination's `AgentArrived`, or gives up after `timeout`
/// (also on a closed or hopelessly lagged event stream).
async fn wait_for_arrival(
    events: &mut broadcast::Receiver<ServerEvent>,
    timeout: Duration,
) -> bool {
    let Some(deadline) = tokio::time::Instant::now().checked_add(timeout) else {
        return false;
    };
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
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
/// the way a simulator does, and exits when the session closes. A request
/// that resolves nowhere is refused with the matching `TeleportFailed` key, so
/// the viewer's teleport screen never hangs.
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
        loop {
            let event = match events.recv().await {
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
            let now = Instant::now();
            sim.send_teleport_start(0, now)?;
            sim.send_teleport_failed(reason, now)
        })
        .await;
    if let Err(error) = result {
        tracing::warn!("reporting a refused teleport failed: {error}");
    }
}
