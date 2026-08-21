//! The per-session I/O driver around the sans-I/O machines.
//!
//! One logged-in avatar is one [`SimSession`] + [`SimCaps`] pair behind a
//! single async mutex ([`SimState`]), pumped by two background tasks (a UDP
//! pump and a timer), with every mutation path funnelled through the same
//! flush sequence so queued transmits, [`ServerEvent`]s, timer deadlines and
//! event-queue wakeups can never be stranded inside the state machine.

use std::sync::Arc;
use std::time::Instant;

use sl_proto::{InMemoryAssetSource, RegionIdentity, ServerEvent, SimCaps, SimSession, Transmit};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, Notify, broadcast, watch};

use crate::scenario::{SimEventHook, SimHook};
use crate::udp_assets::{UdpAssetFixtures, answer_from_fixtures};
use crate::world::{AvatarIdentity, SceneFixtures, answer_world_request, push_arrival_world};

/// The lockable state of one logged-in session: the protocol machine, its
/// CAPS surface, and the pieces the driver's auto-behaviours need.
pub(crate) struct SimState {
    /// The sans-I/O simulator-side protocol machine.
    pub(crate) sim: SimSession,
    /// The CAPS dispatch surface granted to this session's seed.
    pub(crate) caps: SimCaps,
    /// The binary asset store served by the session-free asset caps.
    pub(crate) assets: InMemoryAssetSource,
    /// The region identity sent in the automatic `RegionHandshake` greeting
    /// (on `UseCircuitCode`, before the agent's movement completes).
    pub(crate) identity: RegionIdentity,
    /// Scenario hook run right after the arrival world burst when the agent
    /// completes its movement into the region.
    pub(crate) on_agent_arrived: Option<SimHook>,
    /// Scenario hook run for every drained event after the stock behaviour.
    pub(crate) on_event: Option<SimEventHook>,
    /// This session's copy of the legacy UDP asset fixtures (a terrain
    /// upload replaces only this session's heightmap).
    pub(crate) udp_assets: UdpAssetFixtures,
    /// The region's parcels and objects (pushed on arrival, replayed on
    /// request).
    pub(crate) world: SceneFixtures,
    /// Who the agent is, for its own avatar object.
    pub(crate) avatar: AvatarIdentity,
}

/// One live session's shared handle: the lockable state plus its I/O anchors
/// and wake-up channels. Cheap to clone; all clones drive the same session.
#[derive(Clone)]
pub(crate) struct SharedSim {
    /// The session state behind its single async lock.
    pub(crate) state: Arc<Mutex<SimState>>,
    /// This session's own loopback UDP socket (the port the login response
    /// advertised as `sim_port`).
    pub(crate) socket: Arc<UdpSocket>,
    /// Wakes a held `EventQueueGet` long-poll; a permit is stored, so a
    /// notification sent before the poll starts waiting is not lost.
    pub(crate) eq_notify: Arc<Notify>,
    /// Publishes the machine's next `poll_timeout` deadline to the timer task.
    pub(crate) timeout_tx: watch::Sender<Option<Instant>>,
    /// Broadcasts every drained [`ServerEvent`] to test/tool subscribers.
    pub(crate) events_tx: broadcast::Sender<ServerEvent>,
    /// Flipped to `true` when the grid shuts down; tasks exit on it.
    pub(crate) shutdown_rx: watch::Receiver<bool>,
}

/// What a locked flush gathered; applied after the state lock is released so
/// no socket I/O ever happens while the mutex is held.
#[must_use]
pub(crate) struct FlushOutcome {
    /// Datagrams the machine queued for the client.
    transmits: Vec<Transmit>,
    /// Whether CAPS events are queued (wake a held `EventQueueGet` poll).
    wake_event_queue: bool,
}

/// Broadcast-channel capacity for [`ServerEvent`] subscribers; a lagging
/// subscriber loses the oldest events rather than stalling the driver.
const EVENTS_CHANNEL_CAPACITY: usize = 256;

impl SharedSim {
    /// Runs `f` against the session machine, then flushes transmits, events,
    /// the timer deadline and the event-queue wakeup — the only sanctioned
    /// way for library users and tests to call `send_*` / `set_*` /
    /// `enqueue_*` on the live session.
    pub(crate) async fn with_sim<R>(&self, f: impl FnOnce(&mut SimSession) -> R) -> R {
        let mut guard = self.state.lock().await;
        let result = f(&mut guard.sim);
        let outcome = self.flush_locked(&mut guard);
        drop(guard);
        self.finish_flush(outcome).await;
        result
    }

