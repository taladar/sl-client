//! Inter-region teleport: the grid-side sequencing a sans-I/O
//! [`SimSession`] deliberately leaves to its driver.
//!
//! The skeleton is the one both live grids share, and OpenSim's
//! `EntityTransferModule` (`TransferAgent_V2`) spells out: `TeleportStart` on
//! the source (with the progress lines Second Life narrates and OpenSim does
//! not — [`TeleportPolicy`](crate::imitates::TeleportPolicy)), a
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
    AlertInfo, ArrivalPlacement, AssetSource as _, ServerEvent, SimSession, TeleportFinishInfo,
    teleport_strings,
};
use sl_types::map::{RegionCoordinates, TeleportFlags};
use sl_wire::{FakeParcelId, LandmarkAsset, SequenceNumber};
use tokio::sync::{broadcast, watch};

use crate::driver::SharedSim;
use crate::error::Error;
use crate::imitates::{
    CancelAnswer, LocalLookAt, MATURITY_REFUSAL_ALERT, MATURITY_REFUSAL_REASON, RefusalTransport,
    TeleportPolicy,
};
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
    /// The `TeleportProgress` key naming the kind of teleport, on a grid that
    /// names it (`sending_home` / `sending_landmark`); `None` for a teleport
    /// to a location, which neither grid names.
    pub(crate) kind_line: Option<&'static str>,
    /// Whether the client asked for this teleport itself. Only such a
    /// teleport can be cancelled or refused for the agent's maturity
    /// preference; one the grid decided on is carried out.
    pub(crate) client_requested: bool,
}

/// A teleport the grid refuses: what the client is told, and how much of the
/// teleport it saw before the refusal on a grid that starts one first.
#[derive(Debug, Clone)]
pub(crate) struct Refusal {
    /// The flags of the `TeleportStart` and progress lines.
    flags: u32,
    /// The progress lines sent ahead of the failure (event-queue refusals
    /// only: a UDP refusal is sent in place of the start).
    progress: Vec<&'static str>,
    /// The failure reason.
    reason: String,
    /// The alert key and parameters, when they differ from "the reason
    /// again, with no parameters".
    alert: Option<AlertInfo>,
}

impl Refusal {
    /// A refusal whose alert, on a grid that sends one, is its reason again.
    fn plain(flags: u32, progress: Vec<&'static str>, reason: &str) -> Self {
        Self {
            flags,
            progress,
            reason: reason.to_owned(),
            alert: None,
        }
    }
}

