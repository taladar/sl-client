//! The avatar controller: where an agent's own avatar goes when its viewer
//! holds a key.
//!
//! A viewer does not move its avatar. It states which controls are held in its
//! `AgentUpdate`s — forward, back, sideways, up, down, fly — with the way the
//! body faces, and the simulator decides where the avatar goes and says so in
//! the avatar's own object updates. This is that decision, run once a
//! [heartbeat](crate::heartbeat) tick for every root agent in the region.
//!
//! It is a kinematic character, not a physics solver: it walks, runs,
//! crouches, flies, climbs, descends, jumps and falls, keeps a walking avatar
//! on the region's heightfield and inside the region, and stops when the keys
//! are let go. What it does not do — collide with prims, be pushed, stand on a
//! floor that is not the ground, walk over a region border — is the roadmap's
//! `server-world-collision-and-physics` and the crossing tasks.
//!
//! Each tick advances by exactly the heartbeat's step, never by the time
//! measured since the last, so two runs of one scenario put the avatar in the
//! same place.
//!
//! What the two live grids do was measured by the `agent-movement` conformance
//! case (2026-10-08, `book/src/gridspec/movement.md`) and is set per flavour
//! in [`MovementPolicy`]:
//!
//! | behaviour | Second Life | OpenSim |
//! | --- | --- | --- |
//! | walk / run / crouched walk | 3.20 / 5.13 / 2.05 m/s | 3.145 / 5.10 / 3.145 m/s |
//! | level flight / climb / descent | 16.0 / 16.0 / 22.8 m/s | 12.58 / 16.36 / 16.35 m/s |
//! | a jump | 8.1 m/s up, 4 m high | 9.86 m/s up, 5.6 m high |
//! | the fastest fall | 52.8 m/s | 50.1 m/s |
//! | coming to rest once a walk is let go | 0.6 s (a metre on) | 0.3 s (0.6 m on) |
//! | a pre-jump and a hard landing | held until the viewer sends `FINISH_ANIM` | timed: off the ground at once, standing up after 1.1 s |
//! | updates of a walking avatar | every tick | one every two seconds |
//! | updates of a steady flight or climb | none | about three a second |
//! | updates of an avatar at rest | about one and a half a second | none |
//!
//! Whatever the cadence, a change of motion — starting, stopping, turning,
//! leaving or reaching the ground — is reported in the tick it happens, as
//! both grids report it.
//!
//! The animation set follows the motion: the set's default is the
//! locomotion state (`stand`, `walk`, `run`, `crouch`, `crouchwalk`,
//! `pre_jump`, `jump`, `land`, `standup`, `hover`, `hover_up`, `hover_down`,
//! `fly`, `falldown`), stated to the agent each time it changes, as OpenSim
//! states exactly one built-in at a time.

use core::time::Duration;
use std::time::Instant;

use sl_proto::{ControlFlags, MovementMode, RegionLocalParcelId, ServerEvent};
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::RegionCoordinates;
use uuid::Uuid;

use crate::driver::SimState;
use crate::world::{AVATAR_CENTRE_ABOVE_GROUND_M, RegionChange};

/// How a grid moves an avatar, where the live grids disagree.
#[expect(
    clippy::module_name_repetitions,
    reason = "read at its use sites as `ImitatedGrid::movement_policy() -> MovementPolicy`, \
              beside the other per-flavour policies"
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementPolicy {
    /// A walk, metres a second.
    pub walk_speed: f32,
    /// A walk with always-run on, metres a second.
    pub run_speed: f32,
    /// A walk while crouching, metres a second: Second Life slows it, OpenSim
    /// changes only the animation.
    pub crouch_walk_speed: f32,
    /// Level flight, metres a second.
    pub fly_speed: f32,
    /// A climb in flight, metres a second.
    pub climb_speed: f32,
    /// A descent in flight, metres a second.
    pub descent_speed: f32,
    /// How fast a jump leaves the ground, metres a second upwards.
    pub jump_speed: f32,
    /// What pulls an avatar that is not flying back down, metres a second
    /// squared: what makes the measured jump as high as it was.
    pub gravity: f32,
    /// The fastest an avatar falls, metres a second.
    pub terminal_fall_speed: f32,
    /// How long a walk or a run takes to come to rest once its keys are let
    /// go.
    pub ground_stop: Duration,
    /// How long a flight takes to come to rest once its keys are let go.
    pub air_stop: Duration,
    /// How fast an avatar has to come down for its landing to be a hard one,
    /// which stands it up before it can walk again, metres a second.
    pub hard_landing_speed: f32,
    /// How long a hard landing keeps the avatar where it landed, when the
    /// grid times it; [`None`] holds it until the viewer reports the
    /// animation finished (`FINISH_ANIM`), and does the same to a pre-jump,
    /// as Second Life does.
    pub timed_holds: Option<Duration>,
    /// How long a soft landing's `land` stays in the animation set.
    pub soft_landing: Duration,
    /// How often an unchanging motion is reported.
    pub cadence: UpdateCadence,
}

impl MovementPolicy {
    /// Whether the grid waits for the viewer's `FINISH_ANIM` before it lets a
    /// held avatar go.
    #[must_use]
    pub const fn holds_for_finish(&self) -> bool {
        self.timed_holds.is_none()
    }
}

/// How often a grid reports an avatar whose motion is not changing; [`None`]
/// is never.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateCadence {
    /// An avatar walking or running at a steady pace.
    pub walking: Option<Duration>,
    /// An avatar flying, climbing or descending at a steady speed.
    pub airborne: Option<Duration>,
    /// An avatar standing still.
    pub at_rest: Option<Duration>,
}

/// What the avatar is doing, which is also the animation set's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    /// Standing on the ground.
    Standing,
    /// Walking.
    Walking,
    /// Running.
    Running,
    /// Crouching where it stands.
    Crouching,
    /// Walking while crouched.
    CrouchWalking,
    /// About to jump.
    PreJump,
    /// On the way up from a jump.
    Jumping,
    /// Falling, not flying.
    Falling,
    /// Just down from a jump or a short drop.
    Landing,
    /// Getting up from a hard landing.
    StandingUp,
    /// Flying in place.
    Hovering,
    /// Flying upwards.
    HoverUp,
    /// Flying downwards.
    HoverDown,
    /// Flying along.
    Flying,
    /// Sitting on the ground.
    SittingOnGround,
    /// Sitting on a prim.
    SittingOnPrim,
}

