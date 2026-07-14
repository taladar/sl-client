//! Body physics (`WT_PHYSICS`): the breast / belly / butt bounce configuration an
//! avatar's appearance carries, resolved into the six spring-damper motions the
//! reference viewer runs (P34.1), and those motions simulated (P34.2).
//!
//! The physics wearable is the one wearable whose params never shape the body
//! directly. Instead they *configure a simulation*: `Breast_Physics_Mass`,
//! `…_Gravity`, `…_Drag`, `…_Spring`, `…_Gain`, `…_Damping` and `…_Max_Effect`
//! are transmitted sliders that parameterize a spring-damper, one per
//! [`PhysicsMotion`], driven by the acceleration of the joint the body part hangs
//! off (`mChest` / `mPelvis`). Each motion writes a hidden **controller** param
//! (`Breast_Physics_UpDown_Controller`, …), which in turn drives the
//! `*_Driven` morph params that actually move geometry — one per affected body
//! part, so a single belly bounce moves the torso, the legs and the skirt
//! together.
//!
//! Two things make the driven params unusual, and both are handled outside this
//! module:
//!
//! - Their **morph targets exist in no `.llm`**: the reference viewer clones
//!   them out of the shape morphs that already move the right vertices while it
//!   loads each base part, and so does
//!   [`BaseMesh::from_bytes`](crate::BaseMesh::from_bytes).
//! - They also carry [`VolumeMorph`]s, displacing the `LEFT_PEC` / `RIGHT_PEC` /
//!   `BELLY` / `BUTT` **collision volumes**. Those volumes are bindable joints,
//!   so this is what makes a worn *rigged mesh* body bounce, which a system-body
//!   morph target cannot reach.
//!
//! The ingest half (P34.1) turns a visual-param table plus an avatar's appearance
//! into a [`BodyPhysics`] — every motion's resolved settings, its driven params
//! (with the weight range and volume morphs each one needs), and the rest position
//! the user's own shape sits at. The simulation half (P34.2) is
//! [`BodyPhysicsState`]: one spring-damper per motion, stepped each frame from a
//! [`JointSample`] of where the motion's joint currently is, yielding the driven
//! params' weights ([`BodyPhysicsState::driven_weights`]) and the collision volumes'
//! displacements ([`BodyPhysicsState::volume_offsets`]).
//!
//! Like the rest of the crate it is pure: no I/O, no Bevy. The caller samples the
//! joint in whatever world frame it renders in and passes its own world-up vector,
//! so nothing here needs to know about Second Life's Z-up or Bevy's Y-up.
//!
//! Reference: `llphysicsmotion.cpp` (`LLPhysicsMotion` /
//! `LLPhysicsMotionController`).

use crate::params::{ParamEffect, VisualParams, VolumeMorph};
use crate::resolve::ResolvedParams;

/// The weight below which a motion's `Max_Effect` counts as off (the reference
/// tests `behavior_maxeffect == 0` to skip the whole simulation).
const MIN_EFFECT: f32 = 1.0e-4;

/// One of the six body-physics motions `LLPhysicsMotionController` runs, each a
/// spring-damper along one axis of one body part.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Physics…` names read clearly"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhysicsMotion {
    /// Breast bounce (up/down along the chest's local Z).
    BreastUpDown,
    /// Breast cleavage (in/out along the chest's local −X).
    BreastInOut,
    /// Breast sway (left/right along the chest's local −Y).
    BreastLeftRight,
    /// Belly bounce (up/down along the pelvis' local −Z).
    BellyUpDown,
    /// Butt bounce (up/down along the pelvis' local −Z).
    ButtUpDown,
    /// Butt sway (left/right along the pelvis' local −Y).
    ButtLeftRight,
}

impl PhysicsMotion {
    /// Every motion, in the order `LLPhysicsMotionController::onInitialize`
    /// registers them.
    pub const ALL: [Self; 6] = [
        Self::BreastInOut,
        Self::BreastUpDown,
        Self::BreastLeftRight,
        Self::ButtUpDown,
        Self::ButtLeftRight,
        Self::BellyUpDown,
    ];

    /// This motion's slot in [`ALL`](Self::ALL) — the index of its simulation state
    /// in a [`BodyPhysicsState`].
    #[must_use]
    const fn index(self) -> usize {
        match self {
            Self::BreastInOut => 0,
            Self::BreastUpDown => 1,
            Self::BreastLeftRight => 2,
            Self::ButtUpDown => 3,
            Self::ButtLeftRight => 4,
            Self::BellyUpDown => 5,
        }
    }

