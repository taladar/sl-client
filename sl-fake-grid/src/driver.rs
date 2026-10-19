//! The per-session I/O driver around the sans-I/O machines.
//!
//! One logged-in avatar is one [`SimSession`] + [`SimCaps`] pair behind a
//! single async mutex ([`SimState`]), pumped by two background tasks (a UDP
//! pump and a timer), with every mutation path funnelled through the same
//! flush sequence so queued transmits, [`ServerEvent`]s, timer deadlines and
//! event-queue wakeups can never be stranded inside the state machine.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sl_proto::{RegionIdentity, ServerEvent, SimCaps, SimSession, Transmit};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, Notify, broadcast, mpsc, watch};

use crate::assets::GridAssets;
use crate::runtime::{SessionIds, SessionRole};
use crate::scenario::{SimEventHook, SimHook};
use crate::terrain::TerrainFixture;
use crate::time::Now;
use crate::udp_assets::{TerrainTerms, UdpAssetFixtures, answer_from_fixtures};
use crate::world::{
    AvatarIdentity, Ground, RegionChange, RegionUpdate, RegionWorld, answer_world_request,
    push_arrival_world, push_child_world, push_own_appearance, push_own_appearance_baked,
};

/// The lockable state of one logged-in session: the protocol machine, its
/// CAPS surface, and the pieces the driver's auto-behaviours need.
pub(crate) struct SimState {
    /// The sans-I/O simulator-side protocol machine.
    pub(crate) sim: SimSession,
    /// The CAPS dispatch surface granted to this session's seed.
    pub(crate) caps: SimCaps,
    /// The **grid-wide** binary asset store served by the session-free asset
    /// caps ([`crate::assets`]). Every session shares one; an asset id names
    /// a blob the whole grid knows, not one region's.
    pub(crate) assets: GridAssets,
    /// Which live grid this one imitates for a taken object's asset
    /// ([`crate::assets::ObjectAssetPolicy`]). Grid-wide, like the store it
    /// decides between.
    pub(crate) object_assets: crate::assets::ObjectAssetPolicy,
    /// How the region answers the About Land traffic where the live grids
    /// disagree ([`crate::ImitatedGrid::parcel_policy`]).
    pub(crate) parcel_policy: crate::imitates::ParcelPolicy,
    /// How the region answers a sit request where the live grids disagree
    /// ([`crate::ImitatedGrid::sit_policy`]).
    pub(crate) sit_policy: crate::imitates::SitPolicy,
    /// How the region answers a viewer's animations where the live grids
    /// disagree ([`crate::ImitatedGrid::animation_policy`]).
    pub(crate) animation_policy: crate::animations::AnimationPolicy,
    /// The agent's animation set ([`crate::animations`]).
    pub(crate) animations: crate::animations::AnimationSet,
    /// How the region answers the estate where the live grids disagree
    /// ([`crate::ImitatedGrid::estate_policy`]).
    pub(crate) estate_policy: crate::imitates::EstatePolicy,
    /// How the grid answers the world map where the live grids disagree
    /// ([`crate::ImitatedGrid::map_policy`]).
    pub(crate) map_policy: crate::imitates::MapPolicy,
    /// How this grid announces an inventory item it just created — the legacy
    /// UDP message or an event-queue `BulkUpdateInventory`
    /// ([`crate::InventoryAnnouncement`]). Grid-wide, because it is a property
    /// of which live grid this one is being.
    pub(crate) inventory_announcement: crate::inventory::InventoryAnnouncement,
    /// How this grid announces an item a **capability upload** created or
    /// rewrote ([`crate::UploadAnnouncements`]) — the legacy UDP message, or
    /// nothing at all, answered once per upload path. Grid-wide for the same
    /// reason, and a separate knob because the two live grids take opposite
    /// sides of the save and of the take.
    pub(crate) upload_announcements: crate::inventory::UploadAnnouncements,
    /// Who composites this grid's avatars ([`crate::BakePolicy`]). Grid-wide,
    /// and what decides whether an `AvatarAppearance` this session pushes
    /// carries an `AppearanceData` block at all.
    pub(crate) bakes: crate::bakes::BakePolicy,
    /// The region identity sent in the automatic `RegionHandshake` greeting
    /// (on `UseCircuitCode`, before the agent's movement completes).
    pub(crate) identity: RegionIdentity,
    /// What the region says of itself on arrival where the live grids
    /// disagree ([`crate::ImitatedGrid::arrival_policy`]).
    pub(crate) arrival: crate::imitates::ArrivalPolicy,
    /// Whether the agent arrived before its `RegionHandshakeReply` did, so
    /// that whether it is sent its own appearance is still to be decided
    /// ([`crate::BakePolicy::sends_own_appearance`]).
    pub(crate) own_appearance_owed: bool,
    /// What the agent published with `AgentSetAppearance`, where the grid
    /// keeps it ([`crate::bakes::PublishedBakes`]).
    pub(crate) published_bakes: crate::bakes::PublishedBakes,
    /// What the session knows of its agent's groups: the active one an
    /// `AgentDataUpdate` names, and the powers a deed and a move of a
    /// group's object are checked against ([`crate::groups`]).
    pub(crate) groups: crate::groups::AgentGroups,
    /// How the grid runs groups ([`ImitatedGrid::groups_policy`](crate::ImitatedGrid::groups_policy)).
    pub(crate) groups_policy: crate::groups::GroupsPolicy,
    /// How the region's ground and wind are sent
    /// ([`crate::ImitatedGrid::terrain_policy`]).
    pub(crate) terrain_policy: crate::imitates::TerrainPolicy,
    /// Scenario hook run right after the arrival world burst when the agent
    /// completes its movement into the region.
    pub(crate) on_agent_arrived: Option<SimHook>,
    /// Scenario hook run for every drained event after the stock behaviour.
    pub(crate) on_event: Option<SimEventHook>,
    /// This session's copy of the legacy UDP asset fixtures (a terrain
    /// upload replaces only this session's heightmap).
    pub(crate) udp_assets: UdpAssetFixtures,
    /// The region's ground, streamed as `LayerData` in the arrival burst.
    pub(crate) terrain: TerrainFixture,
    /// The **region's** parcels and objects (pushed on arrival, replayed on
    /// request), shared with every other session in the same region.
    ///
    /// Shared rather than cloned per session because a rez is a change to the
    /// region, not to one circuit's view of it: an object one avatar rezzes is
    /// an object the next one to arrive is shown. A session in a *different*
    /// region holds a different store, which is what a handover needs — the
    /// region an object left and the region it arrived in disagree for a
    /// moment by design.
    pub(crate) world: RegionWorld,
    /// Publishes every change this session makes to [`world`](Self::world), so
    /// the region's other sessions can stream it
    /// ([`run_region_watcher`]). Shared with the region, like the store.
    pub(crate) changes: broadcast::Sender<RegionUpdate>,
    /// The grid's identifier minter, for the ids a write path chooses: a
    /// rezzed object's key, a taken object's inventory item. Seeded on a
    /// deterministic grid, so the same run mints the same ids.
    pub(crate) minter: crate::runtime::IdMinter,
    /// The grid's price list and this region's object budget, answered to an
    /// `EconomyDataRequest`. Grid-wide, like the helper policy it comes from.
    pub(crate) economy: sl_proto::EconomyData,
    /// What this session's agent is permitted, and how the deprecated paths
    /// are answered ([`crate::agent_requests`]).
    pub(crate) policy: crate::agent_requests::AgentPolicy,
    /// How the grid answers a request for god powers, and this session's god
    /// state ([`crate::god_powers`]).
    pub(crate) god_powers: crate::god_powers::PowersAnswer,
    /// This session's god rights and the level its viewer was last told.
    pub(crate) god_mode: crate::god_powers::GodMode,
    /// How the grid words its `UntrustedSimulatorMessage` answers.
    pub(crate) untrusted_style: sl_proto::UntrustedMessageStyle,
    /// Every account's home, which a Set-Home of this session's agent writes.
    pub(crate) homes: crate::agent_requests::Homes,
    /// Who may set home where, what the grid says about it and what its
    /// landmarks hold ([`crate::ImitatedGrid::home_policy`]).
    pub(crate) home_policy: crate::imitates::HomePolicy,
    /// The last thing this session's agent was told about its home, and when
    /// ([`crate::imitates::HomePolicy::repeat_quiet`]).
    pub(crate) home_told: Option<(std::time::Instant, crate::imitates::HomeNotice)>,
    /// Who the agent is, for its own avatar object.
    pub(crate) avatar: AvatarIdentity,
    /// What this client holds **selected**, which is the subscription an
    /// object's properties are pushed to: a select opens it, a deselect closes
    /// it, and [`run_region_watcher`] reads it to decide whether a properties
    /// change another session published is any of this client's business.
    ///
    /// Per session rather than per region, because that is what it is — two
    /// residents may hold the same prim selected at once, and neither of them
    /// is thereby holding a lock on it.
    pub(crate) selection: crate::object_edits::Selection,
    /// When the grid handled this session's arrival: the instant its
    /// timeline's steps are timed from ([`crate::timeline`]). Stamped where
    /// the arrival is handled, not where a task that heard of it wakes up —
    /// two tasks woken by one event read two different clocks.
    pub(crate) arrived_at: Option<Instant>,
    /// Where this session's viewer last said its camera is, region-local
    /// metres; [`None`] until an `AgentUpdate` says. OpenSim sends a gaze only
    /// to a camera near the avatar gazing ([`crate::relays`]).
    pub(crate) camera_at: Option<sl_types::lsl::Vector>,
    /// This session's sequence number (its CAPS path component and the key
    /// in the grid's session table).
    pub(crate) seq: u64,
    /// The index of the region this session lives in (into the grid's region
    /// table) — the region handle is fixed at construction, so a teleport is
    /// always a second session.
    pub(crate) region: usize,
    /// The login-minted session identity this circuit shares with every
    /// other circuit of the same login (a teleport destination reuses it: the
    /// client opens every circuit with its login `UseCircuitCode` triple).
    pub(crate) ids: SessionIds,
    /// What this session was opened as, which decides the burst its circuit
    /// gets on `CircuitOpened` (see [`SessionRole`]).
    pub(crate) role: SessionRole,
    /// This session's seed capability URL — what an announcement of it hands
    /// the client (`EstablishAgentCommunication`, `CrossedRegion`).
    pub(crate) seed_url: url::Url,
    /// This session's loopback UDP address, announced alongside the seed.
    pub(crate) udp_addr: std::net::SocketAddr,
    /// The grid's whole region catalogue, as the world map reports it.
    ///
    /// The one piece of state a session holds that is not about its own
    /// region: a viewer asks whichever simulator it is on about every region
    /// on the grid ([`crate::world_map`]). Shared, because it is the same
    /// answer for every session and never changes after start-up.
    pub(crate) map: Arc<[sl_proto::MapRegionInfo]>,
    /// The scripted steps this session runs once its agent has arrived
    /// ([`crate::timeline`]), starting as the region scenario's own.
    ///
    /// Owned per session rather than shared with the region, because a script
    /// belongs to the avatar running it: a teleport or a crossing moves the
    /// steps that have not run yet onto the destination's session
    /// ([`crate::timeline::hand_over`]), leaving the prefix behind here.
    pub(crate) timeline: Vec<crate::timeline::Step>,
    /// The index into [`timeline`](Self::timeline) of the next step to run.
    /// A cursor at the end of the steps is a script that is over — which is
    /// what a hand-over leaves behind.
    pub(crate) timeline_cursor: usize,
    /// Bumped every time a hand-over replaces [`timeline`](Self::timeline).
    ///
    /// Without it a runner that started waiting for step *n* of one script
    /// could execute step *n* of the script that replaced it — the two indices
    /// are the same number and name different steps. The runner reads this
    /// alongside the cursor and treats a change as "look again".
    pub(crate) timeline_generation: u64,
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
    /// Flipped to `true` by the flush rule once the machine is closed
    /// (logout, inactivity, retirement, abandonment), so a pump blocked in
    /// `recv_from` or a timer with no deadline still exits.
    pub(crate) closed_tx: watch::Sender<bool>,
    /// The grid's clock: every instant this session stamps its machine with.
    pub(crate) clock: Now,
    /// Wakes this session's parked timeline runner when a teleport or a
    /// crossing hands it a script ([`crate::timeline::hand_over`]). A permit is
    /// stored, so a hand-over that lands before the runner parks is not lost.
    pub(crate) timeline_notify: Arc<Notify>,
    /// How long a datagram spends on the way to or from the client
    /// ([`FakeGridBuilder::link_latency`](crate::FakeGridBuilder::link_latency)).
    /// Zero on a stock grid, where nothing is delayed.
    pub(crate) latency: Duration,
    /// Where an outbound datagram waits out [`Self::latency`] before it is
    /// sent; `None` when there is none to wait out.
    delayed_tx: Option<mpsc::UnboundedSender<(tokio::time::Instant, Transmit)>>,
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
    /// The grid's current instant — what every `send_*` / `handle_*` call on
    /// this session is stamped with.
    pub(crate) fn now(&self) -> Instant {
        (self.clock)()
    }