impl Posture {
    /// The movement state as `llGetAnimation` names it.
    #[must_use]
    pub const fn state_name(self) -> &'static str {
        match self {
            Self::Standing => "Standing",
            Self::Walking => "Walking",
            Self::Running => "Running",
            Self::Crouching => "Crouching",
            Self::CrouchWalking => "CrouchWalking",
            Self::PreJump => "PreJumping",
            Self::Jumping => "Jumping",
            Self::Falling => "Falling Down",
            Self::Landing => "Landing",
            Self::StandingUp => "Standing Up",
            Self::Hovering => "Hovering",
            Self::HoverUp => "Hovering Up",
            Self::HoverDown => "Hovering Down",
            Self::Flying => "Flying",
            Self::SittingOnGround => "Sitting on Ground",
            Self::SittingOnPrim => "Sitting",
        }
    }

    /// The `AGENT_*` bits of `llGetAgentInfo` the posture sets: flying,
    /// walking, in the air, crouching, sitting on the ground.
    #[must_use]
    pub const fn agent_info(self) -> i32 {
        /// `AGENT_FLYING`.
        const FLYING: i32 = 0x1;
        /// `AGENT_SITTING`.
        const SITTING: i32 = 0x10;
        /// `AGENT_WALKING`.
        const WALKING: i32 = 0x80;
        /// `AGENT_IN_AIR`.
        const IN_AIR: i32 = 0x100;
        /// `AGENT_CROUCHING`.
        const CROUCHING: i32 = 0x400;
        match self {
            Self::Hovering | Self::HoverUp | Self::HoverDown | Self::Flying => FLYING | IN_AIR,
            Self::Walking | Self::Running => WALKING,
            Self::CrouchWalking => WALKING | CROUCHING,
            Self::Crouching => CROUCHING,
            Self::PreJump | Self::Jumping | Self::Falling => IN_AIR,
            Self::SittingOnGround | Self::SittingOnPrim => SITTING,
            Self::Standing | Self::Landing | Self::StandingUp => 0,
        }
    }

    /// The built-in animation's name (`sl_anim`'s short name for it).
    #[must_use]
    pub const fn animation_name(self) -> &'static str {
        match self {
            Self::Standing => "stand",
            Self::Walking => "walk",
            Self::Running => "run",
            Self::Crouching => "crouch",
            Self::CrouchWalking => "crouchwalk",
            Self::PreJump => "pre_jump",
            Self::Jumping => "jump",
            Self::Falling => "falldown",
            Self::Landing => "land",
            Self::StandingUp => "standup",
            Self::Hovering => "hover",
            Self::HoverUp => "hover_up",
            Self::HoverDown => "hover_down",
            Self::Flying => "fly",
            Self::SittingOnGround => "sit_ground_constrained",
            Self::SittingOnPrim => "sit",
        }
    }

    /// The built-in animation's asset id.
    #[must_use]
    pub const fn animation(self) -> Uuid {
        match self {
            Self::Standing => crate::animations::STAND,
            Self::Walking => uuid::uuid!("6ed24bd8-91aa-4b12-ccc7-c97c857ab4e0"),
            Self::Running => uuid::uuid!("05ddbff8-aaa9-92a1-2b74-8fe77a29b445"),
            Self::Crouching => uuid::uuid!("201f3fdf-cb1f-dbec-201f-7333e328ae7c"),
            Self::CrouchWalking => uuid::uuid!("47f5f6fb-22e5-ae44-f871-73aaaf4a6022"),
            Self::PreJump => uuid::uuid!("7a4e87fe-de39-6fcb-6223-024b00893244"),
            Self::Jumping => uuid::uuid!("2305bd75-1ca9-b03b-1faa-b176b8a8c49e"),
            Self::Falling => uuid::uuid!("666307d9-a860-572d-6fd4-c3ab8865c094"),
            Self::Landing => uuid::uuid!("7a17b059-12b2-41b1-570a-186368b6aa6f"),
            Self::StandingUp => uuid::uuid!("3da1d753-028a-5446-24f3-9c9b856d9422"),
            Self::Hovering => uuid::uuid!("4ae8016b-31b9-03bb-c401-b1ea941db41d"),
            Self::HoverUp => uuid::uuid!("62c5de58-cb33-5743-3d07-9e4cd4352864"),
            Self::HoverDown => uuid::uuid!("20f063ea-8306-2562-0b07-5c853b37b31e"),
            Self::Flying => uuid::uuid!("aec4610c-757f-bc4e-c092-c6e9caf18daf"),
            Self::SittingOnGround => uuid::uuid!("1a2bd58e-87ff-0df8-0b4c-53e047b0bb6e"),
            Self::SittingOnPrim => uuid::uuid!("1a5fe8ac-a804-8a5d-7cbd-56bd83184568"),
        }
    }

    /// Whether the avatar is in the air in this posture.
    const fn airborne(self) -> bool {
        matches!(
            self,
            Self::Jumping
                | Self::Falling
                | Self::Hovering
                | Self::HoverUp
                | Self::HoverDown
                | Self::Flying
        )
    }
}

/// The region's ground, for the controller: the height at a point.
pub trait Ground {
    /// The ground height, metres, at region-local (`x`, `y`).
    fn height_at(&self, x: f32, y: f32) -> f32;
}

impl Ground for crate::terrain::TerrainFixture {
    fn height_at(&self, x: f32, y: f32) -> f32 {
        Self::height_at(self, x, y)
    }
}

/// How far in from a region edge the avatar is kept, metres: it walks up to
/// the border and no further (walking over it is a crossing, which this
/// controller does not start).
const EDGE_MARGIN_M: f32 = 0.1;

/// The width of a standard region, metres.
const REGION_WIDTH_M: f32 = 256.0;

/// Below this the avatar is at rest, metres a second.
const AT_REST_M_PER_S: f32 = 0.01;

/// How much a velocity has to change to be reported at once, metres a second.
const VELOCITY_CHANGE_M_PER_S: f32 = 0.05;

/// How much a rotation component has to change to be reported at once.
const ROTATION_CHANGE: f32 = 0.001;

/// How close to the ground the avatar's centre has to be to stand on it,
/// metres.
const ON_GROUND_M: f32 = 0.01;

/// How far either side of a point the ground's slope is read, metres.
const SLOPE_SPAN_M: f32 = 0.5;

/// What the last update of the avatar said.
#[derive(Debug, Clone, PartialEq)]
struct Reported {
    /// Its velocity.
    velocity: Vector,
    /// Its rotation.
    rotation: Rotation,
    /// Whether it was in the air.
    airborne: bool,
}

