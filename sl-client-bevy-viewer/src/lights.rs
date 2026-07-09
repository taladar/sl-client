//! Local lights (Phase 25): fold a prim's `LLLightParams` light block into the
//! scene mirror and render it as a Bevy light.
//!
//! **Ingest (P25.1).** Each in-world prim may carry a light extra-param
//! ([`LightData`]) marking it as a light source, and — when it is a spotlight
//! (projector) — a companion light-image extra-param
//! ([`LightImage`](sl_client_bevy::LightImage)) holding the projected texture and
//! its cone parameters. [`light_from_object`] decodes those two blocks into an
//! [`ObjectLight`] component, which [`apply_object`] attaches to (or clears from)
//! each object entity as its updates arrive.
//!
//! **Render (P25.2).** [`drive_local_lights`] reads those [`ObjectLight`]
//! components each frame and spawns a Bevy [`PointLight`] (or [`SpotLight`] for a
//! projector) as a child of the light-flagged object entity, so the Bevy light
//! rides the prim's transform. Only the nearest / brightest [`MAX_LOCAL_LIGHTS`]
//! prims win the budget each frame, mirroring the way
//! `LLPipeline::setupHWLights` keeps only the closest `LL_NUM_LIGHT_UNITS` — the
//! rest are dropped so the clustered-forward renderer is not overwhelmed. The
//! Bevy light is parented with an identity local transform, so its forward
//! (`-Z`) already equals the Second Life spot direction (the prim's local `-Z`,
//! `at_axis(0,0,-1) * render_rotation`) once the parent's coordinate conversion
//! is applied.
//!
//! [`apply_object`]: crate::objects
//!
//! Reference (read-only): Firestorm `LLVOVolume::getLight*` /
//! `isLightSpotlight` (`indra/newview/llvovolume.cpp`),
//! `LLPipeline::setupHWLights` (`indra/newview/pipeline.cpp`), and
//! `LLLightParams` / `LLLightImageParams`
//! (`indra/llprimitive/llprimitive.{h,cpp}`).

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{LightData, Object, TextureKey};

use crate::camera::FlyCamera;

/// The maximum number of local prim lights rendered at once (P25.2). Second
/// Life's legacy fixed-function path capped hardware lights at
/// `LL_NUM_LIGHT_UNITS` (8); its deferred renderer raises the nearby-light limit
/// (`RenderLocalLightCount`) far higher. Bevy's clustered-forward renderer bounds
/// the per-cluster light count, so we spend a middling scene-wide budget on the
/// nearest / brightest prims each frame.
const MAX_LOCAL_LIGHTS: usize = 32;

/// The luminous power (lumens) of a full-intensity (`intensity == 1.0`) local
/// light. Second Life light intensity is `0.0..=1.0`; this scales it into Bevy's
/// photometric lumens. Set to Bevy's own `VERY_LARGE_CINEMA_LIGHT` default so a
/// full-strength prim light reads brightly at the scene's default exposure
/// without washing out the sunlit `SCENE_LIGHT_ILLUMINANCE` (10,000 lux).
const LOCAL_LIGHT_LUMENS: f32 = 1_000_000.0;

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
#[derive(Resource, Default)]
pub(crate) struct LocalLights {
    /// Light-flagged object entity → its spawned Bevy light child entity and the
    /// last [`ObjectLight`] applied to it. The stored light lets the reconcile
    /// skip a prim whose light is unchanged, so a stable scene does no per-frame
    /// component churn at all.
    assigned: HashMap<Entity, (Entity, ObjectLight)>,
}

/// The projector parameters of a **spotlight** — a light that carries a
/// light-image ([`LightImage`](sl_client_bevy::LightImage)) extra-param and so
/// projects a texture within a cone (`LLVOVolume::isLightSpotlight`). A plain
/// point light has none of this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LightProjection {
    /// The projected texture id (`LLLightImageParams::getLightTexture`).
    pub(crate) texture: TextureKey,
    /// The projector cone field-of-view, in radians (`params.mV[0]`).
    pub(crate) fov: f32,
    /// The projector focus / blur (`params.mV[1]`).
    pub(crate) focus: f32,
    /// The projector ambiance — the diffuse spill outside the cone
    /// (`params.mV[2]`).
    pub(crate) ambiance: f32,
}