    /// Runs `f` against the session machine, then flushes transmits, events,
    /// the timer deadline and the event-queue wakeup — the only sanctioned
    /// way for library users and tests to call `send_*` / `set_*` /
    /// `enqueue_*` on the live session.
    pub(crate) async fn with_sim<R>(&self, f: impl FnOnce(&mut SimSession) -> R) -> R {
        self.with_state(|state| f(&mut state.sim)).await
    }

    /// [`with_sim`](Self::with_sim) for a caller that needs the session's
    /// **region world** alongside its machine, and has changes to publish.
    ///
    /// The same pairing [`FakeAgent::with_world`](crate::FakeAgent::with_world)
    /// offers, plus the half a scripted change owes the rest of the region: the
    /// [`RegionChange`]es the closure returns go onto the region's change
    /// stream while the region lock is still held, so a watcher that wakes on
    /// one cannot re-read a world the change has not landed in yet. The lock
    /// order is the crate's own — session lock, then region lock, never the
    /// reverse.
    pub(crate) async fn with_region<R>(
        &self,
        f: impl FnOnce(
            &mut crate::world::SceneFixtures,
            &mut SimSession,
            Instant,
        ) -> (R, Vec<RegionChange>),
    ) -> R {
        let mut guard = self.state.lock().await;
        let now = self.now();
        // The region lock lives in its own scope: it is a synchronous lock, and
        // the flush below awaits.
        let result = {
            let state = &mut *guard;
            let mut world = state.world.lock();
            let (result, changes) = f(&mut world, &mut state.sim, now);
            for change in changes {
                // A region whose other sessions have all gone has no
                // subscribers; that is not a failure.
                drop(state.changes.send(RegionUpdate {
                    source: state.seq,
                    change,
                }));
            }
            drop(world);
            result
        };
        let outcome = self.flush_locked(&mut guard);
        drop(guard);
        self.finish_flush(outcome).await;
        result
    }

