#![doc = include_str!("../README.md")]

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

use bevy::prelude::*;
use reqwest::blocking::Client as ReqwestBlockingClient;

use sl_proto::{
    Event as SessionEvent, Llsd, LoginResponse, Session, build_event_queue_request,
    build_seed_request, parse_event_queue_response, parse_login_response, parse_seed_response,
};

// Re-export the core types a consumer needs to configure the plugin, drive the
// survey commands, and read events. `Event` is aliased to avoid clashing with
// Bevy's `Event` derive.
pub use sl_proto::{
    AnyMessage, ChatAudible, ChatMessage, ChatSourceType, ChatType, ControlFlags, DisconnectReason,
    ImDialog, InstantMessage, LoginParams, LoginRequest, MapRegionInfo, Maturity, MfaChallenge,
    NeighborInfo, ParcelFlags, ParcelInfo, ParcelOverlayInfo, ProductType, RegionFlags,
    RegionIdentity, RegionLimits, Reliability, Rotation, Transmit, Uuid, Vector, grid_to_handle,
    handle_to_global, handle_to_grid, sim_access,
};
pub use sl_proto::{DisconnectReason as SessionDisconnectReason, Event as SlSessionEvent};

/// The maximum UDP datagram size we are prepared to receive.
const RECV_BUFFER_SIZE: usize = 0x1_0000;

/// How long to wait for a single CAPS event-queue long-poll before retrying.
const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

/// The Bevy plugin that drives a sans-I/O [`Session`] from ECS systems.
#[derive(Debug, Clone)]
pub struct SlClientPlugin {
    /// The login parameters used to start the session.
    pub params: LoginParams,
}

impl Plugin for SlClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SlEvent>()
            .add_event::<SlMfaChallenge>()
            .add_event::<SlCommand>()
            .insert_resource(SlConfig {
                params: self.params.clone(),
            })
            .add_systems(Startup, start_login)
            .add_systems(Update, drive);
    }
}

/// A high-level session event, emitted as a Bevy event.
#[derive(Event, Debug, Clone)]
pub struct SlEvent(pub SessionEvent);

/// Emitted when the grid requires a multi-factor one-time code. To answer it,
/// re-add the plugin with login parameters prepared via
/// `LoginRequest::with_mfa`.
#[derive(Event, Debug, Clone)]
pub struct SlMfaChallenge(pub MfaChallenge);

/// A command to a running session, sent as a Bevy event.
#[derive(Event, Debug)]
pub enum SlCommand {
    /// Send an application message.
    Send {
        /// The message to send.
        message: Box<AnyMessage>,
        /// How to deliver it.
        reliability: Reliability,
    },
    /// Send local chat via `ChatFromViewer`. Incoming chat arrives as an
    /// [`SlSessionEvent::ChatReceived`].
    Chat {
        /// The message text.
        message: String,
        /// The chat type (whisper / normal / shout / …).
        chat_type: ChatType,
        /// The chat channel (`0` for ordinary local chat).
        channel: i32,
    },
    /// Broadcast a local-chat typing indicator (`true` = start, `false` = stop).
    /// Other clients see it as an [`SlSessionEvent::ChatTyping`].
    Typing(bool),
    /// Send a direct (1:1) instant message. Incoming IMs arrive as an
    /// [`SlSessionEvent::InstantMessageReceived`].
    InstantMessage {
        /// The recipient's agent id.
        to_agent_id: Uuid,
        /// The message text.
        message: String,
    },
    /// Send an instant-message typing indicator to `to_agent_id` (`true` = start,
    /// `false` = stop). Other clients see it as an [`SlSessionEvent::ImTyping`].
    ImTyping {
        /// The correspondent's agent id.
        to_agent_id: Uuid,
        /// Whether typing started (`true`) or stopped (`false`).
        typing: bool,
    },
    /// Set the agent control flags (movement); the simulator moves the agent
    /// accordingly. Pass [`ControlFlags::empty`] to stop.
    SetControls(ControlFlags),
    /// Set the agent's body and head rotation (facing/steering).
    SetRotation {
        /// The body rotation.
        body: Rotation,
        /// The head rotation.
        head: Rotation,
    },
    /// Stand the agent up (from sitting).
    Stand,
    /// Sit the agent on the ground where it stands.
    SitOnGround,
    /// Sit the agent on the object `target` at the region-local `offset`. The
    /// result arrives as an [`SlSessionEvent::SitResult`].
    Sit {
        /// The object to sit on.
        target: Uuid,
        /// The seat offset, in region-local metres.
        offset: Vector,
    },
    /// Walk the agent to the global coordinates `(global_x, global_y, z)` using
    /// the simulator's server-side autopilot.
    Autopilot {
        /// The global X coordinate, in metres.
        global_x: f64,
        /// The global Y coordinate, in metres.
        global_y: f64,
        /// The region-local height, in metres.
        z: f64,
    },
    /// Teleport to `position` (region-local) in the region `region_handle`.
    Teleport {
        /// The destination region handle.
        region_handle: u64,
        /// The destination position within the region.
        position: Vector,
        /// The look-at direction on arrival.
        look_at: Vector,
    },
    /// Request the current region's info (agent/object limits).
    RequestRegionInfo,
    /// Request `ParcelProperties` for a metre rectangle (region-local).
    RequestParcelProperties {
        /// The western edge (metres).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
        /// A sequence id echoed back in the reply for matching.
        sequence_id: i32,
    },
    /// Set the draw distance advertised in keep-alive `AgentUpdate`s.
    SetDrawDistance(f32),
    /// Request world-map blocks for a grid-coordinate rectangle (region
    /// indices); each region arrives as an [`SlSessionEvent::MapBlock`].
    RequestMapBlocks {
        /// Minimum grid x (inclusive).
        min_x: u32,
        /// Maximum grid x (inclusive).
        max_x: u32,
        /// Minimum grid y (inclusive).
        min_y: u32,
        /// Maximum grid y (inclusive).
        max_y: u32,
    },
    /// Begin a clean logout.
    Logout,
}