    /// The hidden driver param this motion writes, whose driven params are the
    /// morphs that move (`LLPhysicsMotion`'s `param_driver_name`).
    #[must_use]
    pub const fn controller_param(self) -> &'static str {
        match self {
            Self::BreastUpDown => "Breast_Physics_UpDown_Controller",
            Self::BreastInOut => "Breast_Physics_InOut_Controller",
            Self::BreastLeftRight => "Breast_Physics_LeftRight_Controller",
            Self::BellyUpDown => "Belly_Physics_UpDown_Controller",
            Self::ButtUpDown => "Butt_Physics_UpDown_Controller",
            Self::ButtLeftRight => "Butt_Physics_LeftRight_Controller",
        }
    }

    /// The skeleton joint whose motion drives this body part: its acceleration is
    /// the simulation's forcing term, and its rotation orients
    /// [`direction`](Self::direction).
    #[must_use]
    pub const fn joint(self) -> &'static str {
        match self {
            Self::BreastUpDown | Self::BreastInOut | Self::BreastLeftRight => "mChest",
            Self::BellyUpDown | Self::ButtUpDown | Self::ButtLeftRight => "mPelvis",
        }
    }

    /// The direction, in the [joint](Self::joint)'s local frame, that this motion
    /// measures along: joint velocity / acceleration and gravity are all
    /// projected onto it (`LLPhysicsMotion`'s `motion_direction_vec`).
    #[must_use]
    pub const fn direction(self) -> [f32; 3] {
        match self {
            Self::BreastUpDown => [0.0, 0.0, 1.0],
            Self::BreastInOut => [-1.0, 0.0, 0.0],
            Self::BreastLeftRight | Self::ButtLeftRight => [0.0, -1.0, 0.0],
            Self::BellyUpDown | Self::ButtUpDown => [0.0, 0.0, -1.0],
        }
    }

    /// The `avatar_lad.xml` param names configuring this motion's spring-damper —
    /// the reference's per-motion `controller_map_t`. The mass / gravity / drag
    /// params are shared by every motion of one body part; spring, gain, damping
    /// and max-effect are per axis.
    #[must_use]
    const fn setting_params(self) -> SettingParams {
        match self {
            Self::BreastUpDown => SettingParams {
                mass: "Breast_Physics_Mass",
                gravity: "Breast_Physics_Gravity",
                drag: "Breast_Physics_Drag",
                spring: "Breast_Physics_UpDown_Spring",
                gain: "Breast_Physics_UpDown_Gain",
                damping: "Breast_Physics_UpDown_Damping",
                max_effect: "Breast_Physics_UpDown_Max_Effect",
            },
            Self::BreastInOut => SettingParams {
                mass: "Breast_Physics_Mass",
                gravity: "Breast_Physics_Gravity",
                drag: "Breast_Physics_Drag",
                spring: "Breast_Physics_InOut_Spring",
                gain: "Breast_Physics_InOut_Gain",
                damping: "Breast_Physics_InOut_Damping",
                max_effect: "Breast_Physics_InOut_Max_Effect",
            },
            Self::BreastLeftRight => SettingParams {
                mass: "Breast_Physics_Mass",
                gravity: "Breast_Physics_Gravity",
                drag: "Breast_Physics_Drag",
                spring: "Breast_Physics_LeftRight_Spring",
                gain: "Breast_Physics_LeftRight_Gain",
                damping: "Breast_Physics_LeftRight_Damping",
                max_effect: "Breast_Physics_LeftRight_Max_Effect",
            },
            Self::BellyUpDown => SettingParams {
                mass: "Belly_Physics_Mass",
                gravity: "Belly_Physics_Gravity",
                drag: "Belly_Physics_Drag",
                spring: "Belly_Physics_UpDown_Spring",
                gain: "Belly_Physics_UpDown_Gain",
                damping: "Belly_Physics_UpDown_Damping",
                max_effect: "Belly_Physics_UpDown_Max_Effect",
            },
            Self::ButtUpDown => SettingParams {
                mass: "Butt_Physics_Mass",
                gravity: "Butt_Physics_Gravity",
                drag: "Butt_Physics_Drag",
                spring: "Butt_Physics_UpDown_Spring",
                gain: "Butt_Physics_UpDown_Gain",
                damping: "Butt_Physics_UpDown_Damping",
                max_effect: "Butt_Physics_UpDown_Max_Effect",
            },
            Self::ButtLeftRight => SettingParams {
                mass: "Butt_Physics_Mass",
                gravity: "Butt_Physics_Gravity",
                drag: "Butt_Physics_Drag",
                spring: "Butt_Physics_LeftRight_Spring",
                gain: "Butt_Physics_LeftRight_Gain",
                damping: "Butt_Physics_LeftRight_Damping",
                max_effect: "Butt_Physics_LeftRight_Max_Effect",
            },
        }
    }
}

/// The seven `avatar_lad.xml` param names configuring one motion, resolved into
/// [`PhysicsSettings`] against an avatar's appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SettingParams {
    /// The `…_Mass` param (body-part wide).
    mass: &'static str,
    /// The `…_Gravity` param (body-part wide).
    gravity: &'static str,
    /// The `…_Drag` param (body-part wide).
    drag: &'static str,
    /// The `…_Spring` param (per axis).
    spring: &'static str,
    /// The `…_Gain` param (per axis).
    gain: &'static str,
    /// The `…_Damping` param (per axis).
    damping: &'static str,
    /// The `…_Max_Effect` param (per axis) — zero means the motion is off.
    max_effect: &'static str,
}

/// One motion's resolved spring-damper settings — the physics wearable's sliders
/// for this body part and axis, after appearance and sex resolution.
///
/// [`Default`] is the reference's `initDefaultController` fallback, used for any
/// setting the visual-param table does not define.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Physics…` names read clearly"
)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsSettings {
    /// The bouncing mass (`F = ma`, and the acceleration term's scale).
    pub mass: f32,
    /// Gravity strength: a constant force along the (joint-local) world-down
    /// projection, scaled by the mass.
    pub gravity: f32,
    /// Drag: a velocity-squared resistance (`F = ½kv²`) opposing the *joint's*
    /// motion.
    pub drag: f32,
    /// Spring constant restoring the part to the user's own shape (`F = -kx`).
    pub spring: f32,
    /// Gain on the joint-acceleration forcing term.
    pub gain: f32,
    /// Damping: a resistance proportional to the *param's* own velocity
    /// (`F = -kv`).
    pub damping: f32,
    /// How far, in normalized param space, the bounce may swing either side of
    /// the user's shape. Zero disables the motion entirely.
    pub max_effect: f32,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            mass: 0.2,
            gravity: 0.0,
            drag: 0.15,
            spring: 0.1,
            gain: 10.0,
            damping: 0.05,
            max_effect: 0.1,
        }
    }
}

impl PhysicsSettings {
    /// Whether this motion does anything: a zero `Max_Effect` pins the driven
    /// params at the user's shape, which is the default for every axis (physics
    /// is opt-in, per body part, in the wearable).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.max_effect > MIN_EFFECT
    }
}

