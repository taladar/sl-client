//! The scene's **water fog parameters**, and their delivery to the materials that
//! have to apply the fog themselves.
//!
//! The reference viewer fogs the water twice over, and so does this one:
//!
//! - across the **opaque** scene, per pixel of the depth buffer — the fullscreen
//!   haze pass in [`crate::underwater_fog`] (the reference's
//!   `class3/deferred/waterHazeF.glsl`, likewise run over its deferred render);
//! - and **per fragment** inside every alpha-blended surface's own shader — the
//!   reference's `alphaF.glsl` `WATER_FOG` branch — because a translucent draw
//!   writes no depth and so is nowhere in the buffer the haze pass reads. Fogged
//!   from that buffer it would be measured by the distance of whatever stands
//!   *behind* it, which where that is the void is the camera's far clip: four
//!   kilometres of water, and the surface erased outright
//!   (`viewer-underwater-fog-swallows-translucency`).
//!
//! The second half needs the fog parameters *inside* the material, and that is what
//! this module is: [`WaterFogSettings`] resolves the region's EEP water settings
//! into the four values the shaders want, and
//! `apply_water_fog_to_face_materials` carries them into every
//! [`FaceMaterial`] — the material every prim, mesh, sculpt, avatar, attachment and
//! editor overlay face renders through.
//!
//! # Why the parameters are carried, and not looked up
//!
//! A material shader can read the view bind group (the camera, the lights, the
//! clock) and its own material bind group, and nothing else: Bevy has no
//! per-view slot an application can add a uniform of its own to. So a scene-wide
//! value a material needs has to travel in the material — which is only affordable
//! because these particular values almost never change:
//!
//! - the fog **colour** and **densities** come from the region's water settings,
//!   which change when the region does, when its environment is edited, or when a
//!   day cycle blends between two *water* frames (rare — a day cycle's water track
//!   is usually one frame);
//! - the water **level** changes when the region does;
//! - the `KS` term, which follows the sun through the day and would otherwise be
//!   the one value that changes every frame, is **not** carried at all: the shader
//!   derives it from the scene's directional light, which the viewer aims at
//!   whichever heavenly body is up — the same quantity the CPU takes from the sky
//!   settings' sun / moon rotation;
//! - and which of the two densities applies — the water frame's own, or the one
//!   `getModifiedWaterFogDensity` raises it to while the eye is under the surface —
//!   is decided in the shader from the view position, so a camera bobbing through
//!   the waterline rewrites nothing.
//!
//! What is left is a sweep over the material assets on a change nobody makes twice
//! a minute, and a check (not a write) for each material that some *other* system
//! has touched.
//!
//! The one case that could still cost something is a region whose day cycle
//! animates its **water** track rather than only its sky: the blended values then
//! change at every day-cycle sampling step (`sky::DAY_POSITION_STEPS`, which is what
//! keeps the water *material*'s own compare from firing every frame), and the sweep
//! re-prepares every face material at that cadence. No region seen so far does it —
//! a day cycle's water track is one frame — but if it ever shows up in a profile,
//! quantising the carried values is the lever, not making the sweep cheaper.

use bevy::asset::AssetEventSystems;
use bevy::prelude::*;

use sl_client_bevy::WaterSettings;

use crate::environment::EnvironmentState;
use crate::face_material::FaceMaterial;
use crate::render_overrides::RenderOverrides;
use crate::sky::day_position;
use crate::water::{DEFAULT_WATER_HEIGHT, WaterLevel, drive_water};

/// The scene-wide water fog, as every consumer needs it: the authored fog colour,
/// the density on each side of the surface, and the surface height.
///
/// Resolved once per frame by `update_water_fog_settings` from the region's EEP
/// water settings and the current water level, and read by both halves of the fog —
/// the fullscreen haze pass and the materials that fog themselves — so the two can
/// never disagree about the water they are in.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct WaterFogSettings {
    /// The **authored** (sRGB) water fog colour (`waterFogColor`). The shaders
    /// decode it to linear, exactly where the reference does.
    pub color: Vec3,
    /// The water frame's own fog density — the density in force while the eye is
    /// **above** the surface.
    pub density_above: f32,
    /// The density while the eye is **submerged**: the frame's density raised to
    /// its underwater modifier (`getModifiedWaterFogDensity`).
    pub density_submerged: f32,
    /// The water surface height, in world metres.
    pub level: f32,
}