/// One agent's movement state: what its viewer holds, and how its avatar is
/// moving.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    /// The controls the viewer last stated.
    controls: ControlFlags,
    /// The way the viewer last said the body faces.
    body: Rotation,
    /// Whether a walk is a run.
    mode: MovementMode,
    /// Whether a `FINISH_ANIM` has arrived since the last tick.
    finish_owed: bool,
    /// Whether the jump key has been let go since the last jump, so holding
    /// it is one jump, not a hop each landing.
    jump_armed: bool,
    /// Whether the jump key went down since the last tick while armed: a tap
    /// let go of before the tick is still a jump.
    jump_tapped: bool,
    /// The avatar's velocity.
    velocity: Vector,
    /// What the avatar is doing.
    posture: Posture,
    /// How long the current posture has lasted.
    in_posture: Duration,
    /// How fast a walk or flight the keys let go of is braking, metres a
    /// second squared; zero when nothing is braking.
    braking: f32,
    /// What the last update said, [`None`] before the first.
    reported: Option<Reported>,
    /// How long since the last update.
    since_reported: Duration,
    /// The parcel the agent was last told it stands on.
    parcel: Option<RegionLocalParcelId>,
    /// How many unsolicited parcel pushes this session has had after its
    /// arrival's.
    pushes: i32,
    /// Where the viewer last said its camera is, and its axes.
    camera: Option<sl_proto::Camera>,
}

impl Default for Motion {
    fn default() -> Self {
        Self::arriving(None)
    }
}

/// Where one tick left the avatar.
#[derive(Debug, Clone, PartialEq)]
pub struct Stepped {
    /// Its position, the centre of the avatar.
    pub position: Vector,
    /// Whether the posture changed this tick, so the animation set is to be
    /// stated again.
    pub posture_changed: bool,
    /// The update to send, when one is due.
    pub update: Option<sl_proto::ObjectMotion>,
}

impl Motion {
    /// The controls the viewer last stated.
    #[must_use]
    pub const fn controls(&self) -> ControlFlags {
        self.controls
    }

    /// Where the viewer last said its camera is, and its axes; [`None`]
    /// before its first `AgentUpdate`.
    #[must_use]
    pub const fn camera(&self) -> Option<&sl_proto::Camera> {
        self.camera.as_ref()
    }

    /// The state of an agent that has just arrived standing on `parcel`, which
    /// the arrival already told it of.
    #[must_use]
    pub const fn arriving(parcel: Option<RegionLocalParcelId>) -> Self {
        Self {
            controls: ControlFlags::empty(),
            body: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
            mode: MovementMode::Walk,
            finish_owed: false,
            jump_armed: true,
            jump_tapped: false,
            velocity: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            posture: Posture::Standing,
            in_posture: Duration::ZERO,
            braking: 0.0,
            reported: None,
            since_reported: Duration::ZERO,
            parcel,
            pushes: 0,
            camera: None,
        }
    }

    /// What the avatar is doing.
    #[must_use]
    pub const fn posture(&self) -> Posture {
        self.posture
    }

    /// The avatar's velocity.
    #[must_use]
    pub const fn velocity(&self) -> &Vector {
        &self.velocity
    }

    /// Folds in what a viewer said about its controls: an `AgentUpdate` or a
    /// `SetAlwaysRun`. Anything else is not this module's, and `false` says
    /// so.
    pub fn hear(&mut self, event: &ServerEvent) -> bool {
        match event {
            ServerEvent::AgentUpdate(update) => {
                if update.controls.contains(ControlFlags::FINISH_ANIM) {
                    tracing::debug!(posture = ?self.posture, "the viewer reports an animation finished");
                    self.finish_owed = true;
                }
                self.controls = update.controls;
                self.body = update.body_rotation.clone();
                self.camera = Some(update.camera.clone());
                if !self.controls.contains(ControlFlags::UP_POS) {
                    self.jump_armed = true;
                } else if self.jump_armed {
                    self.jump_tapped = true;
                }
                true
            }
            ServerEvent::SetAlwaysRun { mode } => {
                self.mode = *mode;
                true
            }
            _other => false,
        }
    }

    /// Holds the avatar still and sits it: what a seat does to it. Returns
    /// whether the posture changed, so the animation set is to be stated
    /// again: both grids put a seated avatar in the built-in `sit`
    /// (`avatar/sit`, `avatar/sit-animation`).
    pub fn halt(&mut self) -> bool {
        self.velocity = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        self.braking = 0.0;
        self.set_posture(Posture::SittingOnPrim)
    }

    /// Stands up an avatar its seat let go of, and says whether it was
    /// sitting on one.
    pub fn rise(&mut self) -> bool {
        self.posture == Posture::SittingOnPrim && self.set_posture(Posture::Standing)
    }

    /// Moves the posture on, restarting its clock when it changes, and says
    /// whether it did.
    fn set_posture(&mut self, posture: Posture) -> bool {
        if self.posture == posture {
            return false;
        }
        self.posture = posture;
        self.in_posture = Duration::ZERO;
        true
    }

    /// The direction the held keys ask for, flat on the ground, of length one
    /// or zero.
    fn wanted_direction(&self) -> (f32, f32) {
        let held = |flags: &[ControlFlags]| flags.iter().any(|flag| self.controls.contains(*flag));
        let axis = |positive: &[ControlFlags], negative: &[ControlFlags]| {
            f32::from(u8::from(held(positive))) - f32::from(u8::from(held(negative)))
        };
        let along = axis(
            &[ControlFlags::AT_POS, ControlFlags::NUDGE_AT_POS],
            &[ControlFlags::AT_NEG, ControlFlags::NUDGE_AT_NEG],
        );
        let across = axis(
            &[ControlFlags::LEFT_POS, ControlFlags::NUDGE_LEFT_POS],
            &[ControlFlags::LEFT_NEG, ControlFlags::NUDGE_LEFT_NEG],
        );
        let Rotation { x, y, z, s } = self.body;
        // The body's forward (+X) and left (+Y), laid flat.
        let forward = flat(
            2.0f32.mul_add(-y.mul_add(y, z * z), 1.0),
            2.0 * x.mul_add(y, s * z),
        );
        let left = flat(
            2.0 * x.mul_add(y, -(s * z)),
            2.0f32.mul_add(-x.mul_add(x, z * z), 1.0),
        );
        let wanted = (
            forward.0.mul_add(along, left.0 * across),
            forward.1.mul_add(along, left.1 * across),
        );
        flat(wanted.0, wanted.1)
    }