/// A component marking an object entity as a **light source**, carrying the
/// decoded `LLLightParams` (and, for a spotlight, `LLLightImageParams`)
/// parameters in Second Life semantics — ready for P25.2 to convert into a Bevy
/// `PointLight` / `SpotLight`.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// [`apply_object`](crate::objects) as its updates arrive.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ObjectLight {
    /// The light's **linear** RGB colour, each channel in `0.0..=1.0`. The wire
    /// bytes are the linear (not gamma-corrected) colour — Firestorm's
    /// `LLLightParams::unpack` feeds them straight into `setLinearColor` — so no
    /// sRGB decode is applied here.
    pub(crate) linear_color: [f32; 3],
    /// The light intensity in `0.0..=1.0` — the alpha channel of the wire colour
    /// (`LLVOVolume::getLightIntensity` reads `getLinearColor().mV[3]`). The
    /// effective emitted colour is `linear_color * intensity`.
    pub(crate) intensity: f32,
    /// The light radius, in metres (`LIGHT_MIN_RADIUS`..=`LIGHT_MAX_RADIUS`,
    /// i.e. `0.0..=20.0`).
    pub(crate) radius: f32,
    /// The falloff exponent (`LIGHT_MIN_FALLOFF`..=`LIGHT_MAX_FALLOFF`, i.e.
    /// `0.0..=2.0`): how sharply the light dims toward its radius.
    pub(crate) falloff: f32,
    /// The spotlight cutoff cone half-angle, in degrees
    /// (`LIGHT_MIN_CUTOFF`..=`LIGHT_MAX_CUTOFF`, i.e. `0.0..=180.0`). Sent for
    /// every light but only meaningful for a projector.
    pub(crate) cutoff: f32,
    /// The projector parameters when this is a **spotlight** (it carries a
    /// light-image block); `None` for a plain point light.
    pub(crate) projection: Option<LightProjection>,
}

impl ObjectLight {
    /// Whether this light is a **spotlight** (projector) rather than a plain
    /// point light — true exactly when it carries projector parameters, mirroring
    /// `LLVOVolume::isLightSpotlight` (a light-image block is present).
    pub(crate) const fn is_spotlight(&self) -> bool {
        self.projection.is_some()
    }

    /// The light's effective emitted linear colour: its base colour scaled by its
    /// intensity, mirroring `LLVOVolume::getLightLinearColor`
    /// (`color * color.mV[3]`).
    pub(crate) const fn effective_linear_color(&self) -> [f32; 3] {
        [
            self.linear_color[0] * self.intensity,
            self.linear_color[1] * self.intensity,
            self.linear_color[2] * self.intensity,
        ]
    }
}

/// Convert one wire colour byte to a normalized `0.0..=1.0` float. The workspace
/// denies `as` casts, so the widening goes through [`f32::from`].
fn channel(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

/// Decode an object's light extra-params into an [`ObjectLight`], or `None` if the
/// object is not a light source (it carries no `LLLightParams` block).
///
/// A spotlight additionally carries a light-image block; when present it becomes
/// the [`projection`](ObjectLight::projection).
pub(crate) fn light_from_object(object: &Object) -> Option<ObjectLight> {
    let light: LightData = object.extra.light?;
    let projection = object
        .extra
        .light_image
        .as_ref()
        .map(|image| LightProjection {
            texture: image.texture,
            fov: image.params.x,
            focus: image.params.y,
            ambiance: image.params.z,
        });
    Some(ObjectLight {
        linear_color: [
            channel(light.color[0]),
            channel(light.color[1]),
            channel(light.color[2]),
        ],
        intensity: channel(light.color[3]),
        radius: light.radius,
        falloff: light.falloff,
        cutoff: light.cutoff,
        projection,
    })
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
        // The colour carries the hue; the intensity (the wire alpha) rides the
        // photometric power, so radiance stays proportional to the emitted colour.
        intensity: LOCAL_LIGHT_LUMENS * light.intensity,
        range: light.radius,
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
        intensity: LOCAL_LIGHT_LUMENS * light.intensity,
        range: light.radius,
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
/// distance — the nearest / brightest win the fixed [`MAX_LOCAL_LIGHTS`] budget,
/// mirroring `LLPipeline::setupHWLights`. A prim with a black or zero-radius light
/// contributes nothing and is skipped so it does not waste a slot. The winners'
/// Bevy light children are **kept alive and updated in place** across frames (see
/// [`LocalLights`]); a prim only gains a child on entering the budget and loses it
/// on dropping out — re-spawning every frame flickers the render world.
pub(crate) fn drive_local_lights(
    mut commands: Commands,
    mut assigned: ResMut<LocalLights>,
    camera: Query<&GlobalTransform, With<FlyCamera>>,
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
    use super::{ObjectLight, light_from_object, luminance};
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
