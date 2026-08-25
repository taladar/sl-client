//! Local lights (Phase 25): fold a prim's `LLLightParams` light block into the
//! scene mirror and render it as a Bevy light.
//!
//! **Ingest (P25.1).** Each in-world prim may carry a light extra-param
//! ([`LightData`](sl_client_bevy::LightData)) marking it as a light source, and — when it is a spotlight
//! (projector) — a companion light-image extra-param
//! ([`LightImage`](sl_client_bevy::LightImage)) holding the projected texture and
//! its cone parameters. `light_from_object` decodes those two blocks into an
//! [`ObjectLight`] component, which `apply_object` attaches to (or clears from)
//! each object entity as its updates arrive.
//!
//! **Render (P25.2).** [`drive_local_lights`] reads those [`ObjectLight`]
//! components each frame and spawns a Bevy [`PointLight`] (or [`SpotLight`] for a
//! projector) as a child of the light-flagged object entity, so the Bevy light
//! rides the prim's transform. Only the nearest / brightest `MAX_LOCAL_LIGHTS`
//! prims win the budget each frame, mirroring the way
//! `LLPipeline::setupHWLights` keeps only the closest `LL_NUM_LIGHT_UNITS` — the
//! rest are dropped so the clustered-forward renderer is not overwhelmed. The
//! Bevy light is parented with an identity local transform, so its forward
//! (`-Z`) already equals the Second Life spot direction (the prim's local `-Z`,
//! `at_axis(0,0,-1) * render_rotation`) once the parent's coordinate conversion
//! is applied.
//!
//! `apply_object`: crate::objects
//!
//! Reference (read-only): Firestorm `LLVOVolume::getLight*` /
//! `isLightSpotlight` (`indra/newview/llvovolume.cpp`),
//! `LLPipeline::setupHWLights` (`indra/newview/pipeline.cpp`), and
//! `LLLightParams` / `LLLightImageParams`
//! (`indra/llprimitive/llprimitive.{h,cpp}`).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::sky::SCENE_LIGHT_ILLUMINANCE;
use crate::world_api::{LightProjection, ObjectLight, ViewerCamera};

/// The maximum number of local prim lights rendered at once (P25.2). Second
/// Life's legacy fixed-function path capped hardware lights at
/// `LL_NUM_LIGHT_UNITS` (8); its deferred renderer raises the nearby-light limit
/// (`RenderLocalLightCount`) far higher. Bevy's clustered-forward renderer bounds
/// the per-cluster light count, so we spend a middling scene-wide budget on the
/// nearest / brightest prims each frame.
const MAX_LOCAL_LIGHTS: usize = 32;

/// Second Life scales a local light's reach past its nominal radius: the deferred
/// renderer's light-volume `size` uniform is `getLightRadius() * 1.5`
/// (`LLPipeline::renderDeferredLighting`), and the surface is unlit past it. Our
/// Bevy `range` mirrors that so a light reaches exactly as far as it does in
/// Firestorm.
const SL_RADIUS_TO_RANGE: f32 = 1.5;

/// The falloff "fudge factor" Second Life folds into the shader falloff:
/// `getLightFalloff(DEFERRED_LIGHT_FALLOFF)` with `DEFERRED_LIGHT_FALLOFF == 0.5`
/// (`pipeline.cpp`). So the wire falloff (`0.0..=2.0`) becomes a shader falloff of
/// `0.0..=1.0` before entering [`legacy_distance_attenuation`].
const DEFERRED_LIGHT_FALLOFF: f32 = 0.5;

/// The fraction of a local light's reach (`size`) at which its Bevy lumens are
/// calibrated to match Second Life's surface contribution — half the reach, a
/// representative "surface being lit" distance. Calibrating here (rather than at
/// the light's centre) keeps Bevy's inverse-square point light from reading as the
/// wildly-over-bright facelight the un-scaled `VERY_LARGE_CINEMA_LIGHT` default
/// produced: at the reference distance the light matches Firestorm, and only the
/// unavoidable near-field of the inverse-square curve (which Second Life's bounded
/// falloff does not have) rises above it.
const REFERENCE_REACH_FRACTION: f32 = 0.5;