/// The plugin configuration resource.
#[derive(Resource, Debug)]
struct SlConfig {
    /// The login parameters.
    params: LoginParams,
}

/// The driver's runtime state resource.
#[derive(Resource)]
struct SlState {
    /// The current phase of the driver.
    inner: SlInner,
}

/// The driver phases.
enum SlInner {
    /// Awaiting the result of the (threaded, blocking) XML-RPC login.
    LoggingIn {
        /// The session whose circuit will be bootstrapped on success.
        session: Box<Session>,
        /// Receives the login response body (or an error string).
        rx: Receiver<Result<String, String>>,
    },
    /// The circuit is up; pumping the socket each frame.
    Running {
        /// The driven session.
        session: Box<Session>,
        /// The non-blocking UDP socket.
        socket: UdpSocket,
        /// A reusable receive buffer.
        recv_buf: Vec<u8>,
        /// The CAPS event-queue poller for the current region, if a seed
        /// capability is known. Restarted on each region change.
        caps: Option<CapsPoller>,
    },
    /// The session is finished.
    Done,
}

/// A handle to the background CAPS event-queue long-poll thread for one region.
///
/// The thread fetches the region's capability map, then long-polls
/// `EventQueueGet`, forwarding each decoded event over `rx`. Dropping the poller
/// signals the thread to stop after its in-flight request returns.
struct CapsPoller {
    /// Receives `(message, body)` pairs decoded from the event queue.
    rx: Receiver<(String, Llsd)>,
    /// Set on drop to ask the thread to stop at its next loop iteration.
    stop: Arc<AtomicBool>,
}

impl Drop for CapsPoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Startup system: builds the session and spawns the blocking login thread.
fn start_login(mut commands: Commands, config: Res<SlConfig>) {
    let session = Session::new(config.params.clone());
    let inner = match session.login_http_request() {
        Some(request) => {
            let (tx, rx) = unbounded();
            std::thread::spawn(move || {
                tx.send(perform_login(
                    &request.url,
                    &request.user_agent,
                    request.body,
                ))
                .ok();
            });
            SlInner::LoggingIn {
                session: Box::new(session),
                rx,
            }
        }
        None => SlInner::Done,
    };
    commands.insert_resource(SlState { inner });
}

/// Performs the blocking XML-RPC login POST, returning the response body.
fn perform_login(url: &str, user_agent: &str, body: String) -> Result<String, String> {
    ReqwestBlockingClient::new()
        .post(url)
        .header("Content-Type", "text/xml")
        .header("User-Agent", user_agent)
        .body(body)
        .send()
        .and_then(reqwest::blocking::Response::text)
        .map_err(|error| error.to_string())
}

