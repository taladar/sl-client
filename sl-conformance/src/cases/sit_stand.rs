//! Sit on things and stand up again, and record what the grid says and where
//! it puts the avatar.
//!
//! A sit is a request the simulator answers: an `AgentRequestSit` naming the
//! object and the point clicked, an `AvatarSitResponse` with the seat's
//! transform, and the avatar's own object re-sent as a child of the seat. What
//! varies — between grids, and with the seat — is whether the request is
//! answered at all, what the answer's `AutoPilot` flag says, where the avatar
//! ends up, and what a refusal looks like. This case asks for one sit after
//! another and writes each answer down (`book/src/gridspec/movement.md`
//! § Sitting).
//!
//! The legs, in order:
//!
//! - **near** — a cube rezzed beside the avatar, no script in it. An unanswered
//!   request is asked again after a `FINISH_ANIM`, and then after a `STAND_UP`,
//!   and what got it answered is recorded;
//! - **second** — the second avatar asks to sit on the same cube while the
//!   first is on it, and stands again;
//! - **stand** — where standing up puts the avatar;
//! - **clicked** — the same sit asked for at a point off the cube's centre;
//! - **ground** — the `SIT_ON_GROUND` control, and a sit on the cube from
//!   there;
//! - **far** — the same cube from two distances the avatar is flown to;
//! - **target** — a script gives the cube a sit target (`llSitTarget`); the
//!   sit is asked from a distance, the second avatar asks for the taken
//!   target, and the first asks again from further off;
//! - **teleport** — a teleport within the region, asked while seated;
//! - **unknown** — a sit on an object the region does not have;
//! - **neighbour** — a sit on an object of a neighbouring region, where one
//!   was in view when the avatar arrived.
//!
//! On a fake grid only the legs that need neither a second resident in view,
//! nor a moving avatar, nor a compiled script are run: near, stand, ground and
//! unknown.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, ControlFlags, Event, InventoryType, Object, ObjectKey, PrimShape, RegionCoordinates,
    RegionLocalObjectId, RestoreItem, RezScriptParams, Rotation, ScriptTarget,
    ScriptUploadLocation, Uuid, Vector,
};

use crate::cases::agent_movement::{HOLDING_ANIMATIONS, animation_names, join};
use crate::cases::logout_seated::{
    DeleteOnDrop, delete_command, look_at, sight_object, trash_folder,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    Gait, REGION_TIMEOUT, REPLY_TIMEOUT, is_fake, is_opensim, settle_scene_with_avatar,
    steer_towards, wait_for_own_new_object, wait_for_task_listing,
};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/movement.md § Sitting (sit-stand, 2026-10-08)";

/// The OpenSim start location: the level square in the block's north-eastern
/// region that `agent-movement` measures on, so that the seat and every spot
/// the avatar is flown to are on the same ground.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// The overall budget for settling each avatar's initial scene.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends a settle.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for a rezzed seat, a task listing or a compile.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a sit request is given to be answered: just past the session's
/// own sit timeout, so the next request is not asked while this one is still
/// pending.
const ANSWER_BUDGET: Duration = Duration::from_secs(16);

/// How long an answered sit is given to show the avatar on the seat.
const SEAT_BUDGET: Duration = Duration::from_secs(6);

/// How long the avatar is watched after it is told to stand.
const STAND_BUDGET: Duration = Duration::from_secs(8);

/// How long the avatar is left alone after a flight for it to come down.
const LANDING: Duration = Duration::from_secs(8);

/// How long a short pause between two steps lasts.
const PAUSE: Duration = Duration::from_secs(2);

/// How long a flight to a far spot may take.
const FLIGHT_BUDGET: Duration = Duration::from_secs(60);

/// How far from the avatar the seat is rezzed, in metres.
const SEAT_OFFSET_M: f32 = 1.5;

/// How far below a standing avatar's centre the centre of a half-metre cube
/// resting on the same ground is, in metres.
const SEAT_BELOW_AVATAR_M: f32 = 0.6;

/// The spots the avatar is walked to for the scriptless seat, in metres from
/// it. The walk stops a few metres short of each, and the distance a request
/// was made from is what is recorded: the steps are close enough together to
/// find the distance past which a grid stops answering.
const FAR_PLAIN_M: [f32; 6] = [7.0, 10.0, 13.0, 16.0, 19.0, 24.0];

/// How long a sit request a grid may well not answer is given — one from
/// those spots, or one for an object the region does not have. An answer
/// takes a fifth of a second; the session's own timeout would make a leg of
/// every silence.
const FAR_ANSWER_BUDGET: Duration = Duration::from_secs(4);

/// How long a walk to one of those spots may take.
const WALK_BUDGET: Duration = Duration::from_secs(30);

/// How close to the seat the second avatar has to stand to ask for it, in
/// metres: nearer than any distance a grid was seen to ignore a request from.
const SECOND_WITHIN_M: f32 = 4.5;

/// The further distance the seat with a sit target is asked for from, in
/// metres (the first request comes from the last of [`FAR_PLAIN_M`]).
const FAR_TARGET_M: f32 = 40.0;

/// How far from the seat the teleport asked for while seated goes, in metres.
const TELEPORT_M: f32 = 6.0;

/// The point of the seat the "clicked" leg names: on the half-metre cube's
/// top face, off its centre.
const CLICKED: Vector = Vector {
    x: 0.1,
    y: 0.2,
    z: 0.25,
};

/// The sit target the script sets: a metre above the seat's centre, turned a
/// quarter of the way round the vertical.
const SIT_TARGET_SCRIPT: &str = "default\n{\n    state_entry()\n    {\n        \
     llSitTarget(<0.0, 0.0, 1.0>, llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));\n    }\n}\n";

/// The case's overall budget.
const CASE_TIMEOUT: Duration = Duration::from_secs(12 * 60);

/// One update of an avatar's object.
#[derive(Debug, Clone)]
struct Sample {
    /// Seconds from the start of the watch.
    at: f64,
    /// The object the avatar is a child of; zero when it is not seated.
    parent: RegionLocalObjectId,
    /// Its position: in the region when it stands, relative to the seat when
    /// it sits.
    position: Vector,
    /// Its rotation, in the same frame.
    rotation: Rotation,
}