    /// Advances the avatar standing at `position` by one tick of `step` over
    /// `ground`.
    pub fn step(
        &mut self,
        policy: &MovementPolicy,
        ground: &impl Ground,
        position: &Vector,
        step: Duration,
    ) -> Stepped {
        let dt = step.as_secs_f32();
        let before = self.posture;
        self.in_posture = self.in_posture.saturating_add(step);
        let finished = core::mem::take(&mut self.finish_owed);
        let tapped = core::mem::take(&mut self.jump_tapped);
        // A tick that steps the avatar finds it on no seat: one it sat on has
        // let it go, and it stands where the stand put it.
        let _rose = self.rise();
        let mut at = position.clone();
        let floor = ground.height_at(at.x, at.y) + AVATAR_CENTRE_ABOVE_GROUND_M;
        let flying = self.controls.contains(ControlFlags::FLY);
        let (dir_x, dir_y) = self.wanted_direction();
        let steering = dir_x != 0.0 || dir_y != 0.0;
        if self.controls.contains(ControlFlags::STOP) {
            self.velocity.x = 0.0;
            self.velocity.y = 0.0;
            self.braking = 0.0;
        }

        if self.controls.contains(ControlFlags::SIT_ON_GROUND) && !flying {
            let _changed = self.set_posture(Posture::SittingOnGround);
        }
        if self.posture == Posture::SittingOnGround
            && (self.controls.contains(ControlFlags::STAND_UP) || flying)
        {
            let _changed = self.set_posture(Posture::Standing);
        }

        if flying {
            self.fly(policy, (dir_x, dir_y), dt);
        } else if self.posture.airborne() || at.z > floor + ON_GROUND_M {
            self.fall(policy, dt);
        } else {
            self.walk(policy, (dir_x, dir_y), steering, (finished, tapped), dt);
        }

        at.x = self.velocity.x.mul_add(dt, at.x);
        at.y = self.velocity.y.mul_add(dt, at.y);
        at.z = self.velocity.z.mul_add(dt, at.z);
        // The region's edges stop the avatar: a border is crossed, not walked
        // through, and this controller crosses nothing.
        let inside = EDGE_MARGIN_M..=(REGION_WIDTH_M - EDGE_MARGIN_M);
        if !inside.contains(&at.x) {
            at.x = at.x.clamp(*inside.start(), *inside.end());
            self.velocity.x = 0.0;
        }
        if !inside.contains(&at.y) {
            at.y = at.y.clamp(*inside.start(), *inside.end());
            self.velocity.y = 0.0;
        }
        let floor = ground.height_at(at.x, at.y) + AVATAR_CENTRE_ABOVE_GROUND_M;
        let grounded = !self.posture.airborne() && !flying;
        if grounded || at.z < floor {
            // On the ground, or come down to it this tick.
            if self.posture.airborne() {
                self.land(policy, flying);
            }
            at.z = floor;
            self.velocity.z = 0.0;
        }

        let posture_changed = self.posture != before;
        let update = self.report(policy, ground, &at, step);
        Stepped {
            position: at,
            posture_changed,
            update,
        }
    }

    /// One tick of flight.
    fn fly(&mut self, policy: &MovementPolicy, (dir_x, dir_y): (f32, f32), dt: f32) {
        let climb = self.controls.contains(ControlFlags::UP_POS)
            || self.controls.contains(ControlFlags::NUDGE_UP_POS);
        let descend = self.controls.contains(ControlFlags::UP_NEG)
            || self.controls.contains(ControlFlags::NUDGE_UP_NEG);
        if dir_x != 0.0 || dir_y != 0.0 {
            self.velocity.x = dir_x * policy.fly_speed;
            self.velocity.y = dir_y * policy.fly_speed;
            self.braking = 0.0;
        } else {
            self.brake(policy.air_stop, dt);
        }
        self.velocity.z = match (climb, descend) {
            (true, false) => policy.climb_speed,
            (false, true) => -policy.descent_speed,
            _neither => 0.0,
        };
        let posture = if self.velocity.z > 0.0 {
            Posture::HoverUp
        } else if self.velocity.z < 0.0 {
            Posture::HoverDown
        } else if ground_speed(&self.velocity) > AT_REST_M_PER_S {
            Posture::Flying
        } else {
            Posture::Hovering
        };
        let _changed = self.set_posture(posture);
    }

    /// One tick of falling, or of a jump on its way up.
    fn fall(&mut self, policy: &MovementPolicy, dt: f32) {
        self.velocity.z = (-policy.gravity)
            .mul_add(dt, self.velocity.z)
            .max(-policy.terminal_fall_speed);
        // Over the top of a jump the avatar is still in its jump.
        let posture = if self.posture == Posture::Jumping {
            Posture::Jumping
        } else {
            Posture::Falling
        };
        let _changed = self.set_posture(posture);
    }

    /// The avatar comes down: softly, from a jump or a short drop, or hard.
    fn land(&mut self, policy: &MovementPolicy, flying: bool) {
        let speed = -self.velocity.z;
        self.velocity.x = 0.0;
        self.velocity.y = 0.0;
        self.braking = 0.0;
        let posture = if flying {
            Posture::Hovering
        } else if self.posture == Posture::Jumping || speed < policy.hard_landing_speed {
            Posture::Landing
        } else {
            Posture::StandingUp
        };
        let _changed = self.set_posture(posture);
    }

    /// One tick on the ground.
    fn walk(
        &mut self,
        policy: &MovementPolicy,
        (dir_x, dir_y): (f32, f32),
        steering: bool,
        (finished, tapped): (bool, bool),
        dt: f32,
    ) {
        self.velocity.z = 0.0;
        match self.posture {
            Posture::SittingOnGround => {
                self.velocity.x = 0.0;
                self.velocity.y = 0.0;
                return;
            }
            // A hard landing keeps the avatar where it came down until it is
            // up again: until the viewer says so, or for the grid's own time.
            Posture::StandingUp => {
                let up = policy
                    .timed_holds
                    .map_or(finished, |hold| self.in_posture >= hold);
                if !up {
                    self.velocity.x = 0.0;
                    self.velocity.y = 0.0;
                    return;
                }
                let _changed = self.set_posture(Posture::Standing);
            }
            // A pre-jump leaves the ground when the viewer says it has played
            // out, or at once on a grid that does not wait.
            Posture::PreJump => {
                if policy.holds_for_finish() && !finished {
                    self.velocity.x = 0.0;
                    self.velocity.y = 0.0;
                    return;
                }
                self.velocity.z = policy.jump_speed;
                let _changed = self.set_posture(Posture::Jumping);
                return;
            }
            Posture::Landing if self.in_posture >= policy.soft_landing => {
                let _changed = self.set_posture(Posture::Standing);
            }
            _other => {}
        }
        let crouched = self.controls.contains(ControlFlags::UP_NEG);
        let jump = (self.controls.contains(ControlFlags::UP_POS) || tapped) && self.jump_armed;
        if jump {
            self.jump_armed = false;
            self.velocity.x = 0.0;
            self.velocity.y = 0.0;
            self.braking = 0.0;
            // Off the ground in a later tick: the next one where the grid
            // does not wait (the 0.05 to 0.09 s OpenSim took, rounded up to a
            // whole step), the one after the viewer's `FINISH_ANIM` where it
            // does.
            let _changed = self.set_posture(Posture::PreJump);
            return;
        }
        let speed = if crouched {
            policy.crouch_walk_speed
        } else if self.mode.is_always_run() {
            policy.run_speed
        } else {
            policy.walk_speed
        };
        if steering {
            self.velocity.x = dir_x * speed;
            self.velocity.y = dir_y * speed;
            self.braking = 0.0;
        } else {
            self.brake(policy.ground_stop, dt);
        }
        let moving = ground_speed(&self.velocity) > AT_REST_M_PER_S;
        let landing = self.posture == Posture::Landing;
        let posture = match (moving && steering, crouched) {
            (true, true) => Posture::CrouchWalking,
            (true, false) if self.mode.is_always_run() => Posture::Running,
            (true, false) => Posture::Walking,
            (false, true) => Posture::Crouching,
            (false, false) if landing => Posture::Landing,
            (false, false) => Posture::Standing,
        };
        let _changed = self.set_posture(posture);
    }

