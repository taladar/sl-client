//! A live object to serialise, for the crate's own tests.
//!
//! [`sl_proto::Object`] has no `Default` — every field is something a simulator
//! states — so a test that wants one has to write all of it. This writes it
//! once.

use sl_proto::{
    Object, ObjectExtraParams, ObjectMotion, PrimShapeParams, RegionHandle, RegionLocalObjectId,
    TextureEntry, encode_texture_entry,
};
use sl_types::lsl::Vector;
use uuid::Uuid;

use crate::model::{IDENTITY_ROTATION, ZERO_VECTOR};

/// A one-metre box prim keyed `full_id`, textured with `entry` — the simplest
/// object a take could be handed.
pub(crate) fn box_object(full_id: Uuid, entry: &TextureEntry) -> Object {
    Object {
        region_handle: RegionHandle(0),
        local_id: RegionLocalObjectId(42),
        circuit: sl_proto::CircuitId::default(),
        full_id: full_id.into(),
        parent_id: RegionLocalObjectId(0),
        pcode: 9,
        state: 80,
        crc: 35,
        material: 3,
        click_action: 0,
        update_flags: 0,
        scale: Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        motion: ObjectMotion {
            position: Vector {
                x: 128.0,
                y: 128.0,
                z: 25.0,
            },
            velocity: ZERO_VECTOR,
            acceleration: ZERO_VECTOR,
            rotation: IDENTITY_ROTATION,
            angular_velocity: ZERO_VECTOR,
            collision_plane: None,
        },
        owner_id: Uuid::nil(),
        sound: Uuid::nil(),
        gain: 0.0,
        sound_flags: 0,
        sound_radius: 0.0,
        text: String::new(),
        text_color: [0, 0, 0, 255],
        name_value: String::new(),
        media_url: None,
        texture_entry: encode_texture_entry(entry),
        texture_anim: Vec::new(),
        texture_animation: None,
        shape: PrimShapeParams {
            // A box: a straight path, a square profile, no cut and no taper.
            path_curve: 0x10,
            profile_curve: 0x01,
            path_end: 0,
            path_scale_x: 100,
            path_scale_y: 100,
            ..PrimShapeParams::default()
        },
        particle_system: Vec::new(),
        particles: None,
        data: Vec::new(),
        extra_params: Vec::new(),
        extra: ObjectExtraParams::default(),
        properties: None,
        joint_type: 0,
        joint_pivot: ZERO_VECTOR,
        joint_axis_or_anchor: ZERO_VECTOR,
    }
}