impl Sample {
    /// Whether the avatar is a child of an object.
    fn seated(&self) -> bool {
        self.parent != RegionLocalObjectId(0)
    }
}

/// One `AvatarSitResponse`.
#[derive(Debug, Clone)]
struct Reply {
    /// Seconds from the start of the watch.
    at: f64,
    /// The object it names.
    object: ObjectKey,
    /// Its `AutoPilot` flag.
    autopilot: bool,
    /// The seat position, relative to the object.
    position: Vector,
    /// The seated rotation, relative to the object.
    rotation: Rotation,
    /// The scripted camera's eye offset.
    eye: Vector,
    /// The scripted camera's focus offset.
    focus: Vector,
    /// Its `ForceMouselook` flag.
    mouselook: bool,
}

/// Everything one session was told about its own avatar, in order.
#[derive(Debug)]
struct Watch {
    /// The session's own avatar.
    agent: Uuid,
    /// Another avatar whose updates are kept as well.
    other: Option<Uuid>,
    /// When the watch began.
    started: Instant,
    /// Every update of the avatar's own object.
    samples: Vec<Sample>,
    /// The latest update of the other avatar's object.
    other_last: Option<Sample>,
    /// Every statement of the avatar's animation set.
    animations: Vec<(f64, BTreeSet<String>)>,
    /// Every alert: when, its name (empty for a bare text) and its text.
    alerts: Vec<(f64, String, String)>,
    /// Every sit response.
    replies: Vec<Reply>,
    /// Every teleport event: when, and which.
    teleports: Vec<(f64, String)>,
    /// Whether a holding animation is answered with a `FINISH_ANIM`.
    finish: bool,
    /// The holding animations already answered, while they stay in the set.
    finished: BTreeSet<&'static str>,
}

impl Watch {
    /// A watch of `agent`, last seen standing at `standing`: an avatar at rest
    /// is not reported again, so where the scene's settling left it is the
    /// first thing known about it.
    fn new(agent: Uuid, other: Option<Uuid>, standing: Option<Vector>) -> Self {
        let samples = standing
            .map(|position| Sample {
                at: 0.0,
                parent: RegionLocalObjectId(0),
                position,
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
            })
            .into_iter()
            .collect();
        Self {
            agent,
            other,
            started: Instant::now(),
            samples,
            other_last: None,
            animations: Vec::new(),
            alerts: Vec::new(),
            replies: Vec::new(),
            teleports: Vec::new(),
            finish: false,
            finished: BTreeSet::new(),
        }
    }

    /// Seconds since the watch began.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The latest update of the avatar, or the failure its absence is.
    fn here(&self) -> Result<Sample, TestFailure> {
        self.samples.last().cloned().ok_or_else(|| {
            TestFailure::Assertion("our own avatar never appeared in the object stream".to_owned())
        })
    }

    /// The animation set as last stated, as one string.
    fn animation_set(&self) -> String {
        self.animations
            .last()
            .map_or_else(|| "none".to_owned(), |(_at, set)| join(set))
    }

    /// The animation sets stated since `since`, each with its offset from it.
    fn animations_since(&self, since: f64) -> String {
        let stated: Vec<String> = self
            .animations
            .iter()
            .filter(|(at, _set)| *at >= since)
            .map(|(at, set)| format!("{:.2}:{}", at - since, join(set)))
            .collect();
        if stated.is_empty() {
            "none".to_owned()
        } else {
            stated.join(" ")
        }
    }

    /// The holding animations in the set that have not been answered yet.
    fn unanswered_holds(&self) -> Vec<&'static str> {
        let Some((_at, set)) = self.animations.last() else {
            return Vec::new();
        };
        HOLDING_ANIMATIONS
            .into_iter()
            .filter(|name| set.contains(*name) && !self.finished.contains(name))
            .collect()
    }

    /// Folds one event into the watch.
    fn note(&mut self, event: &Event) {
        let at = self.now();
        match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id.uuid() == self.agent
                    || Some(object.full_id.uuid()) == self.other =>
            {
                let sample = Sample {
                    at,
                    parent: object.parent_id,
                    position: object.motion.position.clone(),
                    rotation: object.motion.rotation.clone(),
                };
                if object.full_id.uuid() == self.agent {
                    tracing::debug!(at, ?sample, "our avatar");
                    self.samples.push(sample);
                } else {
                    self.other_last = Some(sample);
                }
            }
            Event::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } if avatar_id.uuid() == self.agent => {
                let set = animation_names(animations);
                self.finished.retain(|name| set.contains(*name));
                tracing::info!(at, set = join(&set), "our animation set");
                self.animations.push((at, set));
            }
            Event::AlertMessage {
                message,
                alert_info,
                ..
            } => {
                let name = alert_info
                    .iter()
                    .map(|info| info.message.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                tracing::info!(at, name, message, "an alert");
                self.alerts.push((at, name, message.trim().to_owned()));
            }
            Event::AgentAlertMessage { message, .. } => {
                tracing::info!(at, message, "an agent alert");
                self.alerts
                    .push((at, "(agent alert)".to_owned(), message.trim().to_owned()));
            }
            Event::SitResult {
                sit_object,
                autopilot,
                sit_position,
                sit_rotation,
                camera_eye_offset,
                camera_at_offset,
                force_mouselook,
            } => {
                tracing::info!(at, ?event, "a sit response");
                self.replies.push(Reply {
                    at,
                    object: *sit_object,
                    autopilot: *autopilot,
                    position: sit_position.clone(),
                    rotation: sit_rotation.clone(),
                    eye: camera_eye_offset.clone(),
                    focus: camera_at_offset.clone(),
                    mouselook: *force_mouselook,
                });
            }
            Event::TeleportLocal { position, .. } => {
                self.teleports.push((
                    at,
                    format!(
                        "local {:.1}/{:.1}/{:.1}",
                        position.x(),
                        position.y(),
                        position.z()
                    ),
                ));
            }
            Event::TeleportFailed { reason, .. } => {
                self.teleports.push((at, format!("failed: {reason}")));
            }
            Event::TeleportFinished { .. } => {
                self.teleports.push((at, "finished".to_owned()));
            }
            _other => {}
        }
    }

    /// Watches `session` for `budget`, or until `until` holds, and says
    /// whether it came to hold.
    async fn observe(
        &mut self,
        session: &mut Session,
        budget: Duration,
        until: impl Fn(&Self) -> bool + Send + Sync,
    ) -> Result<bool, TestFailure> {
        let started = Instant::now();
        loop {
            if until(self) {
                return Ok(true);
            }
            let remaining = budget.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(false);
            }
            let woken = session
                .wait_for(remaining, |event| {
                    self.note(event);
                    (until(self) || (self.finish && !self.unanswered_holds().is_empty()))
                        .then_some(())
                })
                .await;
            match woken {
                Ok(()) => {}
                Err(TestFailure::Timeout(_)) => return Ok(until(self)),
                Err(other) => return Err(other),
            }
            if self.finish {
                let holds = self.unanswered_holds();
                if !holds.is_empty() {
                    session.send(Command::FinishAnimation).await?;
                    self.finished.extend(holds);
                }
            }
        }
    }
}