    /// The under-the-lock half of the flush rule: run the auto-behaviours
    /// (arrival handshake + world burst, the UDP asset and world fixtures,
    /// the scenario's event hook), drain queued [`ServerEvent`]s into the broadcast, collect
    /// queued transmits, publish the next timer deadline, and note whether a
    /// held event-queue poll should be woken.
    ///
    /// Answering an event may queue further events (a served task inventory
    /// surfaces its own `XferRequested`); the loop keeps draining until the
    /// machine is quiet.
    pub(crate) fn flush_locked(&self, state: &mut SimState) -> FlushOutcome {
        while let Some(event) = state.sim.poll_event() {
            let now = Instant::now();
            // The greeting goes out as soon as the circuit is open, as a real
            // simulator does on `UseCircuitCode`: the viewer waits for the
            // `RegionHandshake` before it sends `CompleteAgentMovement`, and
            // the client discards a handshake that arrives after its
            // `AgentMovementComplete` already completed the arrival.
            if matches!(event, ServerEvent::CircuitOpened { .. })
                && let Err(error) = state.sim.send_region_handshake(&state.identity, now)
            {
                tracing::warn!("auto region handshake failed: {error}");
            }
            if matches!(event, ServerEvent::AgentArrived) {
                // A voice-enabled region tells the arriving viewer which
                // backend to load (`RequiredVoiceVersion` over the event
                // queue, as the simulator does on region entry).
                if let Some(voice_server_type) = state.sim.voice().advertised_server_type() {
                    state
                        .sim
                        .enqueue_required_voice_version(&sl_proto::RequiredVoiceVersion {
                            major_version: 1,
                            region_name: sl_wire::region_name_to_wire(
                                state.identity.sim_name.as_ref(),
                            ),
                            voice_server_type: Some(voice_server_type.to_owned()),
                        });
                }
                // The world burst a simulator sends on region entry: the
                // agent's own avatar, the parcel overlay, its parcel, and
                // every object in view — before the scenario's own hook.
                push_arrival_world(&state.world, &state.avatar, &mut state.sim, now);
                if let Some(hook) = &state.on_agent_arrived {
                    hook(&mut state.sim);
                }
            }
            answer_from_fixtures(
                &mut state.udp_assets,
                &mut state.sim,
                state.identity.region_id,
                &event,
                now,
            );
            answer_world_request(&state.world, &mut state.sim, &event, now);
            if let Some(hook) = &state.on_event {
                hook(&mut state.sim, &event);
            }
            // Only lagging subscribers error; the driver never stalls on them.
            drop(self.events_tx.send(event));
        }
        let mut transmits = Vec::new();
        while let Some(transmit) = state.sim.poll_transmit() {
            transmits.push(transmit);
        }
        self.timeout_tx.send_replace(state.sim.poll_timeout());
        FlushOutcome {
            transmits,
            wake_event_queue: state.sim.has_caps_events(),
        }
    }

    /// The after-the-lock half of the flush rule: put the collected
    /// datagrams on the wire and wake a held event-queue poll if events are
    /// waiting.
    pub(crate) async fn finish_flush(&self, outcome: FlushOutcome) {
        for transmit in outcome.transmits {
            if let Err(error) = self
                .socket
                .send_to(&transmit.payload, transmit.destination)
                .await
            {
                tracing::warn!(
                    "sending {} bytes to {} failed: {error}",
                    transmit.payload.len(),
                    transmit.destination
                );
            }
        }
        if outcome.wake_event_queue {
            self.eq_notify.notify_one();
        }
    }

    /// A fresh subscription to this session's [`ServerEvent`] broadcast.
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<ServerEvent> {
        self.events_tx.subscribe()
    }
}

/// Builds the wake-up channels for a new session around its already-bound
/// socket and initial state, returning the shared handle.
pub(crate) fn new_shared_sim(
    state: SimState,
    socket: Arc<UdpSocket>,
    shutdown_rx: watch::Receiver<bool>,
) -> SharedSim {
    let (events_tx, _) = broadcast::channel(EVENTS_CHANNEL_CAPACITY);
    let (timeout_tx, _) = watch::channel(state.sim.poll_timeout());
    SharedSim {
        state: Arc::new(Mutex::new(state)),
        socket,
        eq_notify: Arc::new(Notify::new()),
        timeout_tx,
        events_tx,
        shutdown_rx,
    }
}

/// Receive buffer size for the UDP pump; comfortably above the LLUDP MTU.
const RECV_BUFFER_BYTES: usize = 64 * 1024;

/// The UDP pump: receive datagrams into the machine and flush, until the
/// grid shuts down or the session closes.
pub(crate) async fn run_udp_pump(shared: SharedSim) {
    let mut shutdown_rx = shared.shutdown_rx.clone();
    let mut buffer = vec![0_u8; RECV_BUFFER_BYTES];
    loop {
        tokio::select! {
            received = shared.socket.recv_from(&mut buffer) => {
                let (length, from) = match received {
                    Ok(pair) => pair,
                    Err(error) => {
                        tracing::warn!("UDP receive failed: {error}");
                        continue;
                    }
                };
                let datagram = buffer.get(..length).unwrap_or_default();
                let closed = {
                    let mut guard = shared.state.lock().await;
                    if let Err(error) = guard.sim.handle_datagram(from, datagram, Instant::now()) {
                        tracing::debug!("datagram from {from} rejected: {error}");
                    }
                    let outcome = shared.flush_locked(&mut guard);
                    let closed = guard.sim.is_closed();
                    drop(guard);
                    shared.finish_flush(outcome).await;
                    closed
                };
                if closed {
                    tracing::info!("session closed; UDP pump exiting");
                    break;
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
}

/// Sleeps until `deadline`, or forever when the machine has no timer armed.
async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(instant) => tokio::time::sleep_until(tokio::time::Instant::from_std(instant)).await,
        None => std::future::pending().await,
    }
}

/// The timer task: drives `handle_timeout` at the machine's own
/// `poll_timeout` deadlines (ack flushes, resends, pings, the inactivity
/// timeout), re-arming from the watch channel the flush rule publishes to.
pub(crate) async fn run_timer(shared: SharedSim) {
    let mut shutdown_rx = shared.shutdown_rx.clone();
    let mut timeout_rx = shared.timeout_tx.subscribe();
    loop {
        let deadline = *timeout_rx.borrow_and_update();
        tokio::select! {
            () = sleep_until_opt(deadline) => {
                let closed = {
                    let mut guard = shared.state.lock().await;
                    guard.sim.handle_timeout(Instant::now());
                    let outcome = shared.flush_locked(&mut guard);
                    let closed = guard.sim.is_closed();
                    drop(guard);
                    shared.finish_flush(outcome).await;
                    closed
                };
                if closed {
                    tracing::info!("session closed; timer task exiting");
                    break;
                }
            }
            changed = timeout_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
}