/// Update system: advances the session each frame.
fn drive(
    mut state: ResMut<SlState>,
    mut events: EventWriter<SlEvent>,
    mut mfa: EventWriter<SlMfaChallenge>,
    mut commands: EventReader<SlCommand>,
) {
    let now = Instant::now();
    let inner = std::mem::replace(&mut state.inner, SlInner::Done);
    state.inner = match inner {
        SlInner::LoggingIn { session, rx } => {
            advance_login(session, rx, now, &mut events, &mut mfa)
        }
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
        } => advance_running(
            session,
            socket,
            recv_buf,
            caps,
            now,
            &mut events,
            &mut commands,
        ),
        SlInner::Done => SlInner::Done,
    };
}

/// Handles the logging-in phase, transitioning to `Running` once the login
/// response arrives.
fn advance_login(
    mut session: Box<Session>,
    rx: Receiver<Result<String, String>>,
    now: Instant,
    events: &mut EventWriter<SlEvent>,
    mfa: &mut EventWriter<SlMfaChallenge>,
) -> SlInner {
    match rx.try_recv() {
        Ok(Ok(body)) => match parse_login_response(&body) {
            Ok(LoginResponse::Success(success)) => {
                if session
                    .handle_login_response(LoginResponse::Success(success), now)
                    .is_err()
                {
                    emit_disconnect(events, DisconnectReason::ProtocolError);
                    return SlInner::Done;
                }
                match bind_socket() {
                    Ok(socket) => {
                        let caps = start_caps_poller(&session);
                        SlInner::Running {
                            session,
                            socket,
                            recv_buf: vec![0u8; RECV_BUFFER_SIZE],
                            caps,
                        }
                    }
                    Err(()) => {
                        emit_disconnect(events, DisconnectReason::ProtocolError);
                        SlInner::Done
                    }
                }
            }
            Ok(LoginResponse::MfaChallenge(challenge)) => {
                mfa.write(SlMfaChallenge(challenge));
                SlInner::Done
            }
            Ok(LoginResponse::Failure(failure)) => {
                emit_disconnect(
                    events,
                    DisconnectReason::LoginFailed {
                        reason: failure.reason,
                        message: failure.message,
                    },
                );
                SlInner::Done
            }
            Err(_parse) => {
                emit_disconnect(events, DisconnectReason::ProtocolError);
                SlInner::Done
            }
        },
        Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
            emit_disconnect(events, DisconnectReason::ProtocolError);
            SlInner::Done
        }
        Err(TryRecvError::Empty) => SlInner::LoggingIn { session, rx },
    }
}

/// Binds a non-blocking UDP socket on an ephemeral port.
fn bind_socket() -> Result<UdpSocket, ()> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_ignored| ())?;
    socket.set_nonblocking(true).map_err(|_ignored| ())?;
    Ok(socket)
}