/// A vector as `x/y/z` to the centimetre.
fn xyz(vector: &Vector) -> String {
    format!("{:.2}/{:.2}/{:.2}", vector.x, vector.y, vector.z)
}

/// A rotation as `x/y/z/s` to three places.
fn xyzs(rotation: &Rotation) -> String {
    format!(
        "{:.3}/{:.3}/{:.3}/{:.3}",
        rotation.x, rotation.y, rotation.z, rotation.s
    )
}

/// The horizontal distance between two region positions, in metres.
fn ground_distance(a: &Vector, b: &Vector) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// What a sit request was answered with.
#[derive(Debug, Clone)]
enum Answer {
    /// An `AvatarSitResponse`.
    Response(Reply),
    /// An alert and no response: its name and its text.
    Alert(String, String),
    /// Neither, for as long as the request was given.
    Nothing,
}

impl Answer {
    /// The answer's kind, as the word the book's tables use.
    const fn kind(&self) -> &'static str {
        match self {
            Self::Response(_) => "response",
            Self::Alert(..) => "alert",
            Self::Nothing => "nothing",
        }
    }
}

/// One sit request and what came of it.
#[derive(Debug, Clone)]
struct Attempt {
    /// The object asked for.
    target: ObjectKey,
    /// How far from the seat the avatar stood when it asked, horizontally.
    distance_m: Option<f32>,
    /// The avatar's animation set when it asked.
    asked_in: String,
    /// The answer.
    answer: Answer,
    /// Seconds from the request to the answer.
    answer_secs: Option<f64>,
    /// The first update that showed the avatar as a child of an object.
    seated: Option<Sample>,
    /// Where the avatar's last update put it, when it never showed it seated.
    left_at: Option<Sample>,
    /// The animation sets stated from the request on.
    animations: String,
}

impl Attempt {
    /// Whether the request drew an `AvatarSitResponse`.
    const fn answered(&self) -> bool {
        matches!(self.answer, Answer::Response(_))
    }

    /// Writes the attempt down under `leg`.
    fn record(&self, leg: &str, metrics: &mut Metrics) {
        let key = |field: &str| format!("{leg}_{field}");
        metrics.set(&key("answer"), self.answer.kind());
        metrics.set(&key("asked_in"), self.asked_in.clone());
        metrics.set(&key("animations"), self.animations.clone());
        if let Some(distance) = self.distance_m {
            metrics.set(&key("distance_m"), f64::from(distance));
        }
        if let Some(secs) = self.answer_secs {
            metrics.set(&key("answer_secs"), secs);
        }
        match &self.answer {
            Answer::Response(reply) => {
                metrics.set(&key("names_target"), reply.object == self.target);
                metrics.set(&key("autopilot"), reply.autopilot);
                metrics.set(&key("sit_position"), xyz(&reply.position));
                metrics.set(&key("sit_rotation"), xyzs(&reply.rotation));
                metrics.set(&key("camera_eye"), xyz(&reply.eye));
                metrics.set(&key("camera_at"), xyz(&reply.focus));
                metrics.set(&key("force_mouselook"), reply.mouselook);
            }
            Answer::Alert(name, text) => {
                metrics.set(&key("alert_name"), name.clone());
                metrics.set(&key("alert_text"), text.clone());
            }
            Answer::Nothing => {}
        }
        metrics.set(&key("seated"), self.seated.is_some());
        if let Some(seated) = &self.seated {
            metrics.set(&key("seated_position"), xyz(&seated.position));
            metrics.set(&key("seated_rotation"), xyzs(&seated.rotation));
        }
        if let Some(left) = &self.left_at {
            metrics.set(&key("left_at"), xyz(&left.position));
        }
    }
}

/// Asks to sit on `target`, whose centre is at `seat_at` when the region has
/// it, naming that centre as the point clicked, and watches what comes back.
async fn try_sit(
    session: &mut Session,
    watch: &mut Watch,
    target: ObjectKey,
    seat_at: Option<&Vector>,
    budget: Duration,
) -> Result<Attempt, TestFailure> {
    let centre = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    try_sit_at(session, watch, target, seat_at, centre, budget).await
}