    /// [`with_sim`](Self::with_sim) for the callers that need more of the
    /// session than its machine — the region watcher, which decides what to
    /// forward from this session's own selection and where its avatar stands.
    pub(crate) async fn with_state<R>(&self, f: impl FnOnce(&mut SimState) -> R) -> R {
        let mut guard = self.state.lock().await;
        let result = f(&mut guard);
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
            let now = self.now();
            if let ServerEvent::AgentUpdate(update) = &event {
                state.camera_at = Some(update.camera.center.clone());
            }
            // The greeting goes out as soon as the circuit is open, as a real
            // simulator does on `UseCircuitCode`: the viewer waits for the
            // `RegionHandshake` before it sends `CompleteAgentMovement`, and
            // the client discards a handshake that arrives after its
            // `AgentMovementComplete` already completed the arrival.
            // Who the agent is, as both live grids say it: first of all on the
            // circuit the avatar arrives on, and again whenever asked.
            let opened = matches!(event, ServerEvent::CircuitOpened { .. });
            if (opened && matches!(state.role, SessionRole::Root))
                || matches!(event, ServerEvent::RequestAgentDataUpdate)
            {
                let agent = state.groups.active_group(&state.avatar);
                if let Err(error) = state.sim.send_agent_data_update(&agent, now) {
                    tracing::warn!("agent data update failed: {error}");
                }
            }
            if opened {
                if let Err(error) = state.sim.send_region_handshake(&state.identity, now) {
                    tracing::warn!("auto region handshake failed: {error}");
                }
                // A neighbour's circuit never sees a `CompleteAgentMovement`,
                // so its scene has to go out now or never: a child agent is
                // exactly a circuit that streams a region the avatar is not
                // standing in.
                if matches!(state.role, SessionRole::Child) {
                    // Second Life greets a child circuit twice; the client
                    // has to take the second for what it is.
                    for _again in 1..state.arrival.child_handshakes {
                        if let Err(error) = state.sim.send_region_handshake(&state.identity, now) {
                            tracing::warn!("repeated region handshake failed: {error}");
                        }
                    }
                    push_child_world(
                        &state.world.lock(),
                        Ground {
                            fixture: &state.terrain,
                            policy: &state.terrain_policy,
                        },
                        &state.identity,
                        state.bakes,
                        &mut state.sim,
                        now,
                    );
                }
            }
            // A viewer may complete its movement before it has answered the
            // region's handshake, and its answer is what says whether it is
            // to be sent its own appearance: an arrival that could not tell
            // is settled here.
            if matches!(event, ServerEvent::RegionHandshakeReplied)
                && state.own_appearance_owed
                && state
                    .bakes
                    .sends_own_appearance(state.sim.handshake_reply_flags())
            {
                state.own_appearance_owed = false;
                push_own_appearance(
                    &state.avatar,
                    &state.assets,
                    state.bakes,
                    &mut state.sim,
                    now,
                );
            }
            if matches!(event, ServerEvent::AgentArrived) {
                state.arrived_at = Some(now);
                state.animations = crate::animations::AnimationSet::arriving();
                // Owed only where the arrival burst could not decide it: a
                // grid that sends the agent its own appearance whatever the
                // handshake says has already sent it with the burst.
                state.own_appearance_owed = state.sim.handshake_reply_flags().is_none()
                    && !state.bakes.sends_own_appearance(None);
                // What Second Life tells an arriving agent about itself and
                // OpenSim does not: its health, and — over the event queue —
                // what it may do to the region's navmesh.
                if state.arrival.health_message
                    && let Err(error) = state.sim.send_health_message(FULL_HEALTH, now)
                {
                    tracing::warn!("arrival health message failed: {error}");
                }
                if state.arrival.agent_state_update {
                    state.sim.enqueue_agent_state_update(true);
                }
                // A voice-enabled region tells the arriving viewer which
                // backend to load (`RequiredVoiceVersion` over the event
                // queue, as a Second Life simulator does on region entry).
                // A silent region -- a stock OpenSim one -- sends no such
                // push; the string appears nowhere in OpenSim's sources.
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
                // agent's own avatar, the parcel overlay, its parcel, the
                // ground, and every object in view — before the scenario's
                // own hook.
                push_arrival_world(
                    &state.world.lock(),
                    Ground {
                        fixture: &state.terrain,
                        policy: &state.terrain_policy,
                    },
                    &state.avatar,
                    &state.assets,
                    state.bakes,
                    &mut state.sim,
                    now,
                );
                crate::world::push_own_animation(
                    &state.avatar,
                    &state.animations,
                    state.animation_policy,
                    &mut state.sim,
                    now,
                );
                if let Some(hook) = &state.on_agent_arrived {
                    hook(&mut state.sim, now);
                }
            }
            let _baked = crate::bakes::answer(
                &mut state.published_bakes,
                state.bakes,
                &mut state.sim,
                &event,
                now,
            );
            // An accepted central bake is stated back to the agent as its
            // appearance at the version baked, as Second Life does.
            if let ServerEvent::ServerAppearanceRequested { cof_version } = &event {
                push_own_appearance_baked(
                    &state.avatar,
                    &state.assets,
                    state.bakes,
                    &mut state.sim,
                    Some(*cof_version),
                    now,
                );
            }
            let _animated = crate::animations::answer(
                &mut state.animations,
                state.animation_policy,
                state.avatar.agent_id,
                &mut state.sim,
                &event,
                now,
            );
            answer_from_fixtures(
                &mut state.udp_assets,
                &state.assets,
                &state.world,
                &mut state.sim,
                TerrainTerms {
                    region_id: state.identity.region_id,
                    manages_estate: state.policy.estate_manager,
                    transfer_alerts: state.estate_policy.terrain_transfer_alerts,
                },
                &event,
                now,
            );
            // Before the world block, not inside it: this takes the region lock
            // itself for a task-inventory write, and the crate's one lock rule
            // is session → region, never region → region.
            let upload_minter = state.minter.clone();
            let _upload = crate::uploads::answer_upload(
                &state.assets,
                &state.world,
                crate::uploads::UploadTerms {
                    announcements: state.upload_announcements,
                    landmarks: crate::uploads::LandmarkTerms {
                        body: state.home_policy.landmark_body,
                        masks: state.home_policy.landmark_masks,
                        region_id: state.identity.region_id,
                        region_handle: state.identity.region_handle,
                    },
                },
                &mut state.sim,
                &event,
                &move || upload_minter.uuid(),
                now,
            );
            {
                let minter = state.minter.clone();
                let mut world = state.world.lock();
                let mut changes = answer_world_request(
                    crate::world::WorldStores {
                        world: &mut world,
                        assets: &state.assets,
                        selection: &mut state.selection,
                        sim: &mut state.sim,
                    },
                    crate::world::WorldPolicies {
                        identity: &state.avatar,
                        region: &state.identity,
                        object_assets: state.object_assets,
                        announcement: state.inventory_announcement,
                        parcels: state.parcel_policy,
                        sits: state.sit_policy,
                        mint: &move || minter.uuid(),
                        groups: crate::object_edits::GroupRights {
                            agent: state.avatar.agent_id,
                            groups: &state.groups,
                            policy: &state.groups_policy,
                        },
                    },
                    &event,
                    now,
                );
                // The estate is answered under the same lock as the rest of the
                // region, and after it: an estate command changes the region's
                // own configuration, which the world's other answers read.
                changes.extend(
                    crate::estate::answer_estate_request(
                        &mut world,
                        crate::estate::EstateAsker {
                            identity: &state.identity,
                            agent: &state.avatar,
                            policy: state.policy,
                            estate: state.estate_policy,
                        },
                        &mut state.sim,
                        &event,
                        now,
                    )
                    .unwrap_or_default(),
                );
                // A display-name change is the agent's own session's to
                // store and say; the rest of the region learns it here.
                if let ServerEvent::DisplayNameChanged(update) = &event {
                    changes.push(RegionChange::DisplayNameChanged(update.clone()));
                }
                // Published while the store is still held, so a watcher that
                // wakes on the change and re-reads the region cannot see a
                // world the change has not landed in yet.
                for change in changes {
                    // A region whose other sessions have all gone has no
                    // subscribers; that is not a failure.
                    drop(state.changes.send(RegionUpdate {
                        source: state.seq,
                        change,
                    }));
                }
                drop(world);
            }
            crate::economy_policy::answer_economy_request(
                &state.economy,
                &mut state.sim,
                &event,
                now,
            );
            crate::agent_requests::answer_agent_request(
                state.policy,
                crate::agent_requests::HomeStore {
                    homes: &state.homes,
                    region_id: state.identity.region_id,
                    policy: &state.home_policy,
                },
                &state.world.lock(),
                state.avatar.agent_id,
                &mut state.sim,
                &event,
                now,
            );
            crate::god_powers::answer_god_request(
                state.god_powers,
                &mut state.god_mode,
                state.avatar.agent_id,
                &mut state.sim,
                &event,
                now,
            );
            crate::world_map::answer_map_request(
                &state.map_policy,
                &state.map,
                state.identity.region_handle,
                &mut state.sim,
                &event,
                now,
            );
            if let Some(hook) = &state.on_event {
                hook(&mut state.sim, &event, now);
            }
            // A reliable packet the grid stopped resending is one the client
            // will never see: whatever a test was waiting for in it is gone,
            // and nothing else in a log would say why.
            if let ServerEvent::ReliableGiveUp { message } = &event {
                tracing::warn!(
                    message = message.as_deref().unwrap_or("?"),
                    "gave up resending a reliable packet the client never acknowledged"
                );
            }
            // Only lagging subscribers error; the driver never stalls on them.
            drop(self.events_tx.send(event));
        }
        let mut transmits = Vec::new();
        while let Some(transmit) = state.sim.poll_transmit() {
            transmits.push(transmit);
        }
        self.timeout_tx.send_replace(state.sim.poll_timeout());
        if state.sim.is_closed() {
            self.closed_tx.send_replace(true);
        }
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
            if let Some(delayed) = &self.delayed_tx {
                // The line is gone only once its sender task is, which is
                // once the session is: nothing is left to deliver to.
                let due = due_after(self.latency);
                let _gone = delayed.send((due, transmit));
            } else {
                send_datagram(&self.socket, &transmit).await;
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

    /// Whether the session machine has closed.
    pub(crate) fn is_closed(&self) -> bool {
        *self.closed_tx.borrow()
    }
}

/// The instant a datagram held back by `latency` is due: now plus the latency,
/// or now on (impossible) overflow.
fn due_after(latency: Duration) -> tokio::time::Instant {
    let now = tokio::time::Instant::now();
    now.checked_add(latency).unwrap_or(now)
}

/// Puts one datagram on the wire, logging a failure rather than returning it:
/// a datagram that cannot be sent is a datagram lost, which the protocol
/// already survives.
async fn send_datagram(socket: &UdpSocket, transmit: &Transmit) {
    if let Err(error) = socket
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

/// The outbound half of an injected link latency: sends each datagram once it
/// is due, in the order the session queued them. Ends when the session's last
/// handle is dropped.
async fn run_delayed_sender(
    socket: Arc<UdpSocket>,
    mut delayed: mpsc::UnboundedReceiver<(tokio::time::Instant, Transmit)>,
) {
    while let Some((due, transmit)) = delayed.recv().await {
        tokio::time::sleep_until(due).await;
        send_datagram(&socket, &transmit).await;
    }
}

/// Builds the wake-up channels for a new session around its already-bound
/// socket and initial state, returning the shared handle. A non-zero
/// `latency` also starts the task that holds outbound datagrams back by it.
pub(crate) fn new_shared_sim(
    state: SimState,
    socket: Arc<UdpSocket>,
    shutdown_rx: watch::Receiver<bool>,
    clock: Now,
    latency: Duration,
) -> SharedSim {
    let (events_tx, _) = broadcast::channel(EVENTS_CHANNEL_CAPACITY);
    let (timeout_tx, _) = watch::channel(state.sim.poll_timeout());
    let (closed_tx, _) = watch::channel(false);
    let delayed_tx = (!latency.is_zero()).then(|| {
        let (delayed_tx, delayed_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_delayed_sender(Arc::clone(&socket), delayed_rx));
        delayed_tx
    });
    SharedSim {
        state: Arc::new(Mutex::new(state)),
        socket,
        eq_notify: Arc::new(Notify::new()),
        timeout_tx,
        events_tx,
        shutdown_rx,
        closed_tx,
        clock,
        timeline_notify: Arc::new(Notify::new()),
        latency,
        delayed_tx,
    }
}

/// Receive buffer size for the UDP pump; comfortably above the LLUDP MTU.
const RECV_BUFFER_BYTES: usize = 64 * 1024;

/// How long the UDP pump waits after a failed receive before trying again,
/// so a persistently-failing socket cannot spin the task at 100% CPU.
const RECV_ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// The UDP pump: receive datagrams into the machine and flush, until the
/// grid shuts down or the session closes.
///
/// With a link latency injected, a received datagram waits it out in arrival
/// order before the machine is handed it — the inbound half of the delay
/// whose outbound half is [`run_delayed_sender`].
pub(crate) async fn run_udp_pump(shared: SharedSim) {
    let mut shutdown_rx = shared.shutdown_rx.clone();
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut buffer = vec![0_u8; RECV_BUFFER_BYTES];
    // Datagrams received and not yet due, oldest first.
    let mut held: VecDeque<(tokio::time::Instant, SocketAddr, Vec<u8>)> = VecDeque::new();
    // Consecutive receive failures, to keep a broken socket from flooding
    // the log at the backoff rate.
    let mut failures: u32 = 0;
    loop {
        let next_due = held.front().map(|(due, _, _)| *due);
        tokio::select! {
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    tracing::info!("session closed; UDP pump exiting");
                    break;
                }
            }
            received = shared.socket.recv_from(&mut buffer) => {
                let (length, from) = match received {
                    Ok(pair) => {
                        failures = 0;
                        pair
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        if failures == 1 {
                            tracing::warn!("UDP receive failed: {error}");
                        } else {
                            tracing::debug!("UDP receive failed ({failures} in a row): {error}");
                        }
                        tokio::time::sleep(RECV_ERROR_BACKOFF).await;
                        continue;
                    }
                };
                let datagram = buffer.get(..length).unwrap_or_default();
                if shared.latency.is_zero() {
                    if deliver_datagram(&shared, from, datagram).await {
                        break;
                    }
                } else {
                    let due = due_after(shared.latency);
                    held.push_back((due, from, datagram.to_vec()));
                }
            }
            () = sleep_until_due(next_due) => {
                if let Some((_due, from, datagram)) = held.pop_front()
                    && deliver_datagram(&shared, from, &datagram).await
                {
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

/// Sleeps until `due`, or forever when nothing is held.
async fn sleep_until_due(due: Option<tokio::time::Instant>) {
    match due {
        Some(instant) => tokio::time::sleep_until(instant).await,
        None => std::future::pending().await,
    }
}

/// Hands one received datagram to the machine and flushes what it answered;
/// `true` once the session has closed.
async fn deliver_datagram(shared: &SharedSim, from: SocketAddr, datagram: &[u8]) -> bool {
    let mut guard = shared.state.lock().await;
    if let Err(error) = guard.sim.handle_datagram(from, datagram, shared.now()) {
        tracing::debug!("datagram from {from} rejected: {error}");
    }
    let outcome = shared.flush_locked(&mut guard);
    let closed = guard.sim.is_closed();
    drop(guard);
    shared.finish_flush(outcome).await;
    if closed {
        tracing::info!("session closed; UDP pump exiting");
    }
    closed
}

/// The health an arriving agent is told it has: all of it.
const FULL_HEALTH: f32 = 100.0;

/// The region watcher: streams the changes *other* sessions in this region
/// made to the shared world — an object rezzed, an object taken away — to this
/// session's viewer.
///
/// A real simulator has one update loop per region that sweeps the scene and
/// sends every viewer what changed. The fake grid has no loop: the session
/// that made the change publishes it and this task, one per session, turns it
/// back into the `ObjectUpdate` or `KillObject` its own circuit needs. A
/// watcher skips the changes its own session published: that client was told
/// directly, in the same breath as the mutation, and telling it again would
/// put every rez down its circuit twice.
///
/// A lagging watcher would silently lose changes, which for a `KillObject`
/// means a ghost object standing in that viewer for good — so a lag is
/// reported rather than swallowed, and the watcher resynchronises by carrying
/// on with the changes it can still see.
pub(crate) async fn run_region_watcher(
    core: Arc<crate::runtime::GridCore>,
    shared: SharedSim,
    seq: u64,
    mut changes: broadcast::Receiver<RegionUpdate>,
) {
    let mut shutdown_rx = shared.shutdown_rx.clone();
    let mut closed_rx = shared.closed_tx.subscribe();
    loop {
        tokio::select! {
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    break;
                }
            }
            received = changes.recv() => {
                match received {
                    // A speaker hears its own line, and whoever made a sound
                    // or an effect may be sent it back; every other change
                    // its session was told about in the same breath.
                    Ok(RegionUpdate { source, change })
                        if source == seq
                            && !matches!(
                                change,
                                RegionChange::Chat(_)
                                    | RegionChange::Effects(_)
                                    | RegionChange::Sound(_)
                            ) => {}
                    Ok(RegionUpdate { change, .. }) => {
                        // A line is heard over a border too, by where the
                        // hearer's avatar stands in the region next door.
                        let over_the_border = match &change {
                            RegionChange::Chat(_) => avatar_over_the_border(&core, &shared).await,
                            _other => None,
                        };
                        let now = shared.now();
                        // Second Life has the hearer's own region say such a
                        // line, in its own coordinates: this session only
                        // works out that it is heard.
                        if let (RegionChange::Chat(line), Some(over)) = (&change, &over_the_border)
                        {
                            let policy = shared.state.lock().await.world.lock().chat;
                            if policy.over_borders
                                == crate::chat::OverBorders::FromTheHearersRegion
                            {
                                say_from_the_hearers_region(&core, &policy, line, over, now).await;
                                continue;
                            }
                        }
                        shared
                            .with_state(|state| {
                                let result = match &change {
                                    RegionChange::Rezzed(object)
                                    | RegionChange::Updated(object) => crate::world::send_objects(
                                        &mut state.sim,
                                        std::slice::from_ref(object.as_ref()),
                                        now,
                                    ),
                                    RegionChange::Moved(object) => state.sim.send_terse_update(
                                        &[sl_proto::TerseUpdate {
                                            local_id: object.local_id,
                                            state: object.state,
                                            motion: object.motion.clone(),
                                        }],
                                        crate::world::REAL_TIME_DILATION,
                                        now,
                                    ),
                                    RegionChange::Killed(local_id) => {
                                        state.sim.send_kill_object(&[*local_id], now)
                                    }
                                    // The one change that is a subscription
                                    // rather than a broadcast: a client that is
                                    // not looking at the object is not told,
                                    // exactly as a simulator does not tell it.
                                    RegionChange::PropertiesChanged {
                                        local_id,
                                        properties,
                                    } => {
                                        if !state.selection.contains(local_id) {
                                            return;
                                        }
                                        state.sim.send_object_properties(properties, now)
                                    }
                                    // The same subscription: a script that
                                    // started in a prim this client holds
                                    // selected moved its contents serial, and
                                    // Second Life says so with a listing
                                    // reply nobody asked for.
                                    RegionChange::ContentsListed { local_id } => {
                                        if !state.selection.contains(local_id) {
                                            return;
                                        }
                                        let listed = {
                                            let world = state.world.lock();
                                            world.object_by_local_id(*local_id).zip(
                                                world.task_inventories.get(local_id).cloned(),
                                            )
                                        };
                                        let Some((object, contents)) = listed else {
                                            return;
                                        };
                                        state.sim.serve_task_inventory(
                                            object.full_id,
                                            contents.serial,
                                            &contents.items,
                                            now,
                                        )
                                    }
                                    // A parcel push goes to the avatars standing
                                    // on that parcel -- OpenSim's
                                    // `SendLandUpdateToAvatarsOverMe` -- so a
                                    // resident on the other side of the region
                                    // is not sent a record of land they are not
                                    // on.
                                    //
                                    // On a grid whose land changes reach the
                                    // whole region (OpenSim's
                                    // `SendLandUpdateToAvatars`), everybody
                                    // else is sent it too, under `-10000`.
                                    RegionChange::ParcelChanged(parcel) => {
                                        if stands_on(state, parcel.local_id) {
                                            state.sim.enqueue_parcel_properties(parcel);
                                        } else if state.parcel_policy.land_change_reaches_region {
                                            let mut elsewhere = (**parcel).clone();
                                            elsewhere.sequence_id = NOT_STANDING_SEQUENCE_ID;
                                            state.sim.enqueue_parcel_properties(&elsewhere);
                                        }
                                        Ok(())
                                    }
                                    // The overlay as this session's viewer
                                    // sees whose land is whose.
                                    RegionChange::OverlayChanged => {
                                        let overlay =
                                            state.world.lock().overlay_for(state.avatar.agent_id);
                                        state.sim.send_parcel_overlay(&overlay, now)
                                    }
                                    RegionChange::RegionConfigured(limits) => {
                                        state.sim.set_region_stats_flags(limits.flags());
                                        state.sim.send_region_info(limits, now)
                                    }
                                    RegionChange::RegionFlagsChanged(flags) => {
                                        state.sim.set_region_stats_flags(*flags);
                                        Ok(())
                                    }
                                    // Heard by whoever is in earshot: an
                                    // avatar standing here, and — down the
                                    // child circuit its viewer holds here —
                                    // one standing just over the border.
                                    RegionChange::Chat(line) => {
                                        let policy = state.world.lock().chat;
                                        let standing = if state.sim.is_root_agent() {
                                            crate::chat::agent_position(&state.sim)
                                        } else if let Some(over) = &over_the_border {
                                            over.standing.clone()
                                        } else {
                                            return;
                                        };
                                        let Some(audible) = policy.hears(
                                            line,
                                            state.avatar.agent_id,
                                            &standing,
                                        ) else {
                                            return;
                                        };
                                        state.sim.send_chat_from_simulator(
                                            &line.from_name,
                                            line.source,
                                            line.owner_id,
                                            line.chat_type,
                                            audible,
                                            line.position.clone(),
                                            &line.message,
                                            now,
                                        )
                                    }
                                    // To whoever the grid's flavour sends
                                    // them: by the viewer's camera, where
                                    // the grid asks how near it is.
                                    RegionChange::Effects(sent) => {
                                        let policy = state.world.lock().relays;
                                        let camera = state.camera_at.clone().unwrap_or_else(|| {
                                            crate::chat::agent_position(&state.sim)
                                        });
                                        if !policy.effects_reach(
                                            sent,
                                            state.avatar.agent_id,
                                            &camera,
                                        ) {
                                            return;
                                        }
                                        state.sim.send_viewer_effect(sent.from, &sent.effects, now)
                                    }
                                    // Heard by a root agent in range of where
                                    // the sound is said to be, where the
                                    // grid's flavour has a range.
                                    RegionChange::Sound(sound) => {
                                        if !state.sim.is_root_agent() {
                                            return;
                                        }
                                        let policy = state.world.lock().relays;
                                        let standing = crate::chat::agent_position(&state.sim);
                                        if !policy.sound_reaches(sound, &standing) {
                                            return;
                                        }
                                        state.sim.send_triggered_sound(sound, now)
                                    }
                                    // An estate notice reaches every avatar
                                    // in the region.
                                    RegionChange::Notice(notice) => {
                                        if !state.sim.is_root_agent() {
                                            return;
                                        }
                                        crate::estate::send_notice(
                                            &mut state.sim,
                                            state.avatar.agent_id,
                                            notice,
                                            now,
                                        )
                                    }
                                    RegionChange::TerrainRetextured(composition) => {
                                        let mut identity = state.identity.clone();
                                        identity.terrain = **composition;
                                        state.sim.send_region_handshake(&identity, now)
                                    }
                                    RegionChange::DisplayNameChanged(update) => {
                                        state.sim.set_display_name(update.name.clone());
                                        state.sim.enqueue_display_name_update(update);
                                        Ok(())
                                    }
                                };
                                if let Err(error) = result {
                                    tracing::debug!(
                                        "streaming a neighbouring session's world change failed: \
                                         {error}"
                                    );
                                }
                            })
                            .await;
                    }
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(
                            "this session missed {missed} of its region's world changes; its \
                             view of the region is now stale until the next refetch"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
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

/// The width of a region, in metres.
const REGION_M: f32 = 256.0;

/// An avatar standing in a region next to the one a session is a child in.
struct OverTheBorder {
    /// The agent.
    agent: sl_types::key::AgentKey,
    /// Where its avatar stands, in the coordinates of the session's region.
    standing: sl_proto::Vector,
    /// How many metres east and north of the session's region the avatar's
    /// own region begins.
    offset: (f32, f32),
}

/// Sends the agent of `over` the `line` said in the region next door, if it
/// is in earshot of it, from the region its avatar stands in and in that
/// region's coordinates: Second Life's way of carrying a line over a border.
async fn say_from_the_hearers_region(
    core: &Arc<crate::runtime::GridCore>,
    policy: &crate::chat::ChatPolicy,
    line: &crate::chat::Line,
    over: &OverTheBorder,
    now: Instant,
) {
    let Some(audible) = policy.hears(line, over.agent, &over.standing) else {
        return;
    };
    // A typing notice past a say's reach, which Second Life marks barely
    // audible within a region, mostly does not come over a border at all:
    // one direction of one aditi run in four brought it (2026-10-13).
    if audible == crate::chat::BARELY_AUDIBLE {
        return;
    }
    let Some(root) = core.root_session_of(over.agent).await else {
        return;
    };
    let position = sl_proto::Vector {
        x: line.position.x - over.offset.0,
        y: line.position.y - over.offset.1,
        z: line.position.z,
    };
    let sent = root
        .with_sim(|sim| {
            sim.send_chat_from_simulator(
                &line.from_name,
                line.source,
                line.owner_id,
                line.chat_type,
                audible,
                position,
                &line.message,
                now,
            )
        })
        .await;
    if let Err(error) = sent {
        tracing::debug!("saying a neighbouring region's line failed: {error}");
    }
}

/// The avatar of `shared`'s agent, when `shared`'s region is not the one it
/// stands in: where a child agent hears from. [`None`] for a root session,
/// and for an agent with no root session to ask.
async fn avatar_over_the_border(
    core: &Arc<crate::runtime::GridCore>,
    shared: &SharedSim,
) -> Option<OverTheBorder> {
    let (agent, here) = {
        let state = shared.state.lock().await;
        if state.sim.is_root_agent() {
            return None;
        }
        (state.avatar.agent_id, state.region)
    };
    let root = core.root_session_of(agent).await?;
    let (at, there) = {
        let state = root.state.lock().await;
        (crate::chat::agent_position(&state.sim), state.region)
    };
    let here = &core.region(here)?.config;
    let there = &core.region(there)?.config;
    let metres = |there: u32, here: u32| {
        let slots = i64::from(there).saturating_sub(i64::from(here));
        // Neighbours are a slot apart at most; anything else is far.
        let slots = i16::try_from(slots).unwrap_or(i16::MAX);
        f32::from(slots) * REGION_M
    };
    let offset = (
        metres(there.grid_x, here.grid_x),
        metres(there.grid_y, here.grid_y),
    );
    Some(OverTheBorder {
        agent,
        standing: sl_proto::Vector {
            x: at.x + offset.0,
            y: at.y + offset.1,
            z: at.z,
        },
        offset,
    })
}

/// The sequence id a changed parcel's record is pushed under to an avatar not
/// standing on it (the reference's `SELECTED_PARCEL_SEQ_ID`, which OpenSim's
/// `SendLandUpdateToAvatars` uses).
const NOT_STANDING_SEQUENCE_ID: i32 = -10000;

/// Whether this session's avatar is standing on `parcel` — who a changed
/// parcel's record is pushed to.
///
/// The position is the one the session was opened at, since the fake grid
/// tracks no movement: an avatar that walked across a parcel line since it
/// arrived is still counted where it landed. That is a limit of the grid, not
/// of the push — the alternative, telling the whole region, would make a test
/// of "who is told" pass for the wrong reason.
fn stands_on(state: &SimState, parcel: sl_proto::RegionLocalParcelId) -> bool {
    let placement = state.sim.arrival_position().position;
    state
        .world
        .lock()
        .parcel_at(placement.x(), placement.y())
        .is_some_and(|standing| standing.local_id == parcel)
}

/// Sleeps until `deadline`, or forever when there is none (the machine has
/// no timer armed, or a poll is held without a deadline).
pub(crate) async fn sleep_until_opt(deadline: Option<Instant>) {
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
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut timeout_rx = shared.timeout_tx.subscribe();
    loop {
        let deadline = *timeout_rx.borrow_and_update();
        tokio::select! {
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    tracing::info!("session closed; timer task exiting");
                    break;
                }
            }
            () = sleep_until_opt(deadline) => {
                let closed = {
                    let mut guard = shared.state.lock().await;
                    guard.sim.handle_timeout(shared.now());
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
