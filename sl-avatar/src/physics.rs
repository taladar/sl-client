//! Body-physics (`WT_PHYSICS`) wearable ingest (P34.1): the breast / belly /
//! butt bounce configuration an avatar's appearance carries, resolved into the
//! six spring-damper motions the reference viewer runs.
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
//! This module is the ingest half (P34.1): it turns a visual-param table plus an
//! avatar's appearance into a [`BodyPhysics`] — every motion's resolved settings,
//! its driven params (with the weight range and volume morphs each one needs),
//! and the rest position the user's own shape sits at. Running the simulation
//! over it each frame is P34.2; [`PhysicsMotionConfig::driven_weight`] is the
//! mapping from a simulated position back onto a driven param's weight, so the
//! motion only has to integrate. Like the rest of the crate it is pure: no I/O,
//! no Bevy, Second Life Z-up metres.
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
    use super::{BodyPhysics, PhysicsMotion, PhysicsSettings};
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
}