/// Asks to sit on `target` at the point `clicked`, an offset from its centre,
/// and watches what comes back.
async fn try_sit_at(
    session: &mut Session,
    watch: &mut Watch,
    target: ObjectKey,
    seat_at: Option<&Vector>,
    clicked: Vector,
    budget: Duration,
) -> Result<Attempt, TestFailure> {
    let asked_at = watch.now();
    let (replies, alerts) = (watch.replies.len(), watch.alerts.len());
    let standing = watch.samples.last().filter(|sample| !sample.seated());
    let distance_m = match (standing, seat_at) {
        (Some(sample), Some(seat)) => Some(ground_distance(&sample.position, seat)),
        _unknown => None,
    };
    let asked_in = watch.animation_set();
    session
        .send(Command::Sit {
            target,
            offset: clicked,
        })
        .await?;
    let _answered = watch
        .observe(session, budget, |watch| {
            watch.replies.len() > replies || watch.alerts.len() > alerts
        })
        .await?;
    let answer = if let Some(reply) = watch.replies.get(replies) {
        Answer::Response(reply.clone())
    } else if let Some((_at, name, text)) = watch.alerts.get(alerts) {
        Answer::Alert(name.clone(), text.clone())
    } else {
        Answer::Nothing
    };
    let answer_secs = match &answer {
        Answer::Response(reply) => Some(reply.at - asked_at),
        Answer::Alert(..) => watch.alerts.get(alerts).map(|(at, ..)| at - asked_at),
        Answer::Nothing => None,
    };
    let was_seated = |watch: &Watch| {
        watch
            .samples
            .iter()
            .any(|sample| sample.at >= asked_at && sample.seated())
    };
    if matches!(answer, Answer::Response(_)) {
        let _seated = watch.observe(session, SEAT_BUDGET, was_seated).await?;
    }
    let seated = watch
        .samples
        .iter()
        .find(|sample| sample.at >= asked_at && sample.seated())
        .cloned();
    let left_at = if seated.is_none() {
        watch.samples.last().cloned()
    } else {
        None
    };
    let attempt = Attempt {
        target,
        distance_m,
        asked_in,
        answer,
        answer_secs,
        seated,
        left_at,
        animations: watch.animations_since(asked_at),
    };
    tracing::info!(?attempt, "a sit attempt");
    Ok(attempt)
}

/// Where standing up left an avatar.
#[derive(Debug, Clone)]
struct Stood {
    /// Seconds from the `STAND_UP` to the first update that showed the avatar
    /// free of the seat.
    secs: Option<f64>,
    /// Where that update put it.
    position: Option<Vector>,
    /// Where its last update put it.
    rest: Option<Vector>,
    /// The animation sets stated from the `STAND_UP` on.
    animations: String,
}

impl Stood {
    /// Writes the stand down under `leg`, with the seat at `seat_at`.
    fn record(&self, leg: &str, seat_at: &Vector, metrics: &mut Metrics) {
        let key = |field: &str| format!("{leg}_{field}");
        metrics.set(&key("unseated"), self.secs.is_some());
        if let Some(secs) = self.secs {
            metrics.set(&key("secs"), secs);
        }
        metrics.set(&key("animations"), self.animations.clone());
        if let Some(position) = &self.position {
            metrics.set(&key("position"), xyz(position));
            metrics.set(
                &key("offset_from_seat"),
                xyz(&Vector {
                    x: position.x - seat_at.x,
                    y: position.y - seat_at.y,
                    z: position.z - seat_at.z,
                }),
            );
        }
        if let Some(rest) = &self.rest {
            metrics.set(&key("rest"), xyz(rest));
        }
    }
}

/// Stands the avatar up and watches where it is put.
async fn stand_up(session: &mut Session, watch: &mut Watch) -> Result<Stood, TestFailure> {
    let asked_at = watch.now();
    session.send(Command::Stand).await?;
    let free = |watch: &Watch| {
        watch
            .samples
            .iter()
            .any(|sample| sample.at >= asked_at && !sample.seated())
    };
    let _freed = watch.observe(session, STAND_BUDGET, free).await?;
    let first = watch
        .samples
        .iter()
        .find(|sample| sample.at >= asked_at && !sample.seated())
        .cloned();
    // Left alone for a moment: the avatar settles onto the ground, and a grid
    // that holds it for an animation says so.
    let _quiet = watch.observe(session, PAUSE, |_watch| false).await?;
    let rest = watch
        .samples
        .last()
        .filter(|sample| sample.at >= asked_at && !sample.seated())
        .map(|sample| sample.position.clone());
    let stood = Stood {
        secs: first.as_ref().map(|sample| sample.at - asked_at),
        position: first.map(|sample| sample.position),
        rest,
        animations: watch.animations_since(asked_at),
    };
    tracing::info!(?stood, "a stand");
    Ok(stood)
}

/// Moves the avatar towards the spot `distance` metres north of `seat_at`
/// and lets it come to rest.
async fn move_to(
    session: &mut Session,
    watch: &mut Watch,
    seat_at: &Vector,
    distance: f32,
    gait: Gait,
) -> Result<(), TestFailure> {
    let from = watch.here()?.position;
    let spot = Vector {
        x: seat_at.x,
        y: seat_at.y + distance,
        z: seat_at.z,
    };
    let (budget, rest) = match gait {
        Gait::Walking => (WALK_BUDGET, PAUSE),
        Gait::Flying | Gait::FlyingLevel => (FLIGHT_BUDGET, LANDING),
    };
    let _stopped = steer_towards(session, from, &spot, gait, budget, |event| {
        watch.note(event);
    })
    .await?;
    session
        .send(Command::SetControls(ControlFlags::empty()))
        .await?;
    let _rested = watch.observe(session, rest, |_watch| false).await?;
    Ok(())
}