impl WaterFogSettings {
    /// Resolve a water frame's fog: its authored colour, its density, and the
    /// density `getModifiedWaterFogDensity` raises that to while the eye is
    /// submerged, at the given surface height.
    ///
    /// Used by `update_water_fog_settings` for the live scene, and by the water
    /// material's own seeds (which have a frame but no resource yet), so a seeded
    /// sea and a driven one fog identically.
    #[must_use]
    pub fn from_water(water: &WaterSettings, level: f32) -> Self {
        Self {
            color: Vec3::new(
                water.water_fog_color.red(),
                water.water_fog_color.green(),
                water.water_fog_color.blue(),
            ),
            density_above: modified_water_fog_density(
                water.water_fog_density,
                water.underwater_fog_mod,
                false,
            ),
            density_submerged: modified_water_fog_density(
                water.water_fog_density,
                water.underwater_fog_mod,
                true,
            ),
            level,
        }
    }

    /// The same settings with both densities zeroed — the `SL_VIEWER_DISABLE_UNDERWATER_FOG`
    /// A/B knob, which has to turn off the material-side fog as well as the
    /// fullscreen pass or it would only be measuring half of the effect.
    #[must_use]
    const fn without_fog(self) -> Self {
        Self {
            density_above: 0.0,
            density_submerged: 0.0,
            ..self
        }
    }
}

impl Default for WaterFogSettings {
    /// No fog at all, at the default sea level: a zero density is the identity of
    /// the fog arithmetic, so nothing is fogged until a region's water settings
    /// arrive.
    fn default() -> Self {
        Self {
            color: Vec3::ZERO,
            density_above: 0.0,
            density_submerged: 0.0,
            level: DEFAULT_WATER_HEIGHT,
        }
    }
}

/// The water fog density the shaders should use, given the water frame's density and
/// underwater fog modifier and whether the eye is submerged — the reference
/// `LLSettingsWater::getModifiedWaterFogDensity` (`llsettingswater.cpp:377`).
///
/// Submerged, the density is raised to the modifier (clamped to the reference's
/// `[0, 10]`); above water it is the frame's density unchanged.
///
/// The guard on a **negative** density is the reference's fix for
/// BUG-233797 / BUG-233798: a negative base raised to a non-integral power is not a
/// real number, so `powf` returns `NaN`, and a `NaN` density reaches the uniform and
/// takes the whole screen with it — the reference's comment calls it an
/// *unrecoverable blackout*. Both are values a region may legitimately send: the
/// density is a free `f32` off the wire, and the modifier is authored per water
/// frame. Of the two remedies the reference weighed, it chose (and this follows)
/// forcing the density to `1.0` in that case, which keeps some notion of fog rather
/// than rounding the modifier and inverting the water's colour.
///
/// Integrality is tested on the *clamped* modifier, as in the reference — a modifier
/// of `10.5` clamps to `10.0`, which is integral, and needs no rescue.
pub(crate) fn modified_water_fog_density(density: f32, fog_mod: f32, submerged: bool) -> f32 {
    if !(submerged && fog_mod > 0.0) {
        return density;
    }
    let fog_mod = fog_mod.clamp(0.0, 10.0);
    let density = if density < 0.0 && fog_mod.fract() > 0.0 {
        1.0
    } else {
        density
    };
    density.powf(fog_mod)
}