/// One morph param a motion drives: the geometry that actually bounces.
///
/// A motion's controller drives one of these per affected body part (the belly
/// controller drives the torso, legs and skirt morphs at once). Both effects a
/// driven param has are carried here: its named morph target on the base mesh
/// (synthesized at `.llm` load — see [`crate::morph::PHYSICS_MORPH_PARAMS`]) and
/// the collision volumes it displaces.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Physics…` names read clearly"
)]
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsDrivenParam {
    /// The driven param's id.
    pub id: i32,
    /// Its name, which is also its morph-target name.
    pub name: String,
    /// Its minimum weight, reached at simulated position `0`.
    pub min: f32,
    /// Its maximum weight, reached at simulated position `1`.
    pub max: f32,
    /// The collision volumes it displaces at full weight (how a rigged mesh body
    /// bounces).
    pub volumes: Vec<VolumeMorph>,
}

/// One ingested motion: its settings, the params it drives, and where the
/// avatar's own shape sits within the driven range.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Physics…` names read clearly"
)]
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsMotionConfig {
    /// Which motion this is.
    pub motion: PhysicsMotion,
    /// The id of its hidden controller (driver) param.
    pub controller: i32,
    /// The controller's own weight, normalized onto `0.0..=1.0` — the rest
    /// position the spring restores towards (`position_user_local`). It is the
    /// midpoint (`0.5`) unless the avatar's shape moved the controller.
    pub rest_position: f32,
    /// The morph params the motion writes each frame.
    pub driven: Vec<PhysicsDrivenParam>,
    /// The resolved spring-damper settings.
    pub settings: PhysicsSettings,
}

impl PhysicsMotionConfig {
    /// The weight to write to `driven` for a simulated position in `0.0..=1.0`,
    /// replicating `LLPhysicsMotion::setParamValue`.
    ///
    /// The simulated position is normalized param space, shared by every driven
    /// param of the motion (they have different weight ranges). It is first
    /// squeezed into the `max_effect`-wide window centred on `0.5` — so a zero
    /// `max_effect` pins the param at its mid weight, and a full one lets it
    /// sweep the whole range — then mapped onto the param's own `[min, max]`.
    #[must_use]
    pub fn driven_weight(&self, driven: &PhysicsDrivenParam, position: f32) -> f32 {
        let effect = self.settings.max_effect;
        let low = 0.5 - effect / 2.0;
        let high = 0.5 + effect / 2.0;
        let rescaled = low + (high - low) * position;
        driven.min + (driven.max - driven.min) * rescaled
    }

    /// The weight `driven` sits at when the motion is at rest (the simulated
    /// position equal to the user's own [`rest_position`](Self::rest_position)) —
    /// the value the driven param already has from ordinary driver propagation,
    /// and the value it must return to when the bounce settles.
    #[must_use]
    pub fn rest_weight(&self, driven: &PhysicsDrivenParam) -> f32 {
        self.driven_weight(driven, self.rest_position)
    }
}

/// An avatar's ingested body-physics configuration: the [`PhysicsMotion`]s its
/// appearance defines, each with resolved settings and driven params (P34.1).
///
/// Built once per avatar appearance, alongside [`MorphWeights`](crate::MorphWeights)
/// and [`SkeletalDeformations`](crate::SkeletalDeformations); the per-frame
/// simulation (P34.2) then reads it every tick. A motion whose controller param
/// the table does not define is omitted, as is one whose controller drives
/// nothing.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Physics…` names read clearly"
)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodyPhysics {
    /// The ingested motions, in [`PhysicsMotion::ALL`] order.
    motions: Vec<PhysicsMotionConfig>,
}

impl BodyPhysics {
    /// Ingest from a visual-param table and a raw wire
    /// `AvatarAppearance.visual_params` byte vector.
    #[must_use]
    pub fn from_appearance(params: &VisualParams, visual_params: &[u8]) -> Self {
        Self::from_resolved(
            params,
            &ResolvedParams::from_appearance(params, visual_params),
        )
    }

    /// Ingest from a visual-param table and already-resolved [`ResolvedParams`]
    /// (the path to use when the caller already resolved the appearance for the
    /// morph / skeletal passes).
    ///
    /// Every setting is taken at its *effective* (sex-gated) weight, so the
    /// breast motions — whose params are `sex="female"` — fall back to their
    /// defaults, and hence to a zero `Max_Effect`, on a male avatar exactly as in
    /// the reference viewer.
    #[must_use]
    pub fn from_resolved(params: &VisualParams, resolved: &ResolvedParams) -> Self {
        let mut motions = Vec::new();
        for motion in PhysicsMotion::ALL {
            let Some(controller) = params.by_name(motion.controller_param()) else {
                continue;
            };
            let ParamEffect::Driver(entries) = &controller.effect else {
                continue;
            };
            let driven: Vec<PhysicsDrivenParam> = entries
                .iter()
                .filter_map(|entry| params.get(entry.id))
                .filter_map(|param| match &param.effect {
                    ParamEffect::Morph(volumes) => Some(PhysicsDrivenParam {
                        id: param.id,
                        name: param.name.clone(),
                        min: param.min,
                        max: param.max,
                        volumes: volumes.clone(),
                    }),
                    _ => None,
                })
                .collect();
            if driven.is_empty() {
                continue;
            }
            motions.push(PhysicsMotionConfig {
                motion,
                controller: controller.id,
                rest_position: normalize(
                    resolved.effective_weight(controller),
                    controller.min,
                    controller.max,
                ),
                driven,
                settings: settings(params, resolved, motion.setting_params()),
            });
        }
        Self { motions }
    }

    /// The ingested motions.
    #[must_use]
    pub fn motions(&self) -> &[PhysicsMotionConfig] {
        &self.motions
    }

    /// The ingested configuration of one motion, if the table defined it.
    #[must_use]
    pub fn motion(&self, motion: PhysicsMotion) -> Option<&PhysicsMotionConfig> {
        self.motions.iter().find(|config| config.motion == motion)
    }