/// Creates a script in `seat` that gives it a sit target, and says whether
/// the grid compiled it.
async fn plant_sit_target(session: &mut Session, seat: &Object) -> Result<bool, TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    session
        .send(Command::RezScript {
            target: seat.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: false,
                item: RestoreItem::new_script(agent, seat.full_id, "New Script", Uuid::new_v4()),
            }),
        })
        .await?;
    let started = Instant::now();
    let item_id = loop {
        session
            .send(Command::FetchTaskInventory {
                target: seat.scoped_id(),
            })
            .await?;
        let found = wait_for_task_listing(session, seat.full_id, STEP_TIMEOUT)
            .await?
            .iter()
            .find(|entry| entry.inv_type == InventoryType::Script)
            .map(|entry| entry.item_id);
        if let Some(item_id) = found {
            break item_id;
        }
        if started.elapsed() >= STEP_TIMEOUT {
            return Err(TestFailure::Assertion(
                "the new script did not appear in the seat's contents".to_owned(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    session
        .send(Command::UploadScript {
            location: ScriptUploadLocation::TaskInventory {
                task_id: seat.full_id,
                item_id,
                running: true,
                experience: None,
            },
            target: ScriptTarget::Mono,
            source: SIT_TARGET_SCRIPT.as_bytes().to_vec(),
        })
        .await?;
    let compiled = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ScriptUploaded { compiled, .. } => Some(*compiled),
            Event::AssetUploadFailed { .. } => Some(false),
            _other => None,
        })
        .await?;
    // The script has to run before the seat has its target.
    tokio::time::sleep(PAUSE).await;
    Ok(compiled)
}

/// Whether a sit on a seat beside the avatar is answered.
const NEAR_SIT_ANSWERED: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// The `AutoPilot` flag of the answer to a sit on a seat a metre and a half
/// away: set by both grids, as it is from every other distance.
const NEAR_AUTOPILOT: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Whether the seat position in the answer for a seat **with** a sit target
/// is where the avatar's own update then puts it. Second Life states that
/// position; OpenSim states the script's target, 0.35 m lower.
const TARGET_ANSWER_IS_WHERE_IT_SITS: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// The same for a seat **without** one, where the grid chooses the spot: here
/// it is Second Life whose answer is a third of a metre below the avatar and
/// OpenSim whose two agree.
const PLAIN_ANSWER_IS_WHERE_IT_SITS: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// Whether a sit on a scriptless seat is answered from more than
/// [`FAR_M`] away. Second Life answered from 7.5 m and not from 9.3 m, nor
/// from anywhere further; OpenSim answered from every distance tried, 20 m
/// the furthest.
const FAR_PLAIN_ANSWERED: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// The distance past which [`FAR_PLAIN_ANSWERED`] is read, in metres.
const FAR_M: f32 = 12.0;

/// Whether a seat with a sit target is answered from 40 m away.
const FAR_TARGET_ANSWERED: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// Whether a teleport within the region takes a seated avatar off its seat.
const TELEPORT_UNSEATS: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: SOURCE,
};

/// What a sit on an object the region does not have is answered with.
const UNKNOWN_ANSWER: Measured<&str> = Measured {
    second_life: "alert",
    opensim: "nothing",
    source: SOURCE,
};

/// The name of the alert that answers it, where one does.
const UNKNOWN_ALERT: Measured<&str> = Measured {
    second_life: "SitFailNotSameRegion",
    opensim: "",
    source: SOURCE,
};

/// How far in front of where it sat an avatar that stands up is put, in
/// metres: the same for both avatars on each grid, with a sit target and
/// without.
const STAND_FORWARD_M: Measured<f32> = Measured {
    second_life: 0.34,
    opensim: 0.65,
    source: SOURCE,
};

/// How far above where it sat it is put, in metres, before it drops.
const STAND_RISE_M: Measured<f32> = Measured {
    second_life: 0.0,
    opensim: 0.57,
    source: SOURCE,
};

/// How far a stand's placement may be from the measured one, in metres.
const STAND_SLACK_M: f32 = 0.03;

/// Holds the length `actual` to the one measured on `grid`, within
/// [`STAND_SLACK_M`].
fn check_length(
    what: &str,
    grid: Grid,
    measured: &Measured<f32>,
    actual: f32,
) -> Result<(), TestFailure> {
    let expected = *measured.on(grid);
    crate::support::check(
        (actual - expected).abs() <= STAND_SLACK_M,
        &format!(
            "{what}: {actual:.2} m on {grid:?}, measured {expected:.2} m ({})",
            measured.source
        ),
    )
}

/// How far an answer's seat position may be from the avatar's own update and
/// still count as the same place, in metres.
const SAME_PLACE_M: f32 = 0.05;

/// Whether the seat position `attempt`'s answer states is where the avatar's
/// own update put it; `None` when there was no answer or no update.
fn answer_is_where_it_sits(attempt: &Attempt) -> Option<bool> {
    let Answer::Response(reply) = &attempt.answer else {
        return None;
    };
    let seated = attempt.seated.as_ref()?;
    let (dx, dy, dz) = (
        reply.position.x - seated.position.x,
        reply.position.y - seated.position.y,
        reply.position.z - seated.position.z,
    );
    Some(dx.hypot(dy).hypot(dz) <= SAME_PLACE_M)
}

/// What the case found and holds every grid to.
#[derive(Debug, Default)]
struct Found {
    /// The first sit on the seat beside the avatar that was answered.
    near: Option<Attempt>,
    /// Whether standing up from it freed the avatar.
    stood: Option<bool>,
    /// Where that put the avatar, from where it sat: how far along the
    /// ground, and how far up.
    stood_at: Option<(f32, f32)>,
    /// Each sit on the scriptless seat from further off: the distance, and
    /// whether it was answered.
    far: Vec<(f32, bool)>,
    /// The first sit on the seat once it had a sit target.
    target: Option<Attempt>,
    /// Whether the sit target was answered from its furthest distance.
    far_target: Option<bool>,
    /// Whether the avatar was still on its seat after a teleport.
    seated_after_teleport: Option<bool>,
    /// The sit on an object the region does not have.
    unknown: Option<Attempt>,
}

impl Found {
    /// Holds what was found to the measured answers.
    fn check(&self, grid: Grid) -> Result<(), TestFailure> {
        let near = self.near.as_ref();
        NEAR_SIT_ANSWERED.check(
            "whether a sit on a seat beside the avatar is answered",
            grid,
            &near.is_some_and(Attempt::answered),
        )?;
        crate::support::check(
            near.is_some_and(|near| near.seated.is_some()),
            "the avatar was never shown as a child of the seat it sat on",
        )?;
        crate::support::check(
            self.stood == Some(true),
            "standing up did not free the avatar from its seat",
        )?;
        if let Some((forward, rise)) = self.stood_at {
            check_length(
                "how far in front of its seat a standing avatar is put",
                grid,
                &STAND_FORWARD_M,
                forward,
            )?;
            check_length(
                "how far above its seat a standing avatar is put",
                grid,
                &STAND_RISE_M,
                rise,
            )?;
        }
        if let Some(Answer::Response(reply)) = near.map(|near| &near.answer) {
            NEAR_AUTOPILOT.check(
                "the AutoPilot flag of a sit beside the avatar",
                grid,
                &reply.autopilot,
            )?;
        }
        // A fake grid's seats all have a sit target, so its near sit is the
        // one a live grid's scripted seat is compared with.
        let (plain, target) = if is_fake(grid) {
            (None, near)
        } else {
            (near, self.target.as_ref())
        };
        if let Some(same) = plain.and_then(answer_is_where_it_sits) {
            PLAIN_ANSWER_IS_WHERE_IT_SITS.check(
                "whether a scriptless seat's answer states where the avatar sits",
                grid,
                &same,
            )?;
        }
        if let Some(same) = target.and_then(answer_is_where_it_sits) {
            TARGET_ANSWER_IS_WHERE_IT_SITS.check(
                "whether a sit target's answer states where the avatar sits",
                grid,
                &same,
            )?;
        }
        let beyond: Vec<bool> = self
            .far
            .iter()
            .filter(|(distance, _answered)| *distance > FAR_M)
            .map(|(_distance, answered)| *answered)
            .collect();
        if !beyond.is_empty() {
            FAR_PLAIN_ANSWERED.check(
                "whether a scriptless seat is answered from more than 12 m away",
                grid,
                &beyond.iter().all(|answered| *answered),
            )?;
        }
        if let Some(answered) = self.far_target {
            FAR_TARGET_ANSWERED.check(
                "whether a sit target is answered from 40 m away",
                grid,
                &answered,
            )?;
        }
        if let Some(seated) = self.seated_after_teleport {
            TELEPORT_UNSEATS.check(
                "whether a teleport within the region unseats the avatar",
                grid,
                &!seated,
            )?;
        }
        if let Some(unknown) = &self.unknown {
            UNKNOWN_ANSWER.check(
                "what a sit on an object the region does not have is answered with",
                grid,
                &unknown.answer.kind(),
            )?;
            let name = match &unknown.answer {
                Answer::Alert(name, _text) => name.as_str(),
                Answer::Response(_) | Answer::Nothing => "",
            };
            UNKNOWN_ALERT.check("the alert that refuses such a sit", grid, &name)?;
        }
        Ok(())
    }
}

/// The session-and-watch pair of the second avatar, where the grid shows two
/// residents to each other.
struct Second<'a> {
    /// Its session.
    session: &'a mut Session,
    /// What it was told.
    watch: Watch,
}