    /// Slows a horizontal motion whose keys were let go to a stop over
    /// `stop`, at the rate it was going when they were.
    fn brake(&mut self, stop: Duration, dt: f32) {
        let speed = ground_speed(&self.velocity);
        if speed <= AT_REST_M_PER_S {
            self.velocity.x = 0.0;
            self.velocity.y = 0.0;
            self.braking = 0.0;
            return;
        }
        if self.braking <= 0.0 {
            let secs = stop.as_secs_f32();
            self.braking = if secs > 0.0 {
                speed / secs
            } else {
                f32::INFINITY
            };
        }
        let slower = self.braking.mul_add(-dt, speed).max(0.0);
        let scale = slower / speed;
        self.velocity.x *= scale;
        self.velocity.y *= scale;
        if slower <= 0.0 {
            self.braking = 0.0;
        }
    }

    /// The update the tick owes the viewer, if any: at once for a change of
    /// motion, otherwise at the grid's cadence for what the avatar is doing.
    fn report(
        &mut self,
        policy: &MovementPolicy,
        ground: &impl Ground,
        at: &Vector,
        step: Duration,
    ) -> Option<sl_proto::ObjectMotion> {
        self.since_reported = self.since_reported.saturating_add(step);
        let airborne = self.posture.airborne();
        let now = Reported {
            velocity: self.velocity.clone(),
            rotation: self.body.clone(),
            airborne,
        };
        let changed = self
            .reported
            .as_ref()
            .is_none_or(|last| differs(last, &now));
        let moving = speed(&self.velocity) > AT_REST_M_PER_S;
        let period = if !moving {
            policy.cadence.at_rest
        } else if airborne {
            policy.cadence.airborne
        } else {
            policy.cadence.walking
        };
        let due = period.is_some_and(|period| self.since_reported >= period);
        if !changed && !due {
            return None;
        }
        self.since_reported = Duration::ZERO;
        self.reported = Some(now);
        Some(sl_proto::ObjectMotion {
            position: at.clone(),
            velocity: self.velocity.clone(),
            acceleration: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: self.body.clone(),
            angular_velocity: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            collision_plane: Some(if airborne {
                [0.0, 0.0, 0.0, 1.0]
            } else {
                ground_plane(ground, at)
            }),
        })
    }

    /// Notes the parcel the avatar now stands on, and says whether the agent
    /// is to be told of it, under which sequence id: `counting` numbers each
    /// push after the arrival's, as Second Life does; otherwise every push
    /// goes under the unsolicited id.
    pub fn crossed_into(
        &mut self,
        parcel: Option<RegionLocalParcelId>,
        counting: bool,
    ) -> Option<i32> {
        let parcel = parcel?;
        if self.parcel == Some(parcel) {
            return None;
        }
        self.parcel = Some(parcel);
        if counting {
            self.pushes = self.pushes.saturating_add(1);
            Some(self.pushes)
        } else {
            Some(crate::world::UNSOLICITED_SEQUENCE_ID)
        }
    }
}

/// `(x, y)` scaled to length one, or zero.
fn flat(x: f32, y: f32) -> (f32, f32) {
    let length = x.hypot(y);
    if length <= f32::EPSILON {
        (0.0, 0.0)
    } else {
        (x / length, y / length)
    }
}

/// The horizontal speed of `velocity`.
fn ground_speed(velocity: &Vector) -> f32 {
    velocity.x.hypot(velocity.y)
}

/// The whole speed of `velocity`.
fn speed(velocity: &Vector) -> f32 {
    ground_speed(velocity).hypot(velocity.z)
}

/// Whether `now` is a change of motion from `last` worth reporting at once.
fn differs(last: &Reported, now: &Reported) -> bool {
    let velocity = speed(&Vector {
        x: now.velocity.x - last.velocity.x,
        y: now.velocity.y - last.velocity.y,
        z: now.velocity.z - last.velocity.z,
    });
    let rotation = [
        now.rotation.x - last.rotation.x,
        now.rotation.y - last.rotation.y,
        now.rotation.z - last.rotation.z,
        now.rotation.s - last.rotation.s,
    ]
    .into_iter()
    .any(|component| component.abs() > ROTATION_CHANGE);
    velocity > VELOCITY_CHANGE_M_PER_S || rotation || last.airborne != now.airborne
}

/// The plane of the ground under an avatar centred at `at`: its normal and
/// its offset, `0 0 1 h` on level ground at height `h`.
fn ground_plane(ground: &impl Ground, at: &Vector) -> [f32; 4] {
    let span = SLOPE_SPAN_M;
    let rise_x = ground.height_at(at.x + span, at.y) - ground.height_at(at.x - span, at.y);
    let rise_y = ground.height_at(at.x, at.y + span) - ground.height_at(at.x, at.y - span);
    let (nx, ny, nz) = (-rise_x, -rise_y, 2.0 * span);
    let length = nx.hypot(ny).hypot(nz);
    let (nx, ny, nz) = (nx / length, ny / length, nz / length);
    let foot_z = at.z - AVATAR_CENTRE_ABOVE_GROUND_M;
    [nx, ny, nz, nz.mul_add(foot_z, nx.mul_add(at.x, ny * at.y))]
}

/// Puts the default of the animation set of the agent in session `state` at
/// what its posture plays — a script's override of the posture's state
/// (`llSetAnimationOverride`), or the built-in — and states the set to its
/// viewer and to the region's others when that changed it.
pub(crate) fn state_posture(state: &mut SimState, now: Instant) {
    let agent = state.avatar.agent_id;
    let animation = state
        .world
        .lock()
        .script_instances
        .posture_animation(agent, state.motion.posture());
    if !state.animations.set_default(animation) {
        return;
    }
    crate::world::push_own_animation(
        &state.avatar,
        &state.animations,
        state.animation_policy,
        &mut state.sim,
        now,
    );
    // Every statement the agent is sent, the others are sent too.
    let statement = state.animations.observed(state.animation_policy, agent);
    state.tell_others(|resident| {
        resident.animations.clone_from(&statement);
        RegionChange::ResidentAnimated {
            agent,
            statement: Box::new(statement),
        }
    });
}