    /// Whether any motion is switched on ([`PhysicsSettings::is_active`]) — i.e.
    /// whether this avatar wears a physics wearable that actually does anything.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.motions
            .iter()
            .any(|config| config.settings.is_active())
    }

    /// Force every motion's `Max_Effect` to `max_effect`, switching the simulation
    /// on for an avatar whose wearable leaves it off — the reference viewer's own
    /// `physics_test` debug switch (`LLPhysicsMotion::onUpdate` overrides
    /// `behavior_maxeffect` to `1.0`), which exists because `Max_Effect` defaults
    /// to zero on every axis and so an ordinary avatar never bounces at all.
    ///
    /// The sex gate still applies: the breast settings are `sex="female"`, so their
    /// *other* settings stay at their (male) defaults and only the belly and butt
    /// motions come alive on a male avatar.
    pub fn force_max_effect(&mut self, max_effect: f32) {
        for config in &mut self.motions {
            config.settings.max_effect = max_effect;
        }
    }
}

/// One frame's sample of the joint a [`PhysicsMotion`] hangs off, in whatever
/// world frame the caller renders in.
///
/// Both the joint's motion through the world *and* its rotation matter: the
/// simulation is one-dimensional, along the motion's own axis
/// ([`PhysicsMotion::direction`]) as the joint currently orients it, so a bow
/// forward bounces the breasts along a different world direction than standing
/// upright does. The caller therefore hands over the joint's world position and
/// that direction already rotated into the world frame — the reference's
/// `LLPhysicsMotion::toLocal`, which is the only place the joint's transform is
/// used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointSample {
    /// The joint's world position, in metres.
    pub position: [f32; 3],
    /// [`PhysicsMotion::direction`] rotated into the world frame by the joint's
    /// world rotation. Need not be normalized (the simulation normalizes it).
    pub direction: [f32; 3],
}

/// The simulation state of one motion: the spring-damper's own position and
/// velocity, plus the joint trail its forcing term is differentiated from.
///
/// Positions are in the normalized `0.0..=1.0` param space every driven param of
/// the motion shares (the reference's `_local` suffix), velocities in that space
/// per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct MotionState {
    /// The simulated position (`mPosition_local`). May leave `0.0..=1.0` between
    /// sub-steps; every read clamps.
    position: f32,
    /// The simulated position's velocity (`mVelocity_local`).
    velocity: f32,
    /// The joint's velocity along the motion direction (`mVelocityJoint_local`),
    /// kept to differentiate the acceleration next frame.
    joint_velocity: f32,
    /// The joint's smoothed acceleration along the motion direction
    /// (`mAccelerationJoint_local`) — the simulation's forcing term, and its own
    /// low-pass filter's history.
    joint_acceleration: f32,
    /// The joint's world position last frame (`mPosition_world`), or `None` before
    /// the first sample — the frame that seeds the trail rather than integrating
    /// against a nonsense one.
    joint_position: Option<[f32; 3]>,
}

/// The largest sub-step the integrator takes, in seconds (the reference's
/// `TIME_ITERATION_STEP_MAX`): a long frame is split into equal sub-steps so a
/// bounce has roughly the same amplitude at 15 FPS as at 60.
const TIME_ITERATION_STEP_MAX: f32 = 0.05;

/// Frames longer than this (seconds) are skipped outright: below 1 FPS the
/// reference does not spend time on physics at all.
const MAX_FRAME_TIME: f32 = 1.0;

/// The joint's world displacement is scaled by this before it is differentiated
/// (the reference's `world_to_model_scale`), turning metres into the units the
/// spring-damper's settings are tuned in.
const WORLD_TO_MODEL_SCALE: f32 = 100.0;

/// The joint's velocity and acceleration are differentiated against a time step
/// this many times longer than the frame's (the reference's `joint_local_factor`).
const JOINT_LOCAL_FACTOR: f32 = 30.0;

/// The joint acceleration's low-pass filter: each frame's raw acceleration
/// contributes `1/SMOOTHING` and the previous value the rest (the reference's
/// hard-coded `smoothing`, once a visual param).
const ACCELERATION_SMOOTHING: f32 = 3.0;

/// The simulated position's speed limit, in normalized param space per second
/// (the reference's `max_velocity`).
const MAX_VELOCITY: f32 = 100.0;

/// The mass below which a motion is treated as inert rather than dividing by it
/// (`a = F/m`). Not a reference behaviour — the wearable's `…_Mass` slider bottoms
/// out at `0.1` — but a table that omits the param entirely must not produce an
/// infinite acceleration.
const MIN_MASS: f32 = 1.0e-4;

/// The running simulation of an avatar's body physics (P34.2): one spring-damper
/// per [`PhysicsMotion`], stepped every frame against the [`BodyPhysics`] the
/// appearance ingested.
///
/// Kept per avatar alongside its [`BodyPhysics`] and re-usable across appearance
/// changes (the state is keyed by motion, not by the config's layout), so a
/// re-ingested wearable retunes the springs mid-bounce rather than restarting them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodyPhysicsState {
    /// Each motion's state, indexed by [`PhysicsMotion::index`].
    motions: [MotionState; 6],
}

impl BodyPhysicsState {
    /// Advance every motion of `physics` by `dt` seconds
    /// (`LLPhysicsMotionController::onUpdate`).
    ///
    /// `sample` supplies the current [`JointSample`] of a motion's joint, or `None`
    /// when the caller cannot resolve it (which forgets the joint trail, so the next
    /// sample re-seeds rather than integrating across the gap). `world_up` is the
    /// caller's world up axis — Second Life's `(0, 0, 1)` gravity vector, which the
    /// motions project onto their own axis.
    ///
    /// An inactive motion (zero `Max_Effect`) is pinned at the user's own shape and
    /// costs nothing, which is the ordinary case: physics is off unless the wearable
    /// switches it on.
    pub fn step(
        &mut self,
        physics: &BodyPhysics,
        dt: f32,
        world_up: [f32; 3],
        sample: impl Fn(PhysicsMotion) -> Option<JointSample>,
    ) {
        for config in physics.motions() {
            if let Some(state) = self.motions.get_mut(config.motion.index()) {
                step_motion(config, state, dt, world_up, sample(config.motion));
            }
        }
    }