/// Walks the second avatar up to the seat if the flight to the build location
/// left it further off than a grid answers a sit from.
async fn come_near(
    second: &mut Second<'_>,
    seat_at: &Vector,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let from = second.watch.here()?.position;
    if ground_distance(&from, seat_at) > SECOND_WITHIN_M {
        // It came down from a flight nobody has answered for yet.
        second.session.send(Command::FinishAnimation).await?;
        let watch = &mut second.watch;
        let _stopped = steer_towards(
            second.session,
            from,
            seat_at,
            Gait::Walking,
            WALK_BUDGET,
            |event| watch.note(event),
        )
        .await?;
        let _rested = second
            .watch
            .observe(second.session, PAUSE, |_watch| false)
            .await?;
    }
    metrics.set(
        "second_stands_m_from_seat",
        f64::from(ground_distance(&second.watch.here()?.position, seat_at)),
    );
    Ok(())
}

/// The legs that need only the first avatar and a seat beside it.
async fn near_legs(
    session: &mut Session,
    watch: &mut Watch,
    second: Option<&mut Second<'_>>,
    seat: &Object,
    metrics: &mut Metrics,
    found: &mut Found,
) -> Result<(), TestFailure> {
    let seat_at = &seat.motion.position;
    // The first request, with nothing done about whatever the avatar's arrival
    // left it in; then the same after a FINISH_ANIM, and after a STAND_UP.
    let mut near = try_sit(session, watch, seat.full_id, Some(seat_at), ANSWER_BUDGET).await?;
    near.record("near", metrics);
    let mut freed_by = "nothing needed";
    if !near.answered() {
        session.send(Command::FinishAnimation).await?;
        tokio::time::sleep(PAUSE).await;
        near = try_sit(session, watch, seat.full_id, Some(seat_at), ANSWER_BUDGET).await?;
        near.record("near_after_finish", metrics);
        freed_by = "FINISH_ANIM";
    }
    if !near.answered() {
        session.send(Command::Stand).await?;
        tokio::time::sleep(PAUSE).await;
        near = try_sit(session, watch, seat.full_id, Some(seat_at), ANSWER_BUDGET).await?;
        near.record("near_after_stand", metrics);
        freed_by = "STAND_UP";
    }
    metrics.set(
        "near_answered_after",
        if near.answered() { freed_by } else { "never" },
    );
    // From here on the avatar behaves as a viewer does.
    watch.finish = true;
    let seated = near.seated.is_some();
    found.near = Some(near);
    if !seated {
        return Ok(());
    }

    // The second avatar asks for the seat the first is on.
    if let Some(second) = second {
        let _seen = second
            .watch
            .observe(second.session, PAUSE, |_watch| false)
            .await?;
        if let Some(observed) = &second.watch.other_last {
            metrics.set("observer_sees_seated", observed.seated());
            metrics.set("observer_sees_position", xyz(&observed.position));
        }
        let attempt = try_sit(
            second.session,
            &mut second.watch,
            seat.full_id,
            Some(seat_at),
            ANSWER_BUDGET,
        )
        .await?;
        attempt.record("second", metrics);
        if attempt.seated.is_some() {
            let stood = stand_up(second.session, &mut second.watch).await?;
            stood.record("second_stand", seat_at, metrics);
        }
    }

    let stood = stand_up(session, watch).await?;
    stood.record("stand", seat_at, metrics);
    found.stood = Some(stood.secs.is_some());
    let sat = found.near.as_ref().and_then(|near| near.seated.as_ref());
    if let (Some(sat), Some(stands)) = (sat, &stood.position) {
        // The seat is an unrotated cube, so where the avatar sat in the
        // region is the seat's position and the offset added up.
        let forward =
            (stands.x - seat_at.x - sat.position.x).hypot(stands.y - seat_at.y - sat.position.y);
        let rise = stands.z - seat_at.z - sat.position.z;
        metrics.set("stand_forward_m", f64::from(forward));
        metrics.set("stand_rise_m", f64::from(rise));
        found.stood_at = Some((forward, rise));
    }

    // The same seat, asked for at a point on its top face rather than at its
    // centre: whether the point clicked moves where the avatar is put.
    let clicked = try_sit_at(
        session,
        watch,
        seat.full_id,
        Some(seat_at),
        CLICKED,
        ANSWER_BUDGET,
    )
    .await?;
    clicked.record("clicked", metrics);
    if clicked.seated.is_some() {
        let stood = stand_up(session, watch).await?;
        stood.record("clicked_stand", seat_at, metrics);
    }

    // The ground, and the seat from there.
    let asked_at = watch.now();
    session.send(Command::SitOnGround).await?;
    let _quiet = watch.observe(session, SEAT_BUDGET, |_watch| false).await?;
    metrics.set("ground_animations", watch.animations_since(asked_at));
    metrics.set(
        "ground_updates",
        u32::try_from(
            watch
                .samples
                .iter()
                .filter(|sample| sample.at >= asked_at)
                .count(),
        )
        .unwrap_or(u32::MAX),
    );
    if let Some(sample) = watch.samples.last() {
        metrics.set("ground_position", xyz(&sample.position));
        metrics.set("ground_parent", sample.parent.0);
    }
    let from_ground = try_sit(session, watch, seat.full_id, Some(seat_at), ANSWER_BUDGET).await?;
    from_ground.record("from_ground", metrics);
    let stood = stand_up(session, watch).await?;
    stood.record("from_ground_stand", seat_at, metrics);
    Ok(())
}