/// Handles the running phase: receive UDP and CAPS events, apply commands, time
/// out, transmit, and surface events.
fn advance_running(
    mut session: Box<Session>,
    socket: UdpSocket,
    mut recv_buf: Vec<u8>,
    mut caps: Option<CapsPoller>,
    now: Instant,
    events: &mut EventWriter<SlEvent>,
    commands: &mut EventReader<SlCommand>,
) -> SlInner {
    // Drain all available inbound datagrams.
    loop {
        match socket.recv_from(&mut recv_buf) {
            Ok((len, from)) => {
                if let Some(datagram) = recv_buf.get(..len) {
                    session.handle_datagram(from, datagram, now).ok();
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(_other) => break,
        }
    }

    // Drain any CAPS event-queue events (ParcelProperties, TeleportFinish, ...).
    if let Some(poller) = caps.as_ref() {
        while let Ok((message, body)) = poller.rx.try_recv() {
            session.handle_caps_event(&message, &body, now).ok();
        }
    }

    // Apply queued commands.
    for command in commands.read() {
        match command {
            SlCommand::Send {
                message,
                reliability,
            } => {
                session.enqueue((**message).clone(), *reliability, now).ok();
            }
            SlCommand::Chat {
                message,
                chat_type,
                channel,
            } => {
                session.say(message, *chat_type, *channel, now).ok();
            }
            SlCommand::Typing(typing) => {
                session.set_typing(*typing, now).ok();
            }
            SlCommand::InstantMessage {
                to_agent_id,
                message,
            } => {
                session
                    .send_instant_message(*to_agent_id, message, now)
                    .ok();
            }
            SlCommand::ImTyping {
                to_agent_id,
                typing,
            } => {
                session.send_im_typing(*to_agent_id, *typing, now).ok();
            }
            SlCommand::SetControls(controls) => {
                session.set_controls(*controls, now).ok();
            }
            SlCommand::SetRotation { body, head } => {
                session.set_rotation(body.clone(), head.clone(), now).ok();
            }
            SlCommand::Stand => {
                session.stand(now).ok();
            }
            SlCommand::SitOnGround => {
                session.sit_on_ground(now).ok();
            }
            SlCommand::Sit { target, offset } => {
                session.sit_on(*target, offset.clone(), now).ok();
            }
            SlCommand::Autopilot {
                global_x,
                global_y,
                z,
            } => {
                session.autopilot_to(*global_x, *global_y, *z, now).ok();
            }
            SlCommand::Teleport {
                region_handle,
                position,
                look_at,
            } => {
                session
                    .teleport_to(*region_handle, position.clone(), look_at.clone(), now)
                    .ok();
            }
            SlCommand::RequestRegionInfo => {
                session.request_region_info(now).ok();
            }
            SlCommand::RequestParcelProperties {
                west,
                south,
                east,
                north,
                sequence_id,
            } => {
                session
                    .request_parcel_properties(*west, *south, *east, *north, *sequence_id, now)
                    .ok();
            }
            SlCommand::SetDrawDistance(far) => session.set_draw_distance(*far),
            SlCommand::RequestMapBlocks {
                min_x,
                max_x,
                min_y,
                max_y,
            } => {
                session
                    .request_map_blocks(*min_x, *max_x, *min_y, *max_y, now)
                    .ok();
            }
            SlCommand::Logout => session.initiate_logout(now),
        }
    }

    // Fire timers that are due.
    if session
        .poll_timeout()
        .is_some_and(|deadline| now >= deadline)
    {
        session.handle_timeout(now);
    }

    // Flush outgoing datagrams.
    while let Some(transmit) = session.poll_transmit() {
        socket.send_to(&transmit.payload, transmit.destination).ok();
    }

    // Surface events. A region change brings a new seed capability, so restart
    // the event-queue poller against the new region (dropping the old poller
    // signals its thread to stop).
    let mut done = false;
    let mut region_changed = false;
    while let Some(event) = session.poll_event() {
        match &event {
            SessionEvent::Disconnected(_) | SessionEvent::LoggedOut => done = true,
            SessionEvent::RegionChanged { .. } => region_changed = true,
            _ => {}
        }
        events.write(SlEvent(event));
    }
    if region_changed {
        caps = start_caps_poller(&session);
    }

    if done || session.is_closed() {
        SlInner::Done
    } else {
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
        }
    }
}

/// Starts the CAPS event-queue poll thread for the session's current seed
/// capability, returning its handle, or `None` if no seed is known yet.
fn start_caps_poller(session: &Session) -> Option<CapsPoller> {
    let seed = session.seed_capability()?.to_owned();
    let (tx, rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    std::thread::spawn(move || run_event_queue(seed, &tx, &thread_stop));
    Some(CapsPoller { rx, stop })
}

/// Fetches the capability map from `seed_url`, then long-polls its
/// `EventQueueGet` capability on a background thread, forwarding each decoded
/// event to `caps_tx` until `stop` is set, the receiver is dropped (e.g. on
/// region change), or a request fails fatally.
fn run_event_queue(seed_url: String, caps_tx: &Sender<(String, Llsd)>, stop: &AtomicBool) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let seed_body = build_seed_request(&["EventQueueGet"]);
    let Ok(response) = http
        .post(&seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(seed_body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    let Ok(capabilities) = parse_seed_response(&text) else {
        return;
    };
    let Some(event_queue_url) = capabilities.get("EventQueueGet").cloned() else {
        return;
    };

    let mut ack: Option<i32> = None;
    while !stop.load(Ordering::Relaxed) {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
        {
            Ok(response) => response,
            Err(_error) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let Ok(text) = response.text() else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).is_err() {
                return;
            }
        }
    }
}

/// Emits a disconnect event.
fn emit_disconnect(events: &mut EventWriter<SlEvent>, reason: DisconnectReason) {
    events.write(SlEvent(SessionEvent::Disconnected(reason)));
}