/// Second Life's local-light distance attenuation, ported verbatim from
/// `calcLegacyDistanceAttenuation` (`deferredUtil.glsl`): a clamped quadratic that
/// falls to zero at the light's reach, peaking near the centre — **bounded**,
/// unlike a physical inverse-square. `dist` is the surface distance normalised by
/// the light's `size` (`0.0` at the centre, `1.0` at the reach); `falloff` is the
/// already-fudged shader falloff (`wire_falloff * DEFERRED_LIGHT_FALLOFF`).
fn legacy_distance_attenuation(dist: f32, falloff: f32) -> f32 {
    // `(distance + falloff) / (1 + falloff)`, clamped, then squared and doubled —
    // the reference's "tweak falloff slightly to match pre-EEP attenuation".
    let ramp = ((dist + falloff) / (1.0 + falloff)).clamp(0.0, 1.0);
    let atten = 1.0 - ramp;
    atten * atten * 2.0
}

/// The Bevy photometric power (lumens) for a local light, calibrated so its
/// illuminance at [`REFERENCE_REACH_FRACTION`] of the light's reach matches Second
/// Life's surface contribution there relative to the scene sun
/// ([`SCENE_LIGHT_ILLUMINANCE`]).
///
/// Second Life's contribution to a lit surface is
/// `linear_color * intensity * legacy_distance_attenuation(dist, falloff)` — a
/// bounded, dimensionless add competing with the sun in the same linear space. A
/// Bevy point light's illuminance is `lumens / (4π d²)`, so matching the two at the
/// reference distance `d_ref = size * REFERENCE_REACH_FRACTION` gives
/// `lumens = 4π d_ref² * SCENE_LIGHT_ILLUMINANCE * intensity * atten`. The result
/// scales with the light's `size²` (bigger lights reach proportionally brighter)
/// and with its falloff, exactly as Firestorm does — and is dramatically dimmer
/// than the old flat `VERY_LARGE_CINEMA_LIGHT` constant, which read a worn
/// facelight as a floodlight.
fn local_light_lumens(light: &ObjectLight) -> f32 {
    let size = light.radius * SL_RADIUS_TO_RANGE;
    let falloff = light.falloff * DEFERRED_LIGHT_FALLOFF;
    let atten = legacy_distance_attenuation(REFERENCE_REACH_FRACTION, falloff);
    let d_ref = size * REFERENCE_REACH_FRACTION;
    // `4π d_ref² · sun · intensity · atten` — the illuminance-match solved for
    // lumens, written as a straight product (plain `f32` locals).
    let sphere = 4.0 * core::f32::consts::PI * d_ref * d_ref;
    sphere * SCENE_LIGHT_ILLUMINANCE * light.intensity * atten
}

/// The smallest spotlight cone half-angle (radians) handed to a Bevy
/// [`SpotLight`]: Bevy requires a positive outer angle, so a near-zero projector
/// FOV is clamped up to this.
const MIN_SPOT_ANGLE: f32 = 0.05;
/// The largest spotlight cone half-angle (radians) handed to a Bevy
/// [`SpotLight`]: Bevy requires the outer angle strictly below `π/2`, so a wide
/// projector FOV is clamped down to just under it.
const MAX_SPOT_ANGLE: f32 = core::f32::consts::FRAC_PI_2 - 0.01;

/// Marks a Bevy light entity spawned by [`drive_local_lights`] as the render of a
/// prim's [`ObjectLight`]. Parented to the light-flagged object entity so it is
/// never confused with the object geometry.
#[derive(Component)]
pub(crate) struct LocalLightChild;