/// The legs that move the avatar away from the seat, give the seat a sit
/// target and teleport the seated avatar: live grids only.
async fn far_legs(
    session: &mut Session,
    watch: &mut Watch,
    mut second: Option<&mut Second<'_>>,
    seat: &Object,
    metrics: &mut Metrics,
    found: &mut Found,
) -> Result<(), TestFailure> {
    let seat_at = &seat.motion.position;
    for distance in FAR_PLAIN_M {
        move_to(session, watch, seat_at, distance, Gait::Walking).await?;
        let leg = format!("far_{distance:.0}m");
        let attempt = try_sit(
            session,
            watch,
            seat.full_id,
            Some(seat_at),
            FAR_ANSWER_BUDGET,
        )
        .await?;
        attempt.record(&leg, metrics);
        if let Some(distance) = attempt.distance_m {
            found.far.push((distance, attempt.answered()));
        }
        if attempt.seated.is_some() {
            let stood = stand_up(session, watch).await?;
            stood.record(&format!("{leg}_stand"), seat_at, metrics);
        }
    }

    // The seat gets a sit target. The avatar is where the last far leg left
    // it; the camera goes back to the seat, which the script upload names.
    look_at(session, seat_at).await?;
    let compiled = plant_sit_target(session, seat).await?;
    metrics.set("sit_target_script_compiled", compiled);
    if !compiled {
        return Ok(());
    }
    let attempt = try_sit(session, watch, seat.full_id, Some(seat_at), ANSWER_BUDGET).await?;
    attempt.record("target", metrics);
    let on_target = attempt.seated.is_some();
    found.target = Some(attempt);

    // The second avatar asks for the taken target.
    if let Some(second) = second.as_deref_mut() {
        let attempt = try_sit(
            second.session,
            &mut second.watch,
            seat.full_id,
            Some(seat_at),
            ANSWER_BUDGET,
        )
        .await?;
        attempt.record("target_second", metrics);
        if attempt.seated.is_some() {
            let stood = stand_up(second.session, &mut second.watch).await?;
            stood.record("target_second_stand", seat_at, metrics);
        }
    }
    if on_target {
        let stood = stand_up(session, watch).await?;
        stood.record("target_stand", seat_at, metrics);
    }

    // The target from further off, and a teleport asked from the seat.
    move_to(session, watch, seat_at, FAR_TARGET_M, Gait::Flying).await?;
    let leg = format!("target_{FAR_TARGET_M:.0}m");
    let attempt = try_sit(session, watch, seat.full_id, Some(seat_at), ANSWER_BUDGET).await?;
    attempt.record(&leg, metrics);
    found.far_target = Some(attempt.answered());
    if attempt.seated.is_none() {
        return Ok(());
    }
    let Some(region_handle) = session.region_handle() else {
        return Ok(());
    };
    let asked_at = watch.now();
    let teleports = watch.teleports.len();
    session
        .send(Command::Teleport {
            region_handle,
            position: RegionCoordinates::new(seat_at.x, seat_at.y - TELEPORT_M, seat_at.z + 1.0),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let _answered = watch
        .observe(session, REPLY_TIMEOUT, |watch| {
            watch.teleports.len() > teleports
        })
        .await?;
    let _settled = watch.observe(session, SEAT_BUDGET, |_watch| false).await?;
    metrics.set(
        "teleport_seated_answer",
        watch
            .teleports
            .get(teleports)
            .map_or_else(|| "nothing".to_owned(), |(_at, what)| what.clone()),
    );
    metrics.set(
        "teleport_seated_animations",
        watch.animations_since(asked_at),
    );
    if let Some(sample) = watch.samples.last() {
        found.seated_after_teleport = Some(sample.seated());
        metrics.set("teleport_seated_still_seated", sample.seated());
        metrics.set("teleport_seated_position", xyz(&sample.position));
    }
    if let Some(second) = second {
        let _seen = second
            .watch
            .observe(second.session, PAUSE, |_watch| false)
            .await?;
        if let Some(observed) = &second.watch.other_last {
            metrics.set("teleport_observer_sees_seated", observed.seated());
        }
    }
    if watch.samples.last().is_some_and(Sample::seated) {
        let stood = stand_up(session, watch).await?;
        stood.record("teleport_stand", seat_at, metrics);
    }
    Ok(())
}

/// What the setup hands the legs.
struct Setup {
    /// The seat, as the sitter's own stream showed it.
    seat: Object,
    /// The first avatar, and where it stands.
    sitter: (Uuid, Vector),
    /// The second avatar and where it stands, where there is one in view.
    second: Option<(Uuid, Option<Vector>)>,
    /// An object of a neighbouring region that came into view, if one did.
    neighbour_object: Option<ObjectKey>,
}

/// Brings the avatars into place and rezzes the seat beside the first.
async fn set_up(ctx: &mut TestContext, guard: &mut DeleteOnDrop) -> Result<Setup, TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let sitter = ctx.primary();
    let sitter_id = sitter
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let folders = sitter
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _other => None,
        })
        .await?;
    guard.trash = trash_folder(&folders);
    sitter.wait_for_region(REGION_TIMEOUT).await?;
    let settled = settle_scene_with_avatar(
        sitter,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        SETTLE_IDLE,
    )
    .await?;
    let sitter_at = settled.avatar.clone().ok_or_else(|| {
        TestFailure::Assertion("the sitter never appeared in its own object stream".to_owned())
    })?;
    // The second avatar comes to the same spot before the seat exists, so
    // that settling its scene does not swallow the seat's arrival.
    let mut second_avatar = None;
    if !is_fake(grid)
        && let Some(second) = ctx.secondary()
    {
        second.wait_for_region(REGION_TIMEOUT).await?;
        let settled =
            settle_scene_with_avatar(second, grid, build_position, SETTLE_WINDOW, SETTLE_IDLE)
                .await?;
        second_avatar = second
            .agent_id()
            .map(|agent| (agent.uuid(), settled.avatar));
    }
    // On the ground the avatar stands on.
    let seat_height = sitter_at.z - SEAT_BELOW_AVATAR_M;
    let sitter = ctx.primary();
    sitter
        .send(Command::RezObject {
            shape: PrimShape::cube(Vector {
                x: sitter_at.x + SEAT_OFFSET_M,
                y: sitter_at.y,
                z: seat_height,
            }),
            group_id: None,
        })
        .await?;
    let seat = wait_for_own_new_object(sitter, &settled.seen, STEP_TIMEOUT)
        .await?
        .map_err(|reason| TestFailure::Assertion(format!("the seat was not rezzed: {reason}")))?;
    guard.seat = Some(seat.scoped_id());
    if second_avatar.is_some()
        && let Some(second) = ctx.secondary()
    {
        look_at(second, &seat.motion.position).await?;
        let _seen = sight_object(second, seat.full_id, STEP_TIMEOUT).await?;
    }
    Ok(Setup {
        seat,
        sitter: (sitter_id, sitter_at),
        second: second_avatar,
        neighbour_object: settled.neighbour_object,
    })
}