/// Resolve the region's EEP water settings and the current water level into
/// [`WaterFogSettings`], on a write-on-change so that a settled scene marks the
/// resource unchanged and neither consumer does any work.
///
/// `SL_VIEWER_DISABLE_UNDERWATER_FOG=1` ([`RenderOverrides`]) zeroes **both**
/// densities here, which is what makes that A/B knob turn off the whole effect and
/// not merely the fullscreen half of it.
pub(crate) fn update_water_fog_settings(
    environment: Res<EnvironmentState>,
    level: Res<WaterLevel>,
    overrides: Res<RenderOverrides>,
    mut settings: ResMut<WaterFogSettings>,
) {
    let position = day_position(&environment);
    // No water frame is no fog: a region whose environment has not arrived yet
    // fogs nothing, exactly as the default settings do.
    let resolved = environment
        .settings
        .blended_water_settings(position)
        .map_or_else(
            || WaterFogSettings {
                level: level.0,
                ..WaterFogSettings::default()
            },
            |water| WaterFogSettings::from_water(&water, level.0),
        );
    let resolved = if overrides.underwater_fog_disabled {
        resolved.without_fog()
    } else {
        resolved
    };
    settings.set_if_neq(resolved);
}

/// Carry the scene's water fog into every [`FaceMaterial`], so a translucent face
/// can fog itself.
///
/// Two paths, and the split is the whole point:
///
/// - when the settings **change** — a new region, an environment edit, a day cycle
///   crossing between two water frames — every material is rewritten. That marks
///   them all changed and re-prepares their bind groups, which is why it must not
///   happen on an ordinary frame;
/// - otherwise only the materials some other system has **added or modified** this
///   frame are looked at, and each is *checked* before it is written. A material
///   that already carries the current fog is left alone, so this system's own
///   writes do not feed back into it: the `Modified` event a write raises finds the
///   values already equal on the next frame and stops there.
///
/// A material is checked rather than blindly written because a write is not free —
/// [`Assets::get_mut`] marks the asset changed, and a changed material is a
/// re-prepared bind group.
pub(crate) fn apply_water_fog_to_face_materials(
    settings: Res<WaterFogSettings>,
    mut events: MessageReader<AssetEvent<FaceMaterial>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let WaterFogSettings {
        color,
        density_above,
        density_submerged,
        level,
    } = *settings;
    if settings.is_changed() {
        for (_id, material) in materials.iter_mut() {
            material
                .extension
                .params
                .set_water_fog(color, density_above, density_submerged, level);
        }
        // Everything is current, including whatever was added this frame — and the
        // sweep just raised a `Modified` event for each of them.
        events.clear();
        return;
    }
    for event in events.read() {
        let (AssetEvent::Added { id } | AssetEvent::Modified { id }) = *event else {
            continue;
        };
        let stale = materials.get(id).is_some_and(|material| {
            !material
                .extension
                .params
                .has_water_fog(color, density_above, density_submerged, level)
        });
        if stale && let Some(mut material) = materials.get_mut(id) {
            material
                .extension
                .params
                .set_water_fog(color, density_above, density_submerged, level);
        }
    }
}

/// Resolves the scene's water fog each frame and carries it into the face
/// materials. Added by [`UnderwaterFogPlugin`](crate::underwater_fog::UnderwaterFogPlugin),
/// which owns the other half of the same effect — the two are always registered
/// together, so the fullscreen haze and the materials' own fog can never be in
/// force one without the other.
#[derive(Debug, Default)]
pub struct WaterFogPlugin;