/// The persistent mapping from a light-flagged object entity to the Bevy light
/// child [`drive_local_lights`] spawned for it (P25.2).
///
/// The light entities are **kept alive across frames** and updated in place — a
/// prim only gains a light child when it enters the render budget and loses it
/// when it drops out. Despawning / re-spawning the Bevy light every frame instead
/// churns the render world and makes the light flicker, so the selection is
/// reconciled against this map rather than rebuilt from scratch.
#[derive(Debug, Resource, Default)]
pub struct LocalLights {
    /// Light-flagged object entity → its spawned Bevy light child entity and the
    /// last [`ObjectLight`] applied to it. The stored light lets the reconcile
    /// skip a prim whose light is unchanged, so a stable scene does no per-frame
    /// component churn at all.
    assigned: HashMap<Entity, (Entity, ObjectLight)>,
}

/// The Rec. 709 relative luminance of a linear RGB colour — used to rank lights
/// by how bright they read, so a dim tinted light does not outbid a strong one.
fn luminance(color: [f32; 3]) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

/// Build the [`PointLight`] for a plain (non-projector) local light.
fn point_light(light: &ObjectLight) -> PointLight {
    PointLight {
        color: light_color(light),
        // The colour carries the hue; the intensity (the wire alpha) is folded into
        // the calibrated photometric power, so radiance stays proportional to the
        // emitted colour and matches Firestorm's surface contribution.
        intensity: local_light_lumens(light),
        // Second Life unlits the surface past `radius * 1.5` (the deferred `size`);
        // Bevy's smooth range window mirrors that reach.
        range: light.radius * SL_RADIUS_TO_RANGE,
        radius: 0.0,
        ..default()
    }
}

/// Build the [`SpotLight`] for a projector local light, its cone taken from the
/// projector's field of view.
fn spot_light(projection: LightProjection, light: &ObjectLight) -> SpotLight {
    // The projector field of view is the *full* cone angle (`LLLightImageParams`
    // defaults it to `F_PI * 0.5`); Bevy's outer angle is the half-angle from the
    // cone axis.
    let outer = (projection.fov * 0.5).clamp(MIN_SPOT_ANGLE, MAX_SPOT_ANGLE);
    // The projector focus sharpens the cone edge: a higher focus pulls the
    // fully-lit inner cone out toward the outer edge (a harder falloff).
    let inner = outer * projection.focus.clamp(0.0, 1.0);
    SpotLight {
        color: light_color(light),
        intensity: local_light_lumens(light),
        range: light.radius * SL_RADIUS_TO_RANGE,
        radius: 0.0,
        inner_angle: inner,
        outer_angle: outer,
        ..default()
    }
}

/// The Bevy [`Color`] for a local light: its linear RGB hue (the intensity rides
/// the photometric power, not the colour).
const fn light_color(light: &ObjectLight) -> Color {
    Color::linear_rgb(
        light.linear_color[0],
        light.linear_color[1],
        light.linear_color[2],
    )
}

/// Spawn a fresh Bevy light child for a light-flagged prim entering the render
/// budget (P25.2), returning its entity.
///
/// A plain point light becomes a [`PointLight`]; a projector (spotlight) becomes
/// a [`SpotLight`]. Both are parented to the object entity with an identity local
/// transform, so the light sits at the prim's origin and a spotlight's forward
/// already points down the prim's Second Life local `-Z` (see the module docs).
fn spawn_local_light(commands: &mut Commands, object: Entity, light: &ObjectLight) -> Entity {
    // The light rides its object entity, whose `Propagate(RenderLayers)` (set in
    // `objects::apply_object`) reaches this child — so a prim light inherits the
    // object's `{main + probe}` layers and illuminates both the main view and the
    // local probe capture cameras, exactly as it did when both were on layer 0.
    let mut child = commands.spawn((Transform::IDENTITY, LocalLightChild, ChildOf(object)));
    match light.projection {
        Some(projection) => child.insert(spot_light(projection, light)),
        None => child.insert(point_light(light)),
    };
    child.id()
}