/// Runs every leg against the seat.
async fn drive(ctx: &mut TestContext, setup: &Setup, found: &mut Found) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let mut metrics = Metrics::default();
    let (sitter, sitter_at) = setup.sitter.clone();
    let mut watch = Watch::new(sitter, None, Some(sitter_at));
    let outcome = async {
        let (session, second_session) = match &setup.second {
            Some(_second) => {
                let (first, second) = ctx.primary_and_secondary().ok_or_else(|| {
                    TestFailure::Assertion("the second avatar's session is gone".to_owned())
                })?;
                (first, Some(second))
            }
            None => (ctx.primary(), None),
        };
        let mut second = match (second_session, setup.second.clone()) {
            (Some(session), Some((agent, standing))) => Some(Second {
                session,
                watch: Watch::new(agent, Some(sitter), standing),
            }),
            _none => None,
        };
        if let Some(second) = second.as_mut() {
            second.watch.finish = true;
            come_near(second, &setup.seat.motion.position, &mut metrics).await?;
        }
        // Where the avatar stands, before anything is asked of it.
        let _seen = watch.observe(session, PAUSE, |_watch| false).await?;
        near_legs(
            session,
            &mut watch,
            second.as_mut(),
            &setup.seat,
            &mut metrics,
            found,
        )
        .await?;
        if !is_fake(grid) && found.near.as_ref().is_some_and(Attempt::answered) {
            far_legs(
                session,
                &mut watch,
                second.as_mut(),
                &setup.seat,
                &mut metrics,
                found,
            )
            .await?;
        }
        // A sit on an object the region does not have.
        let unknown = try_sit(
            session,
            &mut watch,
            ObjectKey::from(Uuid::new_v4()),
            None,
            FAR_ANSWER_BUDGET,
        )
        .await?;
        unknown.record("unknown", &mut metrics);
        found.unknown = Some(unknown);
        // And on one a neighbouring region has, where one is in view.
        if let Some(neighbour) = setup.neighbour_object {
            let attempt = try_sit(session, &mut watch, neighbour, None, FAR_ANSWER_BUDGET).await?;
            attempt.record("neighbour", &mut metrics);
        }
        Ok::<(), TestFailure>(())
    }
    .await;
    metrics.set(
        "alerts",
        watch
            .alerts
            .iter()
            .map(|(at, name, text)| format!("{at:.1}:[{name}] {text}"))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    ctx.metrics().merge(metrics);
    outcome
}

/// Deletes the seat from the session that rezzed it and waits for it to go.
/// The session has known the seat since the rez, so there is nothing to
/// sight first: a resting object is not sent again.
async fn delete_seat(
    sitter: &mut Session,
    seat: &Object,
    trash: sl_client_tokio::InventoryFolderKey,
) -> Result<(), TestFailure> {
    let scoped = seat.scoped_id();
    look_at(sitter, &seat.motion.position).await?;
    sitter.send(delete_command(scoped, trash)).await?;
    sitter
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectRemoved { local_id, .. } if *local_id == scoped => Some(()),
            _other => None,
        })
        .await
}

/// Sits on a seat beside the avatar, from afar, on a sit target and on the
/// ground, and records every answer.
#[derive(Debug)]
pub struct SitStand;

impl GridTest for SitStand {
    fn name(&self) -> &'static str {
        "sit-stand"
    }

    fn description(&self) -> &'static str {
        "Sit near, far, on a sit target, on a taken seat and on the ground; stand; teleport seated"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut guard = DeleteOnDrop {
                commander: ctx.primary().commander(),
                trash: None,
                seat: None,
            };
            let setup = set_up(ctx, &mut guard).await?;
            let mut found = Found::default();
            let outcome = drive(ctx, &setup, &mut found).await;
            // Whatever a leg ran into, the avatar is not left on the seat.
            let stood = ctx.primary().send(Command::Stand).await;
            guard.seat = None;
            let deleted = match guard.trash {
                Some(trash) => delete_seat(ctx.primary(), &setup.seat, trash).await,
                None => Ok(()),
            };

            outcome?;
            stood?;
            deleted?;
            found.check(grid)
        })
    }
}