/// Sits the agent in session `state`, which a seat has just taken, and states
/// its `sit`: at once, as OpenSim's `HandleAgentSit` does, so the seat's
/// script, which hears `changed` in a later tick, finds it playing.
pub(crate) fn seat(state: &mut SimState, now: Instant) {
    if state.motion.halt() {
        state_posture(state, now);
    }
}

/// Stands up the agent in session `state`, which its seat has just let go
/// of, and states its stand at once, as both grids do.
pub(crate) fn rise(state: &mut SimState, now: Instant) {
    if state.motion.rise() {
        state_posture(state, now);
    }
}

/// Runs one tick of the agent's avatar in session `state`: moves it, states
/// its animations when they change, reports it when an update is due, and
/// pushes the parcel it walks onto. A child circuit, an agent that has not
/// arrived and one sitting on something are left alone.
pub(crate) fn advance(state: &mut SimState, step: Duration, now: Instant) {
    if !state.sim.is_root_agent() || state.arrived_at.is_none() {
        return;
    }
    if state.sim.seated_on().is_some() {
        seat(state, now);
        return;
    }
    let placement = state.sim.arrival_position().clone();
    let from = Vector {
        x: placement.position.x(),
        y: placement.position.y(),
        z: placement.position.z(),
    };
    let policy = state.movement_policy;
    let stepped = state.motion.step(&policy, &state.terrain, &from, step);
    let to = stepped.position;
    if stepped.posture_changed {
        tracing::debug!(posture = ?state.motion.posture(), ?to, "the avatar's posture changed");
    }
    if to != from {
        state.sim.set_arrival_position(
            RegionCoordinates::new(to.x, to.y, to.z),
            placement.look_at.clone(),
        );
    }
    let (avatar, parcel) = {
        let mut world = state.world.lock();
        if to != from {
            world.objects.move_wearer(state.avatar.agent_id, &to);
            // Where the region's other viewers place it between updates: the
            // coarse feed and a private parcel go by it.
            if let Some(resident) = world.resident_mut(state.avatar.agent_id) {
                resident.at = to.clone();
                resident.body.motion.position = to.clone();
            }
        }
        let parcel = world.parcel_at(to.x, to.y).cloned();
        (world.avatar_id(state.avatar.agent_id), parcel)
    };
    if to != from {
        crate::driver::refresh_privacy(state, now);
    }
    if stepped.posture_changed {
        state_posture(state, now);
    }
    if let Some(motion) = stepped.update {
        let update = sl_proto::TerseUpdate {
            local_id: avatar,
            state: 0,
            motion,
        };
        if let Err(error) = state.sim.send_terse_update(
            core::slice::from_ref(&update),
            crate::world::REAL_TIME_DILATION,
            now,
        ) {
            tracing::warn!("reporting the agent's avatar failed: {error}");
        }
        // The region's other viewers are sent the same update at the same
        // cadence, as both live grids send another avatar's.
        let agent = state.avatar.agent_id;
        state.tell_others(|resident| {
            resident.body.motion.clone_from(&update.motion);
            RegionChange::ResidentMoved {
                agent,
                update: Box::new(update),
            }
        });
    }
    if let Some(parcel) = parcel
        && let Some(sequence_id) = state.motion.crossed_into(
            Some(parcel.local_id),
            state.parcel_policy.counts_unsolicited_pushes,
        )
    {
        let mut record = parcel;
        record.sequence_id = sequence_id;
        state.sim.enqueue_parcel_properties(&record);
    }
}

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_proto::{AgentUpdateInfo, ControlFlags, MovementMode, RegionLocalParcelId, ServerEvent};
    use sl_types::lsl::{Rotation, Vector};

    use super::{Ground, Motion, MovementPolicy, Posture};
    use crate::ImitatedGrid;
    use crate::world::AVATAR_CENTRE_ABOVE_GROUND_M;

    /// Level ground at 25 m.
    struct Level;

    impl Ground for Level {
        fn height_at(&self, _x: f32, _y: f32) -> f32 {
            25.0
        }
    }

    /// Ground rising one metre for every ten east.
    struct Slope;

    impl Ground for Slope {
        fn height_at(&self, x: f32, _y: f32) -> f32 {
            x / 10.0
        }
    }

    /// The heartbeat's default step.
    const STEP: Duration = crate::heartbeat::DEFAULT_STEP;

    /// Whether two heights are the same to a tenth of a millimetre.
    fn same(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// A standing spot on [`Level`].
    fn standing() -> Vector {
        Vector {
            x: 128.0,
            y: 128.0,
            z: 25.0 + AVATAR_CENTRE_ABOVE_GROUND_M,
        }
    }

    /// The body turned `degrees` about the vertical.
    fn facing(degrees: f32) -> Rotation {
        let half = degrees.to_radians() / 2.0;
        Rotation {
            x: 0.0,
            y: 0.0,
            z: half.sin(),
            s: half.cos(),
        }
    }

    /// The viewer states `controls`, facing `degrees`.
    fn hold(motion: &mut Motion, controls: ControlFlags, degrees: f32) {
        let _heard = motion.hear(&ServerEvent::AgentUpdate(Box::new(AgentUpdateInfo {
            body_rotation: facing(degrees),
            head_rotation: facing(degrees),
            controls,
            camera: sl_proto::Camera::region_center(),
            far: 64.0,
            state: 0,
            flags: 0,
        })));
    }

    /// Runs `ticks` ticks from `at`, returning where the avatar ends up.
    fn run(
        motion: &mut Motion,
        policy: &MovementPolicy,
        ground: &impl Ground,
        mut at: Vector,
        ticks: u32,
    ) -> Vector {
        for _tick in 0..ticks {
            at = motion.step(policy, ground, &at, STEP).position;
        }
        at
    }

    /// Every posture's id is the built-in its name says.
    #[test]
    fn every_posture_is_a_built_in() {
        for posture in [
            Posture::Standing,
            Posture::Walking,
            Posture::Running,
            Posture::Crouching,
            Posture::CrouchWalking,
            Posture::PreJump,
            Posture::Jumping,
            Posture::Falling,
            Posture::Landing,
            Posture::StandingUp,
            Posture::Hovering,
            Posture::HoverUp,
            Posture::HoverDown,
            Posture::Flying,
            Posture::SittingOnGround,
            Posture::SittingOnPrim,
        ] {
            assert_eq!(
                sl_anim::builtin_animation_by_name(posture.animation_name())
                    .map(|builtin| builtin.id),
                Some(posture.animation()),
                "{posture:?}"
            );
        }
    }

    /// **The acceptance**: forward held for N ticks puts the avatar where the
    /// step and the walk speed say, on each flavour, facing where the body
    /// faces.
    #[test]
    fn holding_forward_walks_the_step_times_the_speed() {
        for grid in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let policy = grid.movement_policy();
            let mut motion = Motion::default();
            hold(&mut motion, ControlFlags::AT_POS, 90.0);
            let at = run(&mut motion, &policy, &Level, standing(), 20);
            let walked = policy.walk_speed * STEP.as_secs_f32() * 20.0;
            assert!((at.y - (128.0 + walked)).abs() < 1e-3, "{grid:?}: {at:?}");
            assert!((at.x - 128.0).abs() < 1e-3, "{grid:?}: {at:?}");
            assert!(same(at.z, standing().z), "{grid:?}: {at:?}");
            assert_eq!(motion.posture(), Posture::Walking);
        }
    }

    /// Always-run runs, and a crouch slows only Second Life's avatar.
    #[test]
    fn a_run_is_faster_and_a_crouch_slows_only_second_life() {
        let mut motion = Motion::default();
        let _heard = motion.hear(&ServerEvent::SetAlwaysRun {
            mode: MovementMode::AlwaysRun,
        });
        hold(&mut motion, ControlFlags::AT_POS, 0.0);
        let policy = ImitatedGrid::SecondLife.movement_policy();
        let _at = run(&mut motion, &policy, &Level, standing(), 2);
        assert_eq!(motion.posture(), Posture::Running);
        assert!((motion.velocity().x - policy.run_speed).abs() < 1e-4);

        for (grid, slowed) in [
            (ImitatedGrid::SecondLife, true),
            (ImitatedGrid::OpenSim, false),
        ] {
            let policy = grid.movement_policy();
            let mut motion = Motion::default();
            hold(
                &mut motion,
                ControlFlags::AT_POS | ControlFlags::UP_NEG,
                0.0,
            );
            let _at = run(&mut motion, &policy, &Level, standing(), 2);
            assert_eq!(motion.posture(), Posture::CrouchWalking, "{grid:?}");
            assert_eq!(
                motion.velocity().x < policy.walk_speed - 0.5,
                slowed,
                "{grid:?}"
            );
        }
    }

    /// Letting go coasts the avatar to a stop a little further on, and the
    /// coast is longer on Second Life.
    #[test]
    fn letting_go_coasts_to_a_stop() -> Result<(), String> {
        let mut carried = Vec::new();
        for grid in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let policy = grid.movement_policy();
            let mut motion = Motion::default();
            hold(&mut motion, ControlFlags::AT_POS, 0.0);
            let walked = run(&mut motion, &policy, &Level, standing(), 10);
            hold(&mut motion, ControlFlags::empty(), 0.0);
            let stopped = run(&mut motion, &policy, &Level, walked.clone(), 20);
            assert_eq!(motion.posture(), Posture::Standing, "{grid:?}");
            carried.push(stopped.x - walked.x);
        }
        let (Some(&second_life), Some(&opensim)) = (carried.first(), carried.get(1)) else {
            return Err(format!("two flavours, not {carried:?}"));
        };
        assert!((0.6..1.3).contains(&second_life), "{second_life}");
        assert!((0.3..0.8).contains(&opensim), "{opensim}");
        Ok(())
    }

    /// A walk follows the ground up a slope, and the collision plane tilts
    /// with it.
    #[test]
    fn a_walk_follows_the_ground() {
        let policy = ImitatedGrid::OpenSim.movement_policy();
        let mut motion = Motion::default();
        let start = Vector {
            x: 100.0,
            y: 100.0,
            z: 10.0 + AVATAR_CENTRE_ABOVE_GROUND_M,
        };
        hold(&mut motion, ControlFlags::AT_POS, 0.0);
        let at = run(&mut motion, &policy, &Slope, start, 20);
        assert!(
            (at.z - (at.x / 10.0 + AVATAR_CENTRE_ABOVE_GROUND_M)).abs() < 1e-4,
            "{at:?}"
        );
        let mut seen = None;
        for _tick in 0..40 {
            if let Some(update) = motion.step(&policy, &Slope, &at, STEP).update {
                seen = update.collision_plane;
            }
        }
        let [nx, _ny, nz, _d] = seen.unwrap_or_default();
        assert!(nx < 0.0 && nz > 0.9, "{seen:?}");
    }

    /// The region's edge stops a walk.
    #[test]
    fn the_edge_stops_a_walk() {
        let policy = ImitatedGrid::SecondLife.movement_policy();
        let mut motion = Motion::default();
        hold(&mut motion, ControlFlags::AT_POS, 0.0);
        let at = run(&mut motion, &policy, &Level, standing(), 1_000);
        assert!(at.x < 256.0 && at.x > 255.0, "{at:?}");
    }

    /// Flight climbs, hovers and descends at the flavour's speeds, and comes
    /// down to the ground, flying, without going through it.
    #[test]
    fn flight_climbs_hovers_and_descends() {
        let policy = ImitatedGrid::OpenSim.movement_policy();
        let mut motion = Motion::default();
        hold(&mut motion, ControlFlags::FLY | ControlFlags::UP_POS, 0.0);
        let up = run(&mut motion, &policy, &Level, standing(), 10);
        assert!((up.z - standing().z - policy.climb_speed).abs() < 1e-3);
        assert_eq!(motion.posture(), Posture::HoverUp);
        hold(&mut motion, ControlFlags::FLY, 0.0);
        let hover = run(&mut motion, &policy, &Level, up.clone(), 10);
        assert!(same(hover.z, up.z), "{hover:?} against {up:?}");
        assert_eq!(motion.posture(), Posture::Hovering);
        hold(&mut motion, ControlFlags::FLY | ControlFlags::UP_NEG, 0.0);
        let down = run(&mut motion, &policy, &Level, hover, 30);
        assert!(same(down.z, standing().z), "{down:?}");
        assert_eq!(motion.posture(), Posture::Hovering);
    }

    /// Letting go of flight high up is a fall, fastest at the flavour's
    /// limit, ending in a hard landing.
    #[test]
    fn letting_go_of_flight_falls_and_lands_hard() {
        let policy = ImitatedGrid::OpenSim.movement_policy();
        let mut motion = Motion::default();
        let high = Vector {
            z: 420.0,
            ..standing()
        };
        hold(&mut motion, ControlFlags::empty(), 0.0);
        let mut at = high;
        let mut fastest = 0.0_f32;
        let mut ticks = 0_u32;
        while at.z > standing().z && ticks < 1_000 {
            at = motion.step(&policy, &Level, &at, STEP).position;
            fastest = fastest.max(-motion.velocity().z);
            ticks = ticks.saturating_add(1);
        }
        assert!(
            (fastest - policy.terminal_fall_speed).abs() < 1e-3,
            "{fastest}"
        );
        let secs = f32::from(u16::try_from(ticks).unwrap_or(u16::MAX)) * STEP.as_secs_f32();
        assert!((10.0..14.0).contains(&secs), "{secs}");
        assert_eq!(motion.posture(), Posture::StandingUp);
        // OpenSim stands it up by itself; the forward key then walks it.
        hold(&mut motion, ControlFlags::AT_POS, 0.0);
        let held = run(&mut motion, &policy, &Level, at.clone(), 5);
        assert_eq!(held, at, "held while standing up");
        let walked = run(&mut motion, &policy, &Level, held, 15);
        assert!(walked.x > at.x, "{walked:?}");
    }

    /// Second Life holds a hard landing and a pre-jump until the viewer says
    /// they have played out; OpenSim times both.
    #[test]
    fn second_life_holds_until_finish_anim() {
        let policy = ImitatedGrid::SecondLife.movement_policy();
        let mut motion = Motion::default();
        hold(&mut motion, ControlFlags::UP_POS, 0.0);
        let at = run(&mut motion, &policy, &Level, standing(), 30);
        assert_eq!(at, standing(), "never leaves the ground");
        assert_eq!(motion.posture(), Posture::PreJump);
        hold(
            &mut motion,
            ControlFlags::UP_POS | ControlFlags::FINISH_ANIM,
            0.0,
        );
        let _at = run(&mut motion, &policy, &Level, at, 1);
        assert_eq!(motion.posture(), Posture::Jumping);

        let opensim = ImitatedGrid::OpenSim.movement_policy();
        let mut motion = Motion::default();
        hold(&mut motion, ControlFlags::UP_POS, 0.0);
        let mut at = standing();
        let mut highest = at.z;
        for _tick in 0..40 {
            at = motion.step(&opensim, &Level, &at, STEP).position;
            highest = highest.max(at.z);
        }
        let height = highest - standing().z;
        assert!((4.5..6.5).contains(&height), "{height}");
        assert!(same(at.z, standing().z), "down again: {at:?}");
    }

    /// A tap of the jump key let go of before the next tick is still a jump:
    /// a busy viewer's press and release can both arrive between two ticks.
    #[test]
    fn a_tap_between_ticks_jumps() {
        let policy = ImitatedGrid::OpenSim.movement_policy();
        let mut motion = Motion::default();
        hold(&mut motion, ControlFlags::UP_POS, 0.0);
        hold(&mut motion, ControlFlags::empty(), 0.0);
        let mut at = standing();
        let mut highest = at.z;
        for _tick in 0..40 {
            at = motion.step(&policy, &Level, &at, STEP).position;
            highest = highest.max(at.z);
        }
        assert!(highest - standing().z > 1.0, "{highest}");
    }

    /// Holding the jump key is one jump, not a hop at every landing.
    #[test]
    fn holding_jump_is_one_jump() {
        let policy = ImitatedGrid::OpenSim.movement_policy();
        let mut motion = Motion::default();
        hold(&mut motion, ControlFlags::UP_POS, 0.0);
        let at = run(&mut motion, &policy, &Level, standing(), 60);
        assert_eq!(at, standing());
        let _at = run(&mut motion, &policy, &Level, at, 20);
        assert!(
            matches!(motion.posture(), Posture::Standing | Posture::Crouching),
            "{:?}",
            motion.posture()
        );
    }

    /// A tap of the jump key whose press and release both arrive between
    /// two ticks still jumps.
    #[test]
    fn a_tap_between_two_ticks_jumps() {
        for grid in [ImitatedGrid::OpenSim, ImitatedGrid::SecondLife] {
            let policy = grid.movement_policy();
            let mut motion = Motion::default();
            hold(&mut motion, ControlFlags::UP_POS, 0.0);
            hold(&mut motion, ControlFlags::empty(), 0.0);
            let mut at = standing();
            let mut highest = at.z;
            for _tick in 0..40 {
                at = motion.step(&policy, &Level, &at, STEP).position;
                highest = highest.max(at.z);
                // Second Life waits for the pre-jump to play out.
                if motion.posture() == Posture::PreJump {
                    hold(&mut motion, ControlFlags::FINISH_ANIM, 0.0);
                }
            }
            assert!(highest - standing().z > 1.0, "{grid:?} rose to {highest}");
        }
    }

    /// A change of motion is reported at once; a steady one at the
    /// flavour's cadence.
    #[test]
    fn updates_follow_the_cadence() {
        let count = |grid: ImitatedGrid, controls: ControlFlags, ticks: u32| {
            let policy = grid.movement_policy();
            let mut motion = Motion::default();
            let mut at = standing();
            // Settled at rest first, so the count starts from a reported state.
            for _tick in 0..5 {
                at = motion.step(&policy, &Level, &at, STEP).position;
            }
            hold(&mut motion, controls, 0.0);
            let mut sent = 0_u32;
            for _tick in 0..ticks {
                let stepped = motion.step(&policy, &Level, &at, STEP);
                at = stepped.position;
                sent = sent.saturating_add(u32::from(stepped.update.is_some()));
            }
            sent
        };
        // Ten seconds of a walk: every tick on Second Life, one every two
        // seconds (and the start) on OpenSim.
        assert_eq!(
            count(ImitatedGrid::SecondLife, ControlFlags::AT_POS, 100),
            100
        );
        assert_eq!(count(ImitatedGrid::OpenSim, ControlFlags::AT_POS, 100), 5);
        // Ten seconds of a steady climb: its start alone on Second Life.
        let climb = ControlFlags::FLY | ControlFlags::UP_POS;
        assert_eq!(count(ImitatedGrid::SecondLife, climb, 100), 1);
        assert!(count(ImitatedGrid::OpenSim, climb, 100) > 20);
        // Ten seconds at rest: Second Life keeps reporting, OpenSim does not.
        assert!(count(ImitatedGrid::SecondLife, ControlFlags::empty(), 100) > 10);
        assert_eq!(count(ImitatedGrid::OpenSim, ControlFlags::empty(), 100), 0);
    }

    /// A parcel push goes out for each new parcel, under the unsolicited id
    /// on OpenSim and counted up on Second Life.
    #[test]
    fn a_new_parcel_is_pushed_once() {
        let west = Some(RegionLocalParcelId(1));
        let east = Some(RegionLocalParcelId(2));
        let mut opensim = Motion::arriving(west);
        assert_eq!(opensim.crossed_into(west, false), None);
        assert_eq!(opensim.crossed_into(east, false), Some(0));
        assert_eq!(opensim.crossed_into(east, false), None);
        assert_eq!(opensim.crossed_into(west, false), Some(0));
        let mut second_life = Motion::arriving(west);
        assert_eq!(second_life.crossed_into(east, true), Some(1));
        assert_eq!(second_life.crossed_into(west, true), Some(2));
        assert_eq!(second_life.crossed_into(None, true), None);
    }
}
