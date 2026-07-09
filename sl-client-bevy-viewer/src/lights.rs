//! Local lights (Phase 25): fold a prim's `LLLightParams` light block into the
//! scene mirror.
//!
//! This is the P25.1 slice — *ingest only*. Each in-world prim may carry a light
//! extra-param ([`LightData`]) marking it as a light
//! source, and — when it is a spotlight (projector) — a companion light-image
//! extra-param ([`LightImage`](sl_client_bevy::LightImage)) holding the projected
//! texture and its cone parameters. [`light_from_object`] decodes those two
//! blocks into an [`ObjectLight`] component, which [`apply_object`] attaches to
//! (or clears from) each object entity as its updates arrive. P25.2 will read
//! this component to spawn the nearest / brightest N Bevy `PointLight` /
//! `SpotLight`s.
//!
//! [`apply_object`]: crate::objects
//!
//! Reference (read-only): Firestorm `LLVOVolume::getLight*` /
//! `isLightSpotlight` (`indra/newview/llvovolume.cpp`) and
//! `LLLightParams` / `LLLightImageParams`
//! (`indra/llprimitive/llprimitive.{h,cpp}`).

use bevy::prelude::*;
use sl_client_bevy::{LightData, Object, TextureKey};

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

#[cfg(test)]
mod tests {
    use super::{ObjectLight, light_from_object};
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
}