    /// The simulated position of `motion`, in the normalized `0.0..=1.0` param
    /// space its driven params share (`0.5` — the user's own shape — before the
    /// first step).
    #[must_use]
    pub fn position(&self, motion: PhysicsMotion) -> f32 {
        self.motions
            .get(motion.index())
            .map_or(0.5, |state| state.position.clamp(0.0, 1.0))
    }

    /// The weight each driven morph param sits at this frame — what the caller
    /// writes into the avatar's runtime morph weights.
    ///
    /// Only the active motions are yielded: an inactive one's driven params are
    /// already at the weight the appearance resolved them to, so leaving them alone
    /// is both cheaper and exactly right.
    pub fn driven_weights<'a>(
        &'a self,
        physics: &'a BodyPhysics,
    ) -> impl Iterator<Item = (&'a str, f32)> {
        physics
            .motions()
            .iter()
            .filter(|config| config.settings.is_active())
            .flat_map(move |config| {
                let position = self.position(config.motion);
                config.driven.iter().map(move |driven| {
                    (driven.name.as_str(), config.driven_weight(driven, position))
                })
            })
    }

    /// The position each collision volume is displaced by this frame, accumulated
    /// over every active motion's driven params (`LLPolyMorphTarget::apply`'s volume
    /// pass, `pos * weight` — the `BUTT` volume takes both butt motions).
    ///
    /// This is how a worn **rigged mesh** body bounces: the volumes are bindable
    /// joints, and a mesh body rigs to them. The system body bounces through the
    /// morph targets of [`driven_weights`](Self::driven_weights) instead.
    ///
    /// Returned as a short `Vec` (at most the four volumes the physics params name)
    /// rather than an iterator, because the accumulation is what makes it correct.
    #[must_use]
    pub fn volume_offsets<'a>(&self, physics: &'a BodyPhysics) -> Vec<(&'a str, [f32; 3])> {
        let mut offsets: Vec<(&str, [f32; 3])> = Vec::new();
        for config in physics
            .motions()
            .iter()
            .filter(|config| config.settings.is_active())
        {
            let position = self.position(config.motion);
            for driven in &config.driven {
                let weight = config.driven_weight(driven, position);
                for volume in &driven.volumes {
                    let delta = [
                        volume.position[0] * weight,
                        volume.position[1] * weight,
                        volume.position[2] * weight,
                    ];
                    match offsets
                        .iter_mut()
                        .find(|(name, _)| *name == volume.volume.as_str())
                    {
                        Some((_, total)) => {
                            total[0] += delta[0];
                            total[1] += delta[1];
                            total[2] += delta[2];
                        }
                        None => offsets.push((volume.volume.as_str(), delta)),
                    }
                }
            }
        }
        offsets
    }
}

/// Advance one motion's spring-damper by `dt` seconds (`LLPhysicsMotion::onUpdate`).
fn step_motion(
    config: &PhysicsMotionConfig,
    state: &mut MotionState,
    dt: f32,
    world_up: [f32; 3],
    sample: Option<JointSample>,
) {
    let Some(sample) = sample else {
        state.joint_position = None;
        return;
    };
    // The first sample only seeds the joint trail: with no previous position there
    // is no displacement to differentiate, and the reference's zero-initialized
    // `mPosition_world` makes its own first step integrate a region-sized jump.
    let Some(last) = state.joint_position.replace(sample.position) else {
        *state = MotionState {
            position: config.rest_position,
            joint_position: Some(sample.position),
            ..MotionState::default()
        };
        return;
    };
    // A frame below 1 FPS (or a paused / rewound clock) is not worth simulating; the
    // trail is already re-seeded above, so the next frame starts clean.
    if !dt.is_finite() || dt <= 0.0 || dt > MAX_FRAME_TIME {
        return;
    }
    let settings = config.settings;
    // Physics off, or a degenerate mass: hold the user's own shape.
    if !settings.is_active() || settings.mass < MIN_MASS {
        state.position = config.rest_position;
        state.velocity = 0.0;
        return;
    }
    let direction = normalized(sample.direction);
    // The joint's velocity and acceleration along the motion's axis, both
    // differentiated against a stretched time step and the world displacement
    // scaled up into the units the settings are tuned in.
    let joint_dt = dt * JOINT_LOCAL_FACTOR;
    let displacement = [
        (sample.position[0] - last[0]) * WORLD_TO_MODEL_SCALE,
        (sample.position[1] - last[1]) * WORLD_TO_MODEL_SCALE,
        (sample.position[2] - last[2]) * WORLD_TO_MODEL_SCALE,
    ];
    let joint_velocity = dot(displacement, direction) / joint_dt;
    let raw_acceleration = (joint_velocity - state.joint_velocity) / joint_dt;
    let joint_acceleration = raw_acceleration / ACCELERATION_SMOOTHING
        + state.joint_acceleration * (ACCELERATION_SMOOTHING - 1.0) / ACCELERATION_SMOOTHING;
    // Gravity is a world-space constant, felt only as far as it points along the
    // motion's axis.
    let gravity_local = dot(world_up, direction);

    let steps = substeps(dt);
    let step = dt / f32::from(steps);
    for _ in 0..steps {
        let position = state.position.clamp(0.0, 1.0);
        // Restoring force towards the user's own shape (F = -kx).
        let force_spring = -(position - config.rest_position) * settings.spring;
        // The forcing term: the joint's acceleration, which is what makes the body
        // part lag behind the body (F = ma).
        let force_accel = settings.gain * joint_acceleration * settings.mass;
        // Gravity (F = mg).
        let force_gravity = gravity_local * settings.gravity * settings.mass;
        // Damping opposes the *param's* own velocity (F = -kv).
        let force_damping = -settings.damping * state.velocity;
        // Drag opposes the *joint's* velocity, quadratically (F = ½kv²) — wind
        // resistance, so a fast-moving body part is pushed the way it came from.
        let force_drag =
            0.5 * settings.drag * joint_velocity * joint_velocity * signum(joint_velocity);
        let force_net = force_accel + force_gravity + force_spring + force_damping + force_drag;

        // a = F/m, integrated forward one sub-step.
        let acceleration = force_net / settings.mass;
        let mut velocity =
            (state.velocity + acceleration * step).clamp(-MAX_VELOCITY, MAX_VELOCITY);
        let new_position = position + velocity * step;
        // Pushed past either end of the param's range: stop dead rather than build
        // up a velocity that would take a frame to unwind.
        if (new_position < 0.0 && velocity < 0.0) || (new_position > 1.0 && velocity > 0.0) {
            velocity = 0.0;
        }
        // Any non-finite value (a NaN setting, an infinite force) resets the motion to
        // the user's shape instead of poisoning every later frame.
        if !new_position.is_finite() || !velocity.is_finite() {
            *state = MotionState {
                position: config.rest_position,
                joint_position: Some(sample.position),
                ..MotionState::default()
            };
            return;
        }
        state.velocity = velocity;
        state.joint_acceleration = joint_acceleration;
        state.position = new_position;
    }
    state.joint_velocity = joint_velocity;
}