/// How a teleport ended.
pub(crate) enum TeleportOutcome {
    /// The destination was the agent's own region: a `TeleportLocal` moved it
    /// in place, no new session.
    Local,
    /// The grid refused the teleport, or the client cancelled it in time; the
    /// client has been told and the agent is where it was.
    Refused,
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
    mut request: TeleportRequest,
) -> Result<TeleportOutcome, Error> {
    let dest_region = core.region(request.region).ok_or(Error::UnknownRegion {
        region: request.region.to_string(),
    })?;
    // A simulator never leaves an avatar under its ground: a place below the
    // terrain — a map teleport asks for height 0 — lands on it (OpenSim's
    // `ScenePresence` lifts it to the ground), with the avatar's centre half
    // its height above. The fake grid runs no physics, so nothing else would.
    let wanted = request.arrival.position;
    let ground = dest_region.config.terrain.height_at(wanted.x(), wanted.y());
    if wanted.z() < ground {
        request.arrival.position = RegionCoordinates::new(
            wanted.x(),
            wanted.y(),
            ground + crate::AVATAR_CENTRE_ABOVE_GROUND_M,
        );
    }
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

    let policy = core.teleport_policy;
    if source_region == request.region {
        let flags = request.flags | policy.local_flags;
        let look_at = local_look_at(policy.local_look_at, &request.arrival);
        source
            .with_state(|state| {
                let now = source.now();
                let sim = &mut state.sim;
                sim.send_teleport_start(flags, now)?;
                // Where the agent now stands: what the region measures chat
                // range from, and what a later movement completes to.
                sim.set_arrival_position(request.arrival.position, request.arrival.look_at.clone());
                sim.send_teleport_local(request.arrival.position, look_at, flags, now)?;
                // The avatar has moved, and a simulator says so: OpenSim's
                // `ScenePresence.Teleport` sends the presence's update to
                // every client the moment it is placed. The fake grid runs no
                // physics, so without this the avatar would go on standing,
                // for everyone, where it was.
                let landing = request.arrival.position;
                let avatar = crate::world::avatar_prim(
                    state.world.lock().avatar_local_id,
                    &state.avatar,
                    sl_types::lsl::Vector {
                        x: landing.x(),
                        y: landing.y(),
                        z: landing.z(),
                    },
                );
                crate::world::send_objects(&mut state.sim, &[avatar], now)
            })
            .await?;
        return Ok(TeleportOutcome::Local);
    }

    // The lines this grid narrates the teleport with: the kind, on a grid
    // that names it, then the grid's own.
    let mut lines: Vec<&'static str> = Vec::new();
    if policy.names_the_kind {
        lines.extend(request.kind_line);
    }
    lines.extend_from_slice(policy.progress);

    // A region rated above what the agent has said it wants to see is refused
    // by a grid that checks, once it has got as far as sending the agent — so
    // after every line. Only a location request: the home teleport of an agent
    // set to General into an Adult home region went through on aditi.
    if policy.enforces_maturity_preference
        && request.client_requested
        && request.flags & TeleportFlags::VIA_LOCATION != 0
    {
        let stored = source
            .with_sim(|sim| sim.agent_preferences().max_access_pref.clone())
            .await;
        let preference = sl_proto::Maturity::from_login_access(stored.as_deref());
        let wanted = dest_region.config.maturity;
        // An agent that has stated no preference is not refused for one.
        if !matches!(preference, sl_proto::Maturity::Unknown) && !wanted.permitted_by(preference) {
            let refusal = Refusal {
                flags: request.flags,
                progress: lines,
                reason: MATURITY_REFUSAL_REASON.to_owned(),
                alert: Some(AlertInfo {
                    message: MATURITY_REFUSAL_ALERT.to_owned(),
                    extra_params: TeleportPolicy::maturity_refusal_params(sim_access),
                }),
            };
            report_refusal(source, &policy, &refusal).await;
            return Ok(TeleportOutcome::Refused);
        }
    }

    // The black screen goes up, and the viewer learns what is happening. The
    // sequence number of the last of these is kept so the finish can be held
    // behind the client's acknowledgement of it
    // ([`TELEPORT_START_ACK_TIMEOUT`]): the start, on a grid that narrates
    // nothing, and otherwise the final line — a finish that overtook a
    // progress line would hand the client the lines of a teleport it has
    // already been told is over.
    let start_sequence = source
        .with_sim(|sim| {
            let now = source.now();
            let mut last = sim.next_outgoing_sequence();
            sim.send_teleport_start(request.flags, now)?;
            for line in &lines {
                last = sim.next_outgoing_sequence();
                sim.send_teleport_progress(line, request.flags, now)?;
            }
            Ok::<_, sl_proto::Error>(last)
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
    // What the agent has told the grid about itself goes where the agent
    // goes: the destination session answers the preferences capability next.
    let preferences = source.with_sim(|sim| sim.agent_preferences().clone()).await;
    dest.with_sim(|sim| sim.merge_agent_preferences(&preferences))
        .await;
    // Subscribe before the finish goes out, or the arrival can slip past.
    let mut dest_events = dest.subscribe_events();

    let finish = TeleportFinishInfo {
        agent_id,
        location_id: policy.finish_location_id,
        dest: dest_addr,
        region_handle: dest_handle,
        seed: dest_seed,
        sim_access,
        teleport_flags: request.flags,
        region_size: policy.finish_states_region_size.then_some((
            sl_proto::STANDARD_REGION_SIZE_METRES,
            sl_proto::STANDARD_REGION_SIZE_METRES,
        )),
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

    // The last moment a cancel counts: the client's `TeleportCancel` for this
    // request, if it sent one, was on the wire before its acknowledgement of
    // the start. A grid that honours it abandons the teleport here and says
    // so; one that does not carries on, and the client that cancelled is moved
    // anyway.
    if request.client_requested
        && let CancelAnswer::Failed { reason, alert } = policy.cancel
        && source.with_sim(SimSession::take_teleport_cancel).await
    {
        if opened_here {
            core.remove_session(dest_seq).await;
            dest.with_sim(SimSession::abandon).await;
        }
        let answer = AlertInfo {
            message: alert.to_owned(),
            extra_params: String::new(),
        };
        source
            .with_sim(|sim| send_failure(sim, &policy, reason, Some(&answer), source.now()))
            .await?;
        return Ok(TeleportOutcome::Refused);
    }

    source
        .with_sim(|sim| {
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
            .with_sim(|sim| {
                send_failure(
                    sim,
                    &policy,
                    teleport_strings::TIMEOUT_TPORT,
                    None,
                    source.now(),
                )
            })
            .await
        {
            tracing::warn!("reporting the teleport timeout failed: {error}");
        }
        return Err(Error::TeleportTimedOut);
    }

    // The avatar is in the destination, so its script is too: a timeline
    // belongs to the avatar running it, not to the region it started in. Done
    // before the source is retired, and for a client-initiated teleport as much
    // as for a scripted one ([`crate::timeline::hand_over`]).
    crate::timeline::hand_over(source, &dest).await;

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

/// Which way a `TeleportLocal` says the agent faces, as each grid does.
fn local_look_at(rule: LocalLookAt, arrival: &ArrivalPlacement) -> sl_types::lsl::Vector {
    match rule {
        // OpenSim flattens what was asked for (`lookAt.Z = 0f`) and faces east
        // when nothing horizontal is left.
        LocalLookAt::Requested => {
            let asked = &arrival.look_at;
            if asked.x.abs() < 0.01 && asked.y.abs() < 0.01 {
                sl_types::lsl::Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                }
            } else {
                sl_types::lsl::Vector {
                    x: asked.x,
                    y: asked.y,
                    z: 0.0,
                }
            }
        }
        LocalLookAt::TowardsRegionOrigin => {
            let at = arrival.position;
            let length = at
                .x()
                .mul_add(at.x(), at.y().mul_add(at.y(), at.z() * at.z()))
                .sqrt();
            if length > f32::EPSILON {
                sl_types::lsl::Vector {
                    x: -at.x() / length,
                    y: -at.y() / length,
                    z: -at.z() / length,
                }
            } else {
                arrival.look_at.clone()
            }
        }
    }
}

/// Resolves a client teleport request into a [`TeleportRequest`], or the
/// [`Refusal`] to answer with.
async fn resolve_request(
    core: &GridCore,
    source: &SharedSim,
    event: &ServerEvent,
) -> Option<Result<TeleportRequest, Refusal>> {
    let policy = core.teleport_policy;
    // What a grid that starts a teleport before refusing it had got to: the
    // kind's own line, or the first of its lines for a location.
    let first_line = |kind: Option<&'static str>| -> Vec<&'static str> {
        match kind {
            Some(kind) if policy.names_the_kind => vec![kind],
            _unnamed => policy.progress.first().copied().into_iter().collect(),
        }
    };
    match event {
        ServerEvent::TeleportRequested {
            region_handle,
            position,
            look_at,
        } => Some(core.region_by_handle(*region_handle).map_or_else(
            || {
                Err(Refusal::plain(
                    TeleportFlags::VIA_LOCATION,
                    first_line(None),
                    policy.unknown_region,
                ))
            },
            |region| {
                Ok(TeleportRequest {
                    region,
                    arrival: ArrivalPlacement {
                        position: *position,
                        look_at: look_at.clone(),
                    },
                    flags: TeleportFlags::VIA_LOCATION,
                    kind_line: None,
                    client_requested: true,
                })
            },
        )),
        ServerEvent::TeleportViaLandmark { landmark: None } => {
            // Home: the account's start region, at its centre.
            let agent_id = source.state.lock().await.avatar.agent_id;
            Some(
                core.account_by_agent(agent_id)
                    .and_then(|account| core.start_region(account))
                    .map_or_else(
                        || {
                            Err(Refusal::plain(
                                TeleportFlags::VIA_HOME,
                                first_line(Some(teleport_strings::SENDING_HOME)),
                                policy.no_home,
                            ))
                        },
                        |region| {
                            Ok(TeleportRequest {
                                region,
                                arrival: ArrivalPlacement::default(),
                                flags: TeleportFlags::VIA_HOME,
                                kind_line: Some(teleport_strings::SENDING_HOME),
                                client_requested: true,
                            })
                        },
                    ),
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
            Some(resolved.map_or_else(
                || {
                    Err(Refusal::plain(
                        TeleportFlags::VIA_LANDMARK,
                        first_line(Some(teleport_strings::SENDING_LANDMARK)),
                        policy.unknown_landmark,
                    ))
                },
                |(region, position)| {
                    Ok(TeleportRequest {
                        region,
                        arrival: ArrivalPlacement {
                            position,
                            look_at: ArrivalPlacement::default().look_at,
                        },
                        flags: TeleportFlags::VIA_LANDMARK,
                        kind_line: Some(teleport_strings::SENDING_LANDMARK),
                        client_requested: true,
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
            // A lure's refusal is not measured yet (roadmap
            // `gridspec-teleport-lures`): the key the grid always sent, by
            // the flavour's transport.
            Some(target.map_or_else(
                || {
                    Err(Refusal::plain(
                        flags,
                        first_line(None),
                        teleport_strings::NO_HOST,
                    ))
                },
                |(region, position)| {
                    Ok(TeleportRequest {
                        region,
                        arrival: ArrivalPlacement {
                            position,
                            look_at: ArrivalPlacement::default().look_at,
                        },
                        flags,
                        kind_line: None,
                        client_requested: true,
                    })
                },
            ))
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
                    // A local hop keeps this session, and so does a refusal;
                    // a timed-out move was already reported to the client as
                    // `timeout_tport`.
                    Ok(TeleportOutcome::Local | TeleportOutcome::Refused)
                    | Err(Error::TeleportTimedOut) => {}
                    Err(error) => {
                        tracing::warn!("teleport failed: {error}");
                        let reason = match error {
                            Error::NotRootAgent => teleport_strings::INVALID_REGION_HANDOFF,
                            _ => teleport_strings::NO_HOST,
                        };
                        let refusal = Refusal::plain(0, Vec::new(), reason);
                        report_refusal(&shared, &core.teleport_policy, &refusal).await;
                    }
                },
                Err(refusal) => report_refusal(&shared, &core.teleport_policy, &refusal).await,
            }
        }
    })
}

/// Sends one teleport failure the way this grid sends them: the UDP message
/// with the bare reason, or the event-queue event with the reason and an
/// alert — `alert` when the grid gives the failure a key of its own, the
/// reason again otherwise.
fn send_failure(
    sim: &mut SimSession,
    policy: &TeleportPolicy,
    reason: &str,
    alert: Option<&AlertInfo>,
    now: std::time::Instant,
) -> Result<(), sl_proto::Error> {
    match policy.refusals {
        RefusalTransport::Udp => sim.send_teleport_failed(reason, now),
        RefusalTransport::EventQueue => {
            let repeated = AlertInfo {
                message: reason.to_owned(),
                extra_params: String::new(),
            };
            sim.enqueue_teleport_failed(reason, Some(alert.unwrap_or(&repeated)));
            Ok(())
        }
    }
}

/// Answers a refused request as this grid refuses: OpenSim with a
/// `TeleportFailed` and nothing before it, Second Life with the
/// `TeleportStart` and the progress lines it had reached, then the failure
/// over the event queue.
///
/// The event-queue failure is held behind the client's acknowledgement of the
/// last UDP line, for the reason the finish is
/// ([`TELEPORT_START_ACK_TIMEOUT`]): the two transports race, the live grid's
/// tenth of a second between them is what orders them there, and a failure
/// that overtook its own `TeleportStart` would leave the client starting a
/// teleport that had already ended.
async fn report_refusal(shared: &SharedSim, policy: &TeleportPolicy, refusal: &Refusal) {
    let narrated = if matches!(policy.refusals, RefusalTransport::EventQueue) {
        shared
            .with_sim(|sim| {
                let now = shared.now();
                let mut last = sim.next_outgoing_sequence();
                sim.send_teleport_start(refusal.flags, now)?;
                for line in &refusal.progress {
                    last = sim.next_outgoing_sequence();
                    sim.send_teleport_progress(line, refusal.flags, now)?;
                }
                Ok::<_, sl_proto::Error>(Some(last))
            })
            .await
    } else {
        Ok(None)
    };
    let result = match narrated {
        Ok(last) => {
            if let Some(last) = last
                && !wait_for_start_ack(shared, last).await
            {
                tracing::debug!("a refused teleport's start went unacknowledged");
            }
            shared
                .with_sim(|sim| {
                    send_failure(
                        sim,
                        policy,
                        &refusal.reason,
                        refusal.alert.as_ref(),
                        shared.now(),
                    )
                })
                .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        tracing::warn!("reporting a refused teleport failed: {error}");
    }
}