impl Plugin for WaterFogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WaterFogSettings>()
            .init_resource::<RenderOverrides>()
            .add_systems(
                Update,
                // After the ocean, whose own system publishes the water level these
                // settings are measured from. The water *material* reads the
                // settings a frame late as a result, which is why they carry nothing
                // that changes while anyone is looking.
                update_water_fog_settings.after(drive_water),
            )
            .add_systems(
                PostUpdate,
                // In `PostUpdate`, and after the asset events are flushed: Bevy
                // announces a material's `Added` there, so a system in `Update`
                // would only hear about a face created this frame on the *next*
                // one — and a newly rezzed translucent face would draw one frame
                // unfogged. `PostUpdate` still precedes the render extraction, so
                // the fog reaches the GPU on the frame the material appeared.
                apply_water_fog_to_face_materials.after(AssetEventSystems),
            );
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::{AssetApp as _, AssetEventSystems};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{
        WaterFogSettings, apply_water_fog_to_face_materials, modified_water_fog_density,
        update_water_fog_settings,
    };
    use crate::environment::EnvironmentState;
    use crate::face_material::{FaceMaterial, SlFaceExt};
    use crate::render_overrides::RenderOverrides;
    use crate::water::WaterLevel;

    /// A fog density is the expected one, to within a relative tolerance that leaves
    /// room for the last bit of a `powf` — and, since every comparison against a
    /// `NaN` is false, an assertion that the value is a real number at all.
    #[track_caller]
    fn assert_density(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= expected.abs() * 1e-6,
            "expected a fog density of {expected}, got {actual}",
        );
    }

    /// Above water the frame's density is the density, whatever the modifier says —
    /// the modifier only applies to a submerged eye.
    #[test]
    fn above_water_the_density_is_untouched() {
        assert_density(modified_water_fog_density(2.0, 0.25, false), 2.0);
        // Including the value that would otherwise need the negative-base rescue:
        // out of the water there is no `powf` to go non-real.
        assert_density(modified_water_fog_density(-2.0, 0.25, false), -2.0);
    }

    /// A non-positive modifier is the reference's own "no modification" case
    /// (`underwater && underwater_fog_mod > 0.0f`), submerged or not.
    #[test]
    fn a_non_positive_modifier_is_untouched() {
        assert_density(modified_water_fog_density(2.0, 0.0, true), 2.0);
        assert_density(modified_water_fog_density(2.0, -1.0, true), 2.0);
    }

    /// The ordinary submerged case: the density raised to the modifier, with the
    /// modifier clamped to the reference's `[0, 10]`.
    #[test]
    fn submerged_the_density_is_raised_to_the_modifier() {
        assert_density(modified_water_fog_density(4.0, 0.5, true), 2.0);
        // 16 clamps to 10, not 16 — `2^10`, not `2^16`.
        assert_density(modified_water_fog_density(2.0, 16.0, true), 1024.0);
    }

    /// BUG-233797 / BUG-233798: a negative density raised to a non-integral power is
    /// not a real number, and the `NaN` `powf` returns would reach the uniform and
    /// black out the whole screen. The reference forces the density to `1.0` in that
    /// case; so does this.
    #[test]
    fn a_negative_density_never_yields_a_non_real_result() {
        assert_density(modified_water_fog_density(-2.0, 0.25, true), 1.0);
        // An *integral* modifier needs no rescue — the power is real, and the
        // reference lets it through.
        assert_density(modified_water_fog_density(-2.0, 2.0, true), 4.0);
        // Integrality is tested after the clamp, as in the reference: 10.5 clamps to
        // 10, which is integral, so this is a plain (real) power, not a rescue.
        assert_density(modified_water_fog_density(-2.0, 10.5, true), 1024.0);
    }

    /// An app with the two systems and a poisoned-or-default environment.
    fn fog_app() -> App {
        let mut app = App::new();
        // The asset plugin, because this system reads the `AssetEvent` messages
        // Bevy raises for a material — `init_asset` alone does not register them.
        app.add_plugins(AssetPlugin::default());
        app.init_resource::<EnvironmentState>()
            .init_resource::<WaterLevel>()
            .init_resource::<RenderOverrides>()
            .init_resource::<WaterFogSettings>()
            .init_asset::<FaceMaterial>()
            .add_systems(Update, update_water_fog_settings)
            // The same split the plugin uses: the material sweep runs after Bevy has
            // flushed the asset events, so a material added this frame is seen this
            // frame.
            .add_systems(
                PostUpdate,
                apply_water_fog_to_face_materials.after(AssetEventSystems),
            );
        app
    }

    /// **Both** densities reach the material, not the one the eye happens to need:
    /// which applies is decided in the shader from the view position, so a camera
    /// crossing the waterline never rewrites a material.
    #[test]
    fn a_face_material_carries_both_densities() -> Result<(), Box<dyn core::error::Error>> {
        let mut app = fog_app();
        // A water frame with a modifier, so the two densities differ.
        {
            let mut environment = app
                .world_mut()
                .get_resource_mut::<EnvironmentState>()
                .ok_or("the environment resource was just inserted")?;
            for water in environment.settings.day_cycle.water_frames.values_mut() {
                water.water_fog_density = 4.0;
                water.underwater_fog_mod = 0.5;
            }
        }
        let handle = app
            .world_mut()
            .resource_mut::<Assets<FaceMaterial>>()
            .add(FaceMaterial {
                base: StandardMaterial::default(),
                extension: SlFaceExt::inert(),
            });

        app.update();

        let materials = app.world().resource::<Assets<FaceMaterial>>();
        let params = materials
            .get(&handle)
            .ok_or("the material is still alive")?
            .extension
            .params;
        assert_density(params.water_fog_color.w, 4.0);
        assert_density(params.water_fog_submerged_density, 2.0);
        Ok(())
    }

    /// A material **added after** the settings settled still gets them: the steady
    /// state is the interesting one, because objects stream in for the whole session
    /// while the water changes only when the region does.
    #[test]
    fn a_material_added_later_is_filled_in() -> Result<(), Box<dyn core::error::Error>> {
        let mut app = fog_app();
        {
            let mut environment = app
                .world_mut()
                .get_resource_mut::<EnvironmentState>()
                .ok_or("the environment resource was just inserted")?;
            for water in environment.settings.day_cycle.water_frames.values_mut() {
                water.water_fog_density = 3.0;
            }
        }
        // A first frame settles the resource, with no materials in the world at all.
        app.update();

        let handle = app
            .world_mut()
            .resource_mut::<Assets<FaceMaterial>>()
            .add(FaceMaterial {
                base: StandardMaterial::default(),
                extension: SlFaceExt::inert(),
            });
        app.update();

        let materials = app.world().resource::<Assets<FaceMaterial>>();
        let params = materials
            .get(&handle)
            .ok_or("the material is still alive")?
            .extension
            .params;
        assert_density(params.water_fog_color.w, 3.0);
        Ok(())
    }

    /// A settled scene must not rewrite a single material: a write marks the asset
    /// changed, and a changed material is a re-prepared bind group — for every face
    /// in the region, every frame. So once the fog is in place, the `Modified` event
    /// the write itself raised has to come back to a system that finds nothing to do.
    #[test]
    fn a_settled_scene_stops_rewriting() -> Result<(), Box<dyn core::error::Error>> {
        let mut app = fog_app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<FaceMaterial>>()
            .add(FaceMaterial {
                base: StandardMaterial::default(),
                extension: SlFaceExt::inert(),
            });
        // Two frames to settle: the first writes the fog, the second sees the
        // `Modified` event that write raised and finds nothing to do.
        app.update();
        app.update();
        // Forget everything raised while settling, so the next frame's events are
        // this system's own doing and nobody else's.
        let _drained: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<AssetEvent<FaceMaterial>>>()
            .drain()
            .collect();

        app.update();

        let modified = app
            .world_mut()
            .resource_mut::<Messages<AssetEvent<FaceMaterial>>>()
            .drain()
            .filter(|event| matches!(event, AssetEvent::Modified { .. }))
            .count();
        assert_eq!(
            modified, 0,
            "a settled scene must raise no further material modifications",
        );
        // …and the fog is still there, so "stopped rewriting" is not "stopped
        // working".
        let materials = app.world().resource::<Assets<FaceMaterial>>();
        let params = materials
            .get(&handle)
            .ok_or("the material is still alive")?
            .extension
            .params;
        assert!(
            (params.water_level - WaterLevel::default().0).abs() < f32::EPSILON,
            "the settled material still carries the water level, got {}",
            params.water_level,
        );
        Ok(())
    }
}