/// Refresh an existing light child's parameters in place (P25.2), so a prim whose
/// light was retuned — or toggled between point and spot — stays current without
/// a despawn / re-spawn. Removes the counterpart light component so a point↔spot
/// switch never leaves both on one entity.
fn update_local_light(commands: &mut Commands, child: Entity, light: &ObjectLight) {
    let mut entity = commands.entity(child);
    match light.projection {
        Some(projection) => {
            entity.insert(spot_light(projection, light));
            entity.remove::<PointLight>();
        }
        None => {
            entity.insert(point_light(light));
            entity.remove::<SpotLight>();
        }
    }
}

/// Render the nearest / brightest light-flagged prims as Bevy lights (P25.2).
///
/// Ranks every light-flagged prim by its emitted luminance attenuated by camera
/// distance — the nearest / brightest win the fixed `MAX_LOCAL_LIGHTS` budget,
/// mirroring `LLPipeline::setupHWLights`. A prim with a black or zero-radius light
/// contributes nothing and is skipped so it does not waste a slot. The winners'
/// Bevy light children are **kept alive and updated in place** across frames (see
/// [`LocalLights`]); a prim only gains a child on entering the budget and loses it
/// on dropping out — re-spawning every frame flickers the render world.
pub fn drive_local_lights(
    mut commands: Commands,
    mut assigned: ResMut<LocalLights>,
    camera: Query<&GlobalTransform, With<ViewerCamera>>,
    lights: Query<(Entity, &ObjectLight, &GlobalTransform)>,
    // The count rendered last frame, so a change (a light coming into / out of
    // the budget) logs once instead of every frame.
    mut last_rendered: Local<usize>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();

    let mut ranked: Vec<(Entity, f32)> = lights
        .iter()
        .filter_map(|(entity, light, transform)| {
            let brightness = luminance(light.effective_linear_color());
            if brightness <= f32::EPSILON || light.radius <= f32::EPSILON {
                return None;
            }
            // Clamp the denominator so a light the camera sits inside does not
            // score infinite; nearer / brighter still ranks higher.
            let distance2 = eye.distance_squared(transform.translation()).max(1.0);
            Some((entity, brightness / distance2))
        })
        .collect();
    // Highest score first, then keep only the budget.
    let candidates = ranked.len();
    ranked.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    ranked.truncate(MAX_LOCAL_LIGHTS);

    if ranked.len() != *last_rendered {
        debug!(
            "local lights: rendering {} of {candidates} candidate prim light(s) \
             (budget {MAX_LOCAL_LIGHTS})",
            ranked.len(),
        );
        *last_rendered = ranked.len();
    }

    // Retire the light children of prims that fell out of the budget (or whose
    // object despawned — Bevy's hierarchy already took the child, so `try_despawn`
    // is a safe no-op there). Retaining leaves only entries for the selected,
    // still-alive objects, so the refresh loop below never inserts into a dead
    // entity.
    let selected: std::collections::HashSet<Entity> = ranked.iter().map(|&(e, _)| e).collect();
    assigned.assigned.retain(|object, (child, _)| {
        if selected.contains(object) {
            true
        } else {
            commands.entity(*child).try_despawn();
            false
        }
    });

    // Insert a child for each newly selected prim; refresh the rest only when the
    // light actually changed, so a stable scene does no per-frame ECS churn.
    for (entity, _score) in ranked {
        // The entity came straight from `lights.iter()` this frame, so the lookup
        // cannot miss; skip defensively rather than unwrap.
        let Ok((_, light, _)) = lights.get(entity) else {
            continue;
        };
        match assigned.assigned.get_mut(&entity) {
            Some((child, applied)) => {
                if *applied != *light {
                    update_local_light(&mut commands, *child, light);
                    *applied = *light;
                }
            }
            None => {
                let child = spawn_local_light(&mut commands, entity, light);
                assigned.assigned.insert(entity, (child, *light));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectLight, legacy_distance_attenuation, local_light_lumens, luminance};
    use crate::world_api::light_from_object;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{LightData, LightImage, Object, TextureKey, Uuid, Vector};

    /// Tolerance for the 8-bit-quantized colour round-trips (the workspace denies
    /// strict float comparison).
    const EPS: f32 = 1.0e-6;

    /// Assert two floats are equal within [`EPS`].
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    /// Assert two RGB triples are elementwise equal within [`EPS`].
    fn close3(a: [f32; 3], b: [f32; 3]) -> bool {
        close(a[0], b[0]) && close(a[1], b[1]) && close(a[2], b[2])
    }

    /// A minimal plain prim object with no extra params — the fixture the light
    /// tests decorate.
    fn bare_object() -> Object {
        use sl_client_bevy::{
            CircuitId, ObjectMotion, RegionHandle, RegionLocalObjectId, Rotation,
        };
        // A fresh zero vector per use (`Vector` derives neither `Copy` nor
        // `Default`).
        const fn zero() -> Vector {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode: 9,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: ObjectMotion {
                position: zero(),
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::from_u128(0),
            sound: Uuid::from_u128(0),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// An object with no light block is not a light source.
    #[test]
    fn no_light_block_is_none() {
        assert_eq!(light_from_object(&bare_object()), None);
    }

    /// A point light (light block, no light-image) decodes to a non-spotlight with
    /// its colour split into a linear RGB and a separate intensity (the alpha).
    #[test]
    fn point_light_decodes_without_projection() {
        let mut object = bare_object();
        object.extra.light = Some(LightData {
            // Half-red, quarter alpha.
            color: [255, 0, 0, 64],
            radius: 8.0,
            cutoff: 0.0,
            falloff: 1.5,
        });
        let Some(light) = light_from_object(&object) else {
            unreachable!("a light block decodes to a light")
        };
        assert!(!light.is_spotlight());
        assert!(close3(light.linear_color, [1.0, 0.0, 0.0]));
        assert!(close(light.intensity, 64.0 / 255.0));
        assert!(close(light.radius, 8.0));
        assert!(close(light.falloff, 1.5));
        assert_eq!(light.projection, None);
        // The emitted colour scales the base by the intensity.
        assert!(close3(
            light.effective_linear_color(),
            [64.0 / 255.0, 0.0, 0.0]
        ));
    }

    /// A light that also carries a light-image block decodes as a spotlight, with
    /// the projector texture and its (fov, focus, ambiance) params.
    #[test]
    fn spotlight_carries_projection() {
        let mut object = bare_object();
        object.extra.light = Some(LightData {
            color: [0, 255, 0, 255],
            radius: 5.0,
            cutoff: 45.0,
            falloff: 1.0,
        });
        let texture = TextureKey::from(Uuid::from_u128(42));
        object.extra.light_image = Some(LightImage {
            texture,
            params: Vector {
                x: 1.2,
                y: 0.3,
                z: 0.5,
            },
        });
        let Some(light) = light_from_object(&object) else {
            unreachable!("a light block decodes to a light")
        };
        assert!(light.is_spotlight());
        assert!(close(light.cutoff, 45.0));
        let Some(projection) = light.projection else {
            unreachable!("a light-image block decodes to a projection")
        };
        assert_eq!(projection.texture, texture);
        assert!(close(projection.fov, 1.2));
        assert!(close(projection.focus, 0.3));
        assert!(close(projection.ambiance, 0.5));
    }

    /// The full-intensity white default emits full white.
    #[test]
    fn full_white_emits_white() {
        let light = ObjectLight {
            linear_color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            radius: 5.0,
            falloff: 1.0,
            cutoff: 0.0,
            projection: None,
        };
        assert!(close3(light.effective_linear_color(), [1.0, 1.0, 1.0]));
    }

    /// `legacy_distance_attenuation` matches the reference `deferredUtil.glsl`
    /// curve: bounded, zero at the reach, and peaking at the centre — never the
    /// unbounded inverse-square that made facelights blow out.
    #[test]
    fn legacy_attenuation_is_bounded_reference_curve() {
        // At the centre (`dist == 0`) with zero falloff: `(1 - 0)² * 2 == 2`.
        assert!(close(legacy_distance_attenuation(0.0, 0.0), 2.0));
        // Halfway with zero falloff: `(1 - 0.5)² * 2 == 0.5`.
        assert!(close(legacy_distance_attenuation(0.5, 0.0), 0.5));
        // At the reach it is exactly zero regardless of falloff.
        assert!(close(legacy_distance_attenuation(1.0, 0.0), 0.0));
        assert!(close(legacy_distance_attenuation(1.0, 1.0), 0.0));
        // Past the reach it stays clamped at zero (never negative).
        assert!(close(legacy_distance_attenuation(2.0, 0.5), 0.0));
        // A sharper falloff dims the mid-range: higher falloff → smaller value.
        assert!(legacy_distance_attenuation(0.5, 1.0) < legacy_distance_attenuation(0.5, 0.0));
    }

    /// The worn facelight from the captured dump (white, intensity `153/255`,
    /// radius `1.0`, falloff `0.75`) now reads as a gentle fill: its calibrated
    /// lumens are a tiny fraction of the old flat `1_000_000 * intensity`
    /// (≈ 600 000) that blew out the face.
    #[test]
    fn facelight_lumens_are_a_gentle_fill_not_a_floodlight() {
        let facelight = ObjectLight {
            linear_color: [1.0, 1.0, 1.0],
            intensity: 153.0 / 255.0,
            radius: 1.0,
            falloff: 0.75,
            cutoff: 0.0,
            projection: None,
        };
        let lumens = local_light_lumens(&facelight);
        // Far below the old `VERY_LARGE_CINEMA_LIGHT * intensity` (≈ 600 000).
        assert!(
            lumens < 50_000.0,
            "facelight lumens {lumens} still floodlight-bright"
        );
        // Still a positive, meaningful contribution (not extinguished).
        assert!(lumens > 1_000.0, "facelight lumens {lumens} vanished");
    }

    /// The calibrated lumens rise with both the light's radius (a bigger light
    /// reaches proportionally brighter, `∝ size²`) and its intensity.
    #[test]
    fn lumens_scale_with_radius_and_intensity() {
        let base = ObjectLight {
            linear_color: [1.0, 1.0, 1.0],
            intensity: 0.5,
            radius: 2.0,
            falloff: 1.0,
            cutoff: 0.0,
            projection: None,
        };
        let mut bigger = base;
        bigger.radius = 4.0;
        let mut brighter = base;
        brighter.intensity = 1.0;
        assert!(local_light_lumens(&bigger) > local_light_lumens(&base));
        assert!(local_light_lumens(&brighter) > local_light_lumens(&base));
        // Doubling the intensity doubles the power (it is a linear factor); a
        // relative tolerance, as the kilolumen magnitudes exceed `EPS`.
        let doubled = local_light_lumens(&base) * 2.0;
        let got = local_light_lumens(&brighter);
        assert!((got - doubled).abs() < doubled * 1.0e-5);
    }

    /// White is brighter than any single primary, and green outweighs red /
    /// blue — so the P25.2 budget ranks a strong light above a dim tinted one.
    #[test]
    fn luminance_ranks_by_perceived_brightness() {
        let white = luminance([1.0, 1.0, 1.0]);
        let green = luminance([0.0, 1.0, 0.0]);
        let red = luminance([1.0, 0.0, 0.0]);
        let blue = luminance([0.0, 0.0, 1.0]);
        assert!(close(white, 1.0));
        assert!(green > red);
        assert!(red > blue);
        assert!(close(luminance([0.0, 0.0, 0.0]), 0.0));
    }
}