/// The sub-step count's ceiling. `dt` is at most [`MAX_FRAME_TIME`] by the time
/// [`substeps`] is called, which needs 21, so this only bounds the loop.
const MAX_SUBSTEPS: u16 = 32;

/// How many equal sub-steps a frame of `dt` seconds is integrated in: enough that
/// none exceeds [`TIME_ITERATION_STEP_MAX`], so the sub-step size lands in
/// `0.025..=0.05` s.
///
/// The reference computes this as `(U32)(time_delta / TIME_ITERATION_STEP_MAX) + 1`
/// — counted here rather than cast (the workspace forbids `as`), including its
/// behaviour on an exact multiple, where the `+ 1` buys a step more than needed.
fn substeps(dt: f32) -> u16 {
    let mut steps: u16 = 1;
    while f32::from(steps) * TIME_ITERATION_STEP_MAX <= dt && steps < MAX_SUBSTEPS {
        steps = steps.saturating_add(1);
    }
    steps
}

/// The dot product of two vectors.
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v` scaled to unit length, or the zero vector if it is degenerate (which makes
/// every force that projects onto it vanish, so the motion simply rests).
fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    if length > f32::EPSILON {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// The reference's `llsgn`: `1.0` for a non-negative value, `-1.0` otherwise (note
/// this is *not* `f32::signum`, which returns `1.0` for a positive zero and `-1.0`
/// for a negative one).
fn signum(a: f32) -> f32 {
    if a >= 0.0 { 1.0 } else { -1.0 }
}

/// Resolve one motion's seven settings, each falling back to the reference's
/// default when the table has no such param.
fn settings(
    params: &VisualParams,
    resolved: &ResolvedParams,
    names: SettingParams,
) -> PhysicsSettings {
    let defaults = PhysicsSettings::default();
    let value = |name: &str, fallback: f32| {
        params
            .by_name(name)
            .map_or(fallback, |param| resolved.effective_weight(param))
    };
    PhysicsSettings {
        mass: value(names.mass, defaults.mass),
        gravity: value(names.gravity, defaults.gravity),
        drag: value(names.drag, defaults.drag),
        spring: value(names.spring, defaults.spring),
        gain: value(names.gain, defaults.gain),
        damping: value(names.damping, defaults.damping),
        max_effect: value(names.max_effect, defaults.max_effect),
    }
}

/// Map a weight in `[min, max]` onto `0.0..=1.0` (the reference's
/// `position_user_local`), guarding a degenerate range.
fn normalize(weight: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if span.abs() > f32::EPSILON {
        ((weight - min) / span).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::{BodyPhysics, BodyPhysicsState, JointSample, PhysicsMotion, PhysicsSettings};
    use crate::params::VisualParams;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A cut-down physics wearable: the belly motion (one controller driving two
    /// driven morphs, one of which displaces the `BELLY` collision volume) plus
    /// its four transmitted settings, in wire (ascending id) order
    /// `[Belly_Physics_Drag=10013, …_UpDown_Max_Effect=10014, …_Spring=10015]`.
    const PHYSICS_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="upperBodyMesh" lod="0" file_name="avatar_upper_body.llm">
    <param id="1204" group="1" name="Belly_Physics_Torso_UpDown_Driven" wearable="physics"
           value_default="0" value_min="-1" value_max="1">
      <param_morph>
        <volume_morph name="BELLY" scale="0.0 0.0 0.0" pos="0.0 0.0 0.05"/>
      </param_morph>
    </param>
    <param id="1202" group="1" name="Belly_Physics_Legs_UpDown_Driven" wearable="physics"
           value_min="-1" value_max="1">
      <param_morph/>
    </param>
  </mesh>
  <driver_parameters>
    <param id="1102" group="1" wearable="physics" name="Belly_Physics_UpDown_Controller"
           value_min="-1" value_max="1" value_default="0">
      <param_driver>
        <driven id="1202"/>
        <driven id="1204"/>
      </param_driver>
    </param>
    <param id="10013" group="0" wearable="physics" name="Belly_Physics_Drag"
           value_default="1" value_min="0" value_max="10">
      <param_driver/>
    </param>
    <param id="10014" group="0" wearable="physics" name="Belly_Physics_UpDown_Max_Effect"
           value_default="0" value_min="0" value_max="3">
      <param_driver/>
    </param>
    <param id="10015" group="0" wearable="physics" name="Belly_Physics_UpDown_Spring"
           value_default="10" value_min="0" value_max="20">
      <param_driver/>
    </param>
  </driver_parameters>
</linden_avatar>"#;

    /// Compare two floats within a tolerance (keeps the assertion off
    /// `float_cmp`).
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0e-4
    }

    #[test]
    fn ingests_the_belly_motion_from_the_appearance() -> Result<(), TestError> {
        let params = VisualParams::from_xml(PHYSICS_LAD)?;
        // Wire order [10013, 10014, 10015]: drag mid, max-effect full, spring low.
        let physics = BodyPhysics::from_appearance(&params, &[128, 255, 0]);
        assert_eq!(physics.motions().len(), 1);

        let belly = physics
            .motion(PhysicsMotion::BellyUpDown)
            .ok_or("belly motion")?;
        assert_eq!(belly.controller, 1102);
        assert_eq!(belly.motion.joint(), "mPelvis");
        let [dx, dy, dz] = belly.motion.direction();
        assert!(approx(dx, 0.0) && approx(dy, 0.0) && approx(dz, -1.0));
        // The controller sits at its default (0) in [-1, 1] -> the middle.
        assert!(approx(belly.rest_position, 0.5));

        // Both driven morphs, with the torso one's collision-volume displacement.
        let names: Vec<&str> = belly
            .driven
            .iter()
            .map(|driven| driven.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Belly_Physics_Legs_UpDown_Driven",
                "Belly_Physics_Torso_UpDown_Driven"
            ]
        );
        let torso = belly.driven.get(1).ok_or("torso driven")?;
        assert_eq!(torso.volumes.len(), 1);
        let volume = torso.volumes.first().ok_or("belly volume")?;
        assert_eq!(volume.volume, "BELLY");
        assert!(approx(volume.position[2], 0.05));

        // Settings: the transmitted ones from the wire, the rest at the
        // reference's defaults.
        let defaults = PhysicsSettings::default();
        assert!(approx(belly.settings.max_effect, 3.0));
        assert!(approx(belly.settings.spring, 0.0));
        assert!(approx(belly.settings.drag, 5.0196));
        assert!(approx(belly.settings.mass, defaults.mass));
        assert!(approx(belly.settings.gain, defaults.gain));
        assert!(belly.settings.is_active());
        assert!(physics.is_active());
        Ok(())
    }

    #[test]
    fn a_zero_max_effect_pins_the_driven_param_at_the_user_shape() -> Result<(), TestError> {
        let params = VisualParams::from_xml(PHYSICS_LAD)?;
        // Max-effect at its default (0) -> the motion is off.
        let physics = BodyPhysics::from_appearance(&params, &[128, 0, 128]);
        let belly = physics
            .motion(PhysicsMotion::BellyUpDown)
            .ok_or("belly motion")?;
        assert!(!belly.settings.is_active());
        assert!(!physics.is_active());

        // With no effect window, every simulated position maps to the driven
        // param's mid weight — the shape the avatar already has.
        let torso = belly.driven.get(1).ok_or("torso driven")?;
        assert!(approx(belly.driven_weight(torso, 0.0), 0.0));
        assert!(approx(belly.driven_weight(torso, 1.0), 0.0));
        assert!(approx(belly.rest_weight(torso), 0.0));
        Ok(())
    }

    #[test]
    fn max_effect_scales_the_driven_swing() -> Result<(), TestError> {
        let params = VisualParams::from_xml(PHYSICS_LAD)?;
        // Max-effect 1.0 (byte 85 of [0, 3]) -> the driven param sweeps its full
        // range as the simulated position sweeps [0, 1].
        let physics = BodyPhysics::from_appearance(&params, &[128, 85, 128]);
        let belly = physics
            .motion(PhysicsMotion::BellyUpDown)
            .ok_or("belly motion")?;
        assert!(approx(belly.settings.max_effect, 1.0));

        let torso = belly.driven.get(1).ok_or("torso driven")?;
        assert!(approx(belly.driven_weight(torso, 0.0), -1.0));
        assert!(approx(belly.driven_weight(torso, 0.5), 0.0));
        assert!(approx(belly.driven_weight(torso, 1.0), 1.0));
        // At rest the bounce sits exactly on the user's shape.
        assert!(approx(belly.rest_weight(torso), 0.0));
        Ok(())
    }

    #[test]
    fn a_table_without_the_physics_wearable_ingests_nothing() -> Result<(), TestError> {
        let lad = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <mesh type="headMesh" lod="0" file_name="avatar_head.llm">
    <param id="1" group="0" name="Plain" value_min="0" value_max="1"><param_morph/></param>
  </mesh>
</linden_avatar>"#;
        let params = VisualParams::from_xml(lad)?;
        let physics = BodyPhysics::from_appearance(&params, &[128]);
        assert!(physics.motions().is_empty());
        assert!(!physics.is_active());
        Ok(())
    }

    // P34.2 — the simulation.

    /// A 60 FPS frame.
    const FRAME: f32 = 1.0 / 60.0;

    /// Second Life's world up, the frame the fixtures below sample in (the direction
    /// vectors are the motions' own, unrotated, as if the avatar stood upright).
    const UP: [f32; 3] = [0.0, 0.0, 1.0];

    /// The [`PHYSICS_LAD`] belly motion with its `Max_Effect` at full (byte 85 of
    /// `[0, 3]` is 1.0), so the driven params sweep their whole range.
    fn active_belly() -> Result<BodyPhysics, TestError> {
        let params = VisualParams::from_xml(PHYSICS_LAD)?;
        let physics = BodyPhysics::from_appearance(&params, &[128, 85, 128]);
        Ok(physics)
    }

    /// Step `state` for `frames` frames with the belly joint held at `position`.
    fn hold(state: &mut BodyPhysicsState, physics: &BodyPhysics, position: [f32; 3], frames: u32) {
        for _ in 0..frames {
            state.step(physics, FRAME, UP, |motion| {
                Some(JointSample {
                    position,
                    direction: motion.direction(),
                })
            });
        }
    }

    /// The sub-step count follows the reference's `(U32)(dt / 0.05) + 1`, including
    /// the extra step it takes on an exact multiple — so a sub-step is never longer
    /// than 0.05 s and a bounce has the same amplitude at 15 FPS as at 60.
    #[test]
    fn a_long_frame_is_split_into_short_substeps() {
        assert_eq!(super::substeps(FRAME), 1);
        assert_eq!(super::substeps(0.05), 2);
        assert_eq!(super::substeps(0.06), 2);
        assert_eq!(super::substeps(0.1), 3);
        // Every sub-step lands in the documented 0.025..=0.05 s window.
        for steps in 1..=20_u16 {
            let dt = f32::from(steps) * 0.037;
            let size = dt / f32::from(super::substeps(dt));
            assert!((0.024..=0.051).contains(&size), "dt {dt} -> step {size}");
        }
    }

    /// A motionless joint never bounces: with no acceleration to force it and the
    /// spring already at the user's own shape, the simulated position stays put and
    /// every driven param stays at its rest weight.
    #[test]
    fn a_still_joint_holds_the_user_shape() -> Result<(), TestError> {
        let physics = active_belly()?;
        let mut state = BodyPhysicsState::default();
        hold(&mut state, &physics, [128.0, 128.0, 21.0], 120);

        let belly = physics
            .motion(PhysicsMotion::BellyUpDown)
            .ok_or("belly motion")?;
        assert!(approx(state.position(PhysicsMotion::BellyUpDown), 0.5));
        let torso = belly.driven.get(1).ok_or("torso driven")?;
        let weights: Vec<(&str, f32)> = state.driven_weights(&physics).collect();
        assert_eq!(weights.len(), 2);
        for (_name, weight) in weights {
            assert!(approx(weight, belly.rest_weight(torso)));
        }
        Ok(())
    }

    /// A joint that drops and then stops bounces: the spring-damper leaves the user's
    /// shape while the body part catches up, then settles back onto it. This is the
    /// whole motion — the `Max_Effect` window only scales how far the driven params
    /// swing while it happens.
    #[test]
    fn a_dropping_joint_bounces_and_settles() -> Result<(), TestError> {
        let physics = active_belly()?;
        let mut state = BodyPhysicsState::default();
        // Seed the joint trail, then drop the pelvis 5 cm in one frame.
        hold(&mut state, &physics, [128.0, 128.0, 21.0], 1);
        hold(&mut state, &physics, [128.0, 128.0, 20.95], 1);
        let bounced = state.position(PhysicsMotion::BellyUpDown);
        assert!(
            (bounced - 0.5).abs() > 0.05,
            "the drop should have moved the belly off the user's shape, got {bounced}"
        );

        // Held still, the spring restores it (the fixture's damping is the
        // reference's default 0.05, so the ring-down takes seconds, not frames).
        hold(&mut state, &physics, [128.0, 128.0, 20.95], 3600);
        let settled = state.position(PhysicsMotion::BellyUpDown);
        assert!(
            (settled - 0.5).abs() < 0.01,
            "the belly should have settled back onto the user's shape, got {settled}"
        );
        Ok(())
    }

    /// The driven params' **volume morphs** displace the collision volumes a rigged
    /// mesh body rigs to — the only way such a body can bounce at all — by
    /// `pos * weight`, accumulated per volume.
    #[test]
    fn a_bounce_displaces_the_collision_volume() -> Result<(), TestError> {
        let physics = active_belly()?;
        let mut state = BodyPhysicsState::default();
        hold(&mut state, &physics, [128.0, 128.0, 21.0], 1);
        hold(&mut state, &physics, [128.0, 128.0, 20.95], 1);

        let belly = physics
            .motion(PhysicsMotion::BellyUpDown)
            .ok_or("belly motion")?;
        let torso = belly.driven.get(1).ok_or("torso driven")?;
        let weight = belly.driven_weight(torso, state.position(PhysicsMotion::BellyUpDown));
        let offsets = state.volume_offsets(&physics);
        assert_eq!(offsets.len(), 1);
        let (volume, offset) = offsets.first().ok_or("belly volume")?;
        assert_eq!(*volume, "BELLY");
        assert!(approx(offset[2], 0.05 * weight));
        assert!(
            offset[2].abs() > 1.0e-3,
            "a settled-looking offset {offset:?}"
        );
        Ok(())
    }

    /// An inactive motion (the default — `Max_Effect` is zero on every axis unless
    /// the wearable turns it on) drives nothing at all: no morph weights, no volume
    /// displacements, so the avatar keeps the shape its appearance resolved to.
    #[test]
    fn an_inactive_motion_drives_nothing() -> Result<(), TestError> {
        let params = VisualParams::from_xml(PHYSICS_LAD)?;
        let mut physics = BodyPhysics::from_appearance(&params, &[128, 0, 128]);
        let mut state = BodyPhysicsState::default();
        hold(&mut state, &physics, [128.0, 128.0, 21.0], 1);
        hold(&mut state, &physics, [128.0, 128.0, 20.5], 10);
        assert!(state.driven_weights(&physics).next().is_none());
        assert!(state.volume_offsets(&physics).is_empty());
        assert!(approx(state.position(PhysicsMotion::BellyUpDown), 0.5));

        // The reference's `physics_test` switch turns it on without a wearable.
        physics.force_max_effect(1.0);
        assert!(physics.is_active());
        hold(&mut state, &physics, [128.0, 128.0, 20.0], 2);
        assert_eq!(state.driven_weights(&physics).count(), 2);
        Ok(())
    }

    /// A joint the caller cannot resolve (a skeleton without it, an avatar mid-
    /// respawn) leaves the motion where it was and re-seeds the joint trail, rather
    /// than integrating a region-sized jump the next time the joint reappears.
    #[test]
    fn an_unresolvable_joint_re_seeds_instead_of_lurching() -> Result<(), TestError> {
        let physics = active_belly()?;
        let mut state = BodyPhysicsState::default();
        hold(&mut state, &physics, [128.0, 128.0, 21.0], 2);
        // The joint vanishes for a frame, and the avatar reappears 200 m away.
        state.step(&physics, FRAME, UP, |_motion| None);
        hold(&mut state, &physics, [328.0, 128.0, 21.0], 1);
        // Re-seeded: the teleport is not a 200 m displacement to differentiate.
        assert!(approx(state.position(PhysicsMotion::BellyUpDown), 0.5));
        Ok(())
    }
}
