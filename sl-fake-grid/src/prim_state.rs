//! A prim as a script reads and writes it ([`Prim`]), from and to the
//! region's record of it — the `Object` a viewer is sent, its properties and
//! its seat.
//!
//! The library has already applied every rule of the imitated grid by the
//! time a prim comes back (`sl_lsl_runtime::library::prim_params`), so this
//! module only translates. Two things make that more than a field copy:
//!
//! - **The wire holds less than a grid keeps.** A face's colour travels as a
//!   byte and its texture offset as a 16-bit number, while Second Life reads
//!   back the float a script set; a spin travels as an angular velocity, not
//!   as an axis, a rate and a gain; a normal map, a sit flag or a damage value
//!   travels not at all. So the last prim a script wrote is kept beside the
//!   record ([`Scripted`]) with what it was written as, and a read takes each
//!   part of it — the faces, the text, the shape, the light, the flexible
//!   path, the spin — from there while that part of the record is still what
//!   was written. A viewer's edit since changes the record, and the read then
//!   decodes the record instead.
//! - **The hover text's alpha is flipped on the wire** (`255 − opacity`, so
//!   opaque packs as zero), which OpenSim reads back as the alpha itself:
//!   that is why its `llSetText(…, 1.0)` reads 0 (`prim/params`). Second
//!   Life reads the opacity.

use sl_lsl_runtime::vm::prim::{
    AttachedSound, Face, Flexible, HoverText, Light, Map, Omega, ParticleSystem, ParticleValue,
    Prim, Projector, ReflectionProbe, Sculpt, Shape, TextureAnim,
};
use sl_proto::{
    FlexibleData, LightData, Object, PrimShapeParams, ReflectionProbe as WireProbe,
    RegionLocalObjectId, RenderMaterialRef, SculptData, SoundFlags, TextureAnimation, TextureFace,
    prim_flags,
};
use sl_types::key::{ObjectKey, TextureKey};
use sl_types::lsl::Vector;

use crate::relays::ScriptSounds;
use crate::world::SceneFixtures;

/// The last prim a script wrote, and the record it was written as.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scripted {
    /// The prim as the script left it.
    pub(crate) prim: Prim,
    /// The record as that write left it.
    pub(crate) written: Object,
}

/// `NULL_KEY`.
const NULL: &str = "00000000-0000-0000-0000-000000000000";

/// The cut quantum of a profile or path (`CUT_QUANTA`).
const CUT_QUANTA: f32 = 0.000_02;

/// `PRIM_SCULPT_TYPE_MESH`.
const SCULPT_MESH: u8 = 5;

/// A byte read as a unit value.
fn unit_of(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

/// A unit value as a rounded byte.
fn byte_of(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    u8::try_from(whole(scaled).clamp(0, 255)).unwrap_or(u8::MAX)
}

/// `value`, already whole, as an `i32`, saturating at the type's bounds and
/// 0 for NaN.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the value is clamped into the i32 range and whole before the cast"
)]
const fn whole(value: f32) -> i32 {
    if value.is_nan() {
        return 0;
    }
    value.trunc().clamp(-2_147_483_648.0, 2_147_483_520.0) as i32
}

/// `value` rounded to the nearest integer as `llround` does, then to a byte
/// wrapping as a C cast does.
fn wrapped_byte(value: f32) -> u8 {
    let rounded = whole(value.round());
    u8::try_from(rounded.rem_euclid(256)).unwrap_or(0)
}

/// A signed byte read back as hundredths.
fn hundredths(byte: u8) -> f32 {
    f32::from(byte.cast_signed()) / 100.0
}

/// A value as the signed byte of hundredths LL packs it in.
fn signed_hundredths(value: f32) -> u8 {
    let rounded = whole((value / 0.01).round()).clamp(-128, 127);
    i8::try_from(rounded).unwrap_or(0).cast_unsigned()
}

/// A cut quantum count as LL packs a begin (`llround(value / CUT_QUANTA)`).
fn quanta(value: f32) -> u16 {
    let rounded = whole((value / CUT_QUANTA).round()).clamp(0, 50_000);
    u16::try_from(rounded).unwrap_or(0)
}

/// A shape from the record's volume parameters and sculpt.
fn shape_of(object: &Object) -> Shape {
    let wire = &object.shape;
    let sculpt = object.extra.sculpt.map(|sculpt| Sculpt {
        map: sculpt.texture.uuid().to_string(),
        kind: i32::from(sculpt.sculpt_type),
    });
    Shape {
        profile_curve: wire.profile_curve,
        path_curve: wire.path_curve,
        profile_begin: f32::from(wire.profile_begin) * CUT_QUANTA,
        profile_end: 1.0 - f32::from(wire.profile_end) * CUT_QUANTA,
        hollow: f32::from(wire.profile_hollow) * CUT_QUANTA,
        path_begin: f32::from(wire.path_begin) * CUT_QUANTA,
        path_end: 1.0 - f32::from(wire.path_end) * CUT_QUANTA,
        scale: [
            (200.0 - f32::from(wire.path_scale_x)) * 0.01,
            (200.0 - f32::from(wire.path_scale_y)) * 0.01,
        ],
        shear: [hundredths(wire.path_shear_x), hundredths(wire.path_shear_y)],
        twist_begin: f32::from(wire.path_twist_begin) / 100.0,
        twist_end: f32::from(wire.path_twist) / 100.0,
        radius_offset: f32::from(wire.path_radius_offset) / 100.0,
        taper: [
            f32::from(wire.path_taper_x) / 100.0,
            f32::from(wire.path_taper_y) / 100.0,
        ],
        revolutions: f32::from(wire.path_revolutions).mul_add(0.015, 1.0),
        skew: f32::from(wire.path_skew) / 100.0,
        sculpt,
    }
}

/// The record's volume parameters for `shape`, packed as LL packs them.
fn wire_shape(shape: &Shape) -> PrimShapeParams {
    let [scale_x, scale_y] = shape.scale;
    let [shear_x, shear_y] = shape.shear;
    let [taper_x, taper_y] = shape.taper;
    let scale_byte = |scale: f32| wrapped_byte(200.0 - scale / 0.01);
    let signed = |value: f32| signed_hundredths(value).cast_signed();
    PrimShapeParams {
        path_curve: shape.path_curve,
        profile_curve: shape.profile_curve,
        path_begin: quanta(shape.path_begin),
        path_end: quanta(1.0 - shape.path_end),
        path_scale_x: scale_byte(scale_x),
        path_scale_y: scale_byte(scale_y),
        path_shear_x: signed_hundredths(shear_x),
        path_shear_y: signed_hundredths(shear_y),
        path_twist: signed(shape.twist_end),
        path_twist_begin: signed(shape.twist_begin),
        path_radius_offset: signed(shape.radius_offset),
        path_taper_x: signed(taper_x),
        path_taper_y: signed(taper_y),
        path_revolutions: wrapped_byte((shape.revolutions - 1.0) / 0.015),
        path_skew: signed(shape.skew),
        profile_begin: quanta(shape.profile_begin),
        profile_end: quanta(1.0 - shape.profile_end),
        profile_hollow: quanta(shape.hollow),
    }
}

/// The record's sculpt for `shape`.
fn wire_sculpt(shape: &Shape) -> Option<SculptData> {
    let sculpt = shape.sculpt.as_ref()?;
    let id = uuid::Uuid::parse_str(&sculpt.map).ok()?;
    let kind = u8::try_from(sculpt.kind.rem_euclid(256)).unwrap_or(0);
    let texture = if kind & 0x07 == SCULPT_MESH {
        sl_proto::SculptOrMeshKey::Mesh(sl_types::key::MeshKey::from(id))
    } else {
        sl_proto::SculptOrMeshKey::Sculpt(TextureKey::from(id))
    };
    Some(SculptData {
        texture,
        sculpt_type: kind,
    })
}

/// A face from the record's texture-entry face.
fn face_of(face: &TextureFace) -> Face {
    let [red, green, blue, alpha] = face.color;
    Face {
        texture: Map {
            texture: face.texture_id.uuid().to_string(),
            repeats: Vector {
                x: face.scale_s,
                y: face.scale_t,
                z: 0.0,
            },
            offsets: Vector {
                x: face.offset_s,
                y: face.offset_t,
                z: 0.0,
            },
            rotation: face.rotation,
        },
        color: Vector {
            x: unit_of(red),
            y: unit_of(green),
            z: unit_of(blue),
        },
        alpha: unit_of(alpha),
        bump: i32::from(face.bumpmap()),
        shiny: i32::from(face.shininess()),
        fullbright: face.fullbright(),
        glow: face.glow,
        texgen: i32::from(face.tex_gen() >> 1),
        ..Face::default()
    }
}

/// The record's texture-entry face for `face`, its media bit kept from
/// `before`.
fn wire_face(face: &Face, before: Option<&TextureFace>) -> TextureFace {
    let texture = uuid::Uuid::parse_str(&face.texture.texture).unwrap_or_default();
    let bump = u8::try_from(face.bump.clamp(0, 31)).unwrap_or(0);
    let shiny = u8::try_from(face.shiny.clamp(0, 3)).unwrap_or(0);
    let media = before.map_or(0, |before| before.media_flags & 0x01);
    let texgen = u8::try_from(face.texgen.clamp(0, 3)).unwrap_or(0);
    TextureFace {
        texture_id: TextureKey::from(texture),
        color: [
            byte_of(face.color.x),
            byte_of(face.color.y),
            byte_of(face.color.z),
            byte_of(face.alpha),
        ],
        scale_s: face.texture.repeats.x,
        scale_t: face.texture.repeats.y,
        offset_s: face.texture.offsets.x.clamp(-1.0, 1.0),
        offset_t: face.texture.offsets.y.clamp(-1.0, 1.0),
        rotation: face.texture.rotation,
        bump_shiny_fullbright: bump | (u8::from(face.fullbright) << 5) | (shiny << 6),
        media_flags: media | (texgen << 1),
        glow: face.glow.clamp(0.0, 1.0),
        material_id: before.and_then(|before| before.material_id),
    }
}

/// The hover text from the record, its alpha read as the imitated grid
/// reads it.
fn text_of(object: &Object, opensim_flavour: bool) -> HoverText {
    let [red, green, blue, flipped] = object.text_color;
    HoverText {
        text: object.text.clone(),
        color: Vector {
            x: unit_of(red),
            y: unit_of(green),
            z: unit_of(blue),
        },
        alpha: if opensim_flavour {
            unit_of(flipped)
        } else {
            1.0 - unit_of(flipped)
        },
    }
}

/// The record's text colour for `text`.
fn wire_text_color(text: &HoverText, opensim_flavour: bool) -> [u8; 4] {
    let flipped = if opensim_flavour {
        byte_of(text.alpha)
    } else {
        255_u8.saturating_sub(byte_of(text.alpha))
    };
    [
        byte_of(text.color.x),
        byte_of(text.color.y),
        byte_of(text.color.z),
        flipped,
    ]
}

/// The light from the record: on while it has one.
fn light_of(object: &Object) -> Light {
    object.extra.light.map_or(
        Light {
            on: false,
            color: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            intensity: 1.0,
            radius: 0.0,
            falloff: 0.0,
        },
        |light| {
            let [red, green, blue, intensity] = light.color;
            Light {
                on: true,
                color: Vector {
                    x: unit_of(red),
                    y: unit_of(green),
                    z: unit_of(blue),
                },
                intensity: unit_of(intensity),
                radius: light.radius,
                falloff: light.falloff,
            }
        },
    )
}

/// The record's light for `light`: none while it is off.
fn wire_light(light: &Light, before: Option<&LightData>) -> Option<LightData> {
    light.on.then(|| LightData {
        color: [
            byte_of(light.color.x),
            byte_of(light.color.y),
            byte_of(light.color.z),
            byte_of(light.intensity),
        ],
        radius: light.radius,
        cutoff: before.map_or(0.0, |before| before.cutoff),
        falloff: light.falloff,
    })
}

/// The flexible path from the record: on while it has one.
fn flexible_of(object: &Object) -> Flexible {
    object.extra.flexible.as_ref().map_or(
        Flexible {
            on: false,
            softness: 0,
            gravity: 0.0,
            friction: 0.0,
            wind: 0.0,
            tension: 0.0,
            force: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        },
        |flexible| Flexible {
            on: true,
            softness: i32::from(flexible.softness),
            gravity: flexible.gravity,
            friction: flexible.air_friction,
            wind: flexible.wind_sensitivity,
            tension: flexible.tension,
            force: flexible.user_force.clone(),
        },
    )
}

/// The record's flexible path for `flexible`: none while it is off.
fn wire_flexible(flexible: &Flexible) -> Option<FlexibleData> {
    flexible.on.then(|| FlexibleData {
        softness: u8::try_from(flexible.softness.clamp(0, 3)).unwrap_or(0),
        tension: flexible.tension,
        air_friction: flexible.friction,
        gravity: flexible.gravity,
        wind_sensitivity: flexible.wind,
        user_force: flexible.force.clone(),
    })
}

/// The spin from the record's angular velocity: the axis it points along,
/// its length the rate.
fn omega_of(object: &Object) -> Omega {
    let velocity = &object.motion.angular_velocity;
    let spin = velocity
        .z
        .mul_add(
            velocity.z,
            velocity.y.mul_add(velocity.y, velocity.x * velocity.x),
        )
        .sqrt();
    if spin <= f32::EPSILON {
        return Omega {
            axis: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            spin: 0.0,
            gain: 0.0,
        };
    }
    Omega {
        axis: Vector {
            x: velocity.x / spin,
            y: velocity.y / spin,
            z: velocity.z / spin,
        },
        spin,
        gain: 1.0,
    }
}

/// The texture animation from the record.
fn texture_anim_of(object: &Object) -> Option<TextureAnim> {
    object.texture_animation.as_ref().map(|anim| TextureAnim {
        mode: i32::from(anim.mode),
        face: i32::from(anim.face),
        size_x: i32::from(anim.size_x),
        size_y: i32::from(anim.size_y),
        start: anim.start,
        length: anim.length,
        rate: anim.rate,
    })
}

/// The record's texture animation for `anim`.
fn wire_texture_anim(anim: &TextureAnim) -> TextureAnimation {
    let byte = |value: i32| u8::try_from(value.rem_euclid(256)).unwrap_or(0);
    TextureAnimation {
        mode: byte(anim.mode),
        face: i8::try_from(anim.face.clamp(-128, 127)).unwrap_or(-1),
        size_x: byte(anim.size_x),
        size_y: byte(anim.size_y),
        start: anim.start,
        length: anim.length,
        rate: anim.rate,
    }
}

/// A colour and an alpha as the RGBA bytes a particle system carries.
fn rgba(colour: &Vector, alpha: f32) -> [u8; 4] {
    [
        byte_of(colour.x),
        byte_of(colour.y),
        byte_of(colour.z),
        byte_of(alpha),
    ]
}

/// The record's particle system for `system`: Second Life's defaults, then
/// each rule in the order given.
fn wire_particles(system: &ParticleSystem) -> sl_proto::ParticleSystem {
    let mut wire = sl_proto::ParticleSystem {
        crc: 1,
        flags: 0,
        pattern: 0x01,
        max_age: 0.0,
        start_age: 0.0,
        inner_angle: 0.0,
        outer_angle: 0.0,
        burst_rate: 0.1,
        burst_radius: 0.0,
        burst_speed_min: 1.0,
        burst_speed_max: 1.0,
        burst_part_count: 1,
        angular_velocity: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        acceleration: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        texture_id: None,
        target_id: None,
        part_flags: 0,
        part_max_age: 10.0,
        part_start_color: [255; 4],
        part_end_color: [255; 4],
        part_start_scale: [1.0, 1.0],
        part_end_scale: [1.0, 1.0],
        part_start_glow: 0.0,
        part_end_glow: 0.0,
        part_blend_func_source: 7,
        part_blend_func_dest: 9,
    };
    let (mut start_colour, mut end_colour) = (
        Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
    );
    let (mut start_alpha, mut end_alpha) = (1.0, 1.0);
    for (code, value) in &system.rules {
        let float = match value {
            ParticleValue::Float(float) => *float,
            _ => 0.0,
        };
        let integer = match value {
            ParticleValue::Integer(integer) => *integer,
            _ => 0,
        };
        let vector = match value {
            ParticleValue::Vector(vector) => vector.clone(),
            _ => Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let key = match value {
            ParticleValue::Key(key) => uuid::Uuid::parse_str(key).ok(),
            _ => None,
        };
        let byte = |value: i32| u8::try_from(value.clamp(0, 255)).unwrap_or(0);
        match code {
            0 => wire.part_flags = u32::try_from(integer).unwrap_or(0),
            1 => start_colour = vector,
            2 => start_alpha = float,
            3 => end_colour = vector,
            4 => end_alpha = float,
            5 => wire.part_start_scale = [vector.x, vector.y],
            6 => wire.part_end_scale = [vector.x, vector.y],
            7 => wire.part_max_age = float,
            8 => wire.acceleration = vector,
            9 => wire.pattern = byte(integer),
            10 | 22 => wire.inner_angle = float,
            11 | 23 => wire.outer_angle = float,
            12 => wire.texture_id = key.map(TextureKey::from),
            13 => wire.burst_rate = float,
            15 => wire.burst_part_count = byte(integer),
            16 => wire.burst_radius = float,
            17 => wire.burst_speed_min = float,
            18 => wire.burst_speed_max = float,
            19 => wire.max_age = float,
            20 => wire.target_id = key.map(ObjectKey::from),
            21 => wire.angular_velocity = vector,
            24 => wire.part_blend_func_source = byte(integer),
            25 => wire.part_blend_func_dest = byte(integer),
            26 => wire.part_start_glow = float,
            27 => wire.part_end_glow = float,
            _ => {}
        }
    }
    wire.part_start_color = rgba(&start_colour, start_alpha);
    wire.part_end_color = rgba(&end_colour, end_alpha);
    wire
}

/// The sound the record plays on.
fn playing_of(object: &Object) -> Option<AttachedSound> {
    if object.sound.is_nil() {
        return None;
    }
    let flags = SoundFlags(object.sound_flags);
    Some(AttachedSound {
        sound: object.sound.to_string(),
        volume: object.gain,
        looped: flags.contains(SoundFlags::LOOP),
        master: flags.contains(SoundFlags::SYNC_MASTER),
        slave: flags.contains(SoundFlags::SYNC_SLAVE),
    })
}

/// The reflection probe from the record.
fn probe_of(object: &Object) -> ReflectionProbe {
    object.extra.reflection_probe.map_or(
        ReflectionProbe {
            on: false,
            ambiance: 0.0,
            clip: 0.0,
            flags: 0,
        },
        |probe| ReflectionProbe {
            on: true,
            ambiance: probe.ambiance,
            clip: probe.clip_distance,
            flags: i32::from(probe.flags.bits()),
        },
    )
}

/// The projector from the record's light image.
fn projector_of(object: &Object) -> Projector {
    object.extra.light_image.as_ref().map_or_else(
        || Projector {
            on: false,
            texture: NULL.to_owned(),
            fov: 0.0,
            focus: 0.0,
            ambience: 0.0,
        },
        |image| Projector {
            on: true,
            texture: image.texture.uuid().to_string(),
            fov: image.params.x,
            focus: image.params.y,
            ambience: image.params.z,
        },
    )
}

/// The prim `local_id` of `world` as a script reads it, or [`None`] for a
/// prim the region does not have.
#[must_use]
pub(crate) fn read(
    world: &SceneFixtures,
    local_id: RegionLocalObjectId,
    opensim_flavour: bool,
) -> Option<Prim> {
    let object = world.objects.get(local_id)?.clone();
    let properties = world.properties_of(local_id)?;
    let root_id = world.root_of(local_id);
    let (position, rotation) = crate::seating::frame_of(world, &object);
    let root_rotation = world.objects.get(root_id).map_or_else(
        || rotation.clone(),
        |root| crate::seating::frame_of(world, root).1,
    );
    let worn = world.objects.attached_to(root_id).is_some();
    let local_position = if root_id == local_id && !worn {
        position.clone()
    } else {
        object.motion.position.clone()
    };
    let shape = shape_of(&object);
    let count = usize::try_from(sl_lsl_runtime::library::faces_of(&shape))
        .unwrap_or(1)
        .max(1);
    let entry = sl_proto::decode_texture_entry(&object.texture_entry, count);
    let mut faces: Vec<Face> = entry.faces.iter().map(face_of).collect();
    faces.resize(count, faces.first().cloned().unwrap_or_default());
    for material in &object.extra.render_material {
        if let Some(face) = faces.get_mut(usize::from(material.face)) {
            face.render_material = material.material_id.to_string();
        }
    }
    let mut prim = Prim {
        key: object.full_id.uuid().to_string(),
        creator: properties.creator_id.uuid().to_string(),
        name: properties.name.clone(),
        description: properties.description.clone(),
        position,
        local_position,
        rotation,
        local_rotation: object.motion.rotation.clone(),
        root_rotation,
        scale: object.scale.clone(),
        shape,
        faces,
        text: text_of(&object, opensim_flavour),
        light: light_of(&object),
        flexible: flexible_of(&object),
        material: i32::from(object.material),
        phantom: object.update_flags & prim_flags::PHANTOM != 0,
        physics: object.update_flags & prim_flags::USE_PHYSICS != 0,
        temp_on_rez: object.update_flags & prim_flags::TEMPORARY_ON_REZ != 0,
        omega: omega_of(&object),
        click_action: i32::from(object.click_action),
        reflection_probe: probe_of(&object),
        projector: projector_of(&object),
        texture_anim: texture_anim_of(&object),
        ..Prim::new_box(&object.full_id.uuid().to_string(), NULL)
    };
    prim.media = vec![None; count];
    prim.sound.playing = playing_of(&object);
    prim.sound.radius = object.sound_radius;
    if let Some(scripted) = world.objects.scripted(local_id) {
        keep_written(&mut prim, &object, scripted);
    }
    Some(prim)
}

/// Takes from what a script last wrote whatever the record still holds as
/// it was written, and what the record holds nothing of.
fn keep_written(prim: &mut Prim, object: &Object, scripted: &Scripted) {
    let (kept, written) = (&scripted.prim, &scripted.written);
    if object.texture_entry == written.texture_entry && kept.faces.len() == prim.faces.len() {
        prim.faces.clone_from(&kept.faces);
    } else {
        // What the wire does not carry of a face is the script's still.
        for (face, kept) in prim.faces.iter_mut().zip(&kept.faces) {
            face.normal.clone_from(&kept.normal);
            face.specular.clone_from(&kept.specular);
            face.alpha_mode = kept.alpha_mode;
            face.mask_cutoff = kept.mask_cutoff;
            face.gltf.clone_from(&kept.gltf);
        }
    }
    if object.text == written.text && object.text_color == written.text_color {
        prim.text.clone_from(&kept.text);
    }
    if object.shape == written.shape && object.extra.sculpt == written.extra.sculpt {
        prim.shape.clone_from(&kept.shape);
    }
    if object.extra.light == written.extra.light {
        prim.light.clone_from(&kept.light);
    }
    if object.extra.flexible == written.extra.flexible {
        prim.flexible.clone_from(&kept.flexible);
    }
    if object.motion.angular_velocity == written.motion.angular_velocity {
        prim.omega.clone_from(&kept.omega);
    }
    if object.particle_system == written.particle_system {
        prim.particles.clone_from(&kept.particles);
    }
    if object.extra.light_image == written.extra.light_image {
        prim.projector.clone_from(&kept.projector);
    }
    if object.extra.reflection_probe == written.extra.reflection_probe {
        prim.reflection_probe.clone_from(&kept.reflection_probe);
    }
    if kept.media.len() == prim.media.len() {
        prim.media.clone_from(&kept.media);
    }
    prim.has_media_list = kept.has_media_list;
    prim.physics_shape = kept.physics_shape;
    prim.allow_unsit = kept.allow_unsit;
    prim.scripted_sit_only = kept.scripted_sit_only;
    prim.sit_flags = kept.sit_flags;
    prim.status = kept.status;
    prim.volume_detect = kept.volume_detect;
    prim.damage = kept.damage;
    prim.health = kept.health;
    prim.sound.queueing = kept.sound.queueing;
    prim.sound.stopped = kept.sound.stopped;
    prim.sound.collision.clone_from(&kept.sound.collision);
    if prim.sound.playing.is_some() {
        prim.sound.playing.clone_from(&kept.sound.playing);
    }
}

/// Writes what `prim` plays on into `object`, `before` the record as it was,
/// as `told` states a prim's sound: the sound, its gain and its loop and sync
/// flags while it plays on, nothing after; the queueing switch in the flags
/// whenever it is on and a stop until something plays again on Second
/// Life, on OpenSim the queueing switch in a loop's flags alone and a stop
/// until anything plays (`SoundModule`). Second Life keeps the radius the
/// script gave; OpenSim gives a prim that sounds with none its 100 m.
fn write_sound(object: &mut Object, before: &Object, prim: &Prim, told: ScriptSounds) {
    let messages = told == ScriptSounds::Messages;
    let queue = if prim.sound.queueing {
        SoundFlags::QUEUE
    } else {
        0
    };
    match &prim.sound.playing {
        Some(playing) => {
            object.sound = uuid::Uuid::parse_str(&playing.sound).unwrap_or_default();
            object.gain = playing.volume;
            let mut flags = queue;
            if playing.looped {
                flags |= SoundFlags::LOOP;
            }
            if playing.master {
                flags |= SoundFlags::SYNC_MASTER;
            }
            if playing.slave {
                flags |= SoundFlags::SYNC_SLAVE;
            }
            object.sound_flags = flags;
        }
        None => {
            object.sound = uuid::Uuid::nil();
            object.gain = 0.0;
            let stop = if prim.sound.stopped {
                SoundFlags::STOP
            } else {
                0
            };
            object.sound_flags = stop | if messages { queue } else { 0 };
        }
    }
    let sounded = prim.sound.playing.is_some() || prim.sound.stopped || before.sound_radius > 0.0;
    object.sound_radius = if messages || prim.sound.radius > 0.0 {
        prim.sound.radius
    } else if sounded {
        crate::relays::OPENSIM_SOUND_RANGE_M
    } else {
        0.0
    };
}

/// Whether `object` differs from `before` only in what `told` changes on a
/// prim without an update: on OpenSim its sound's gain, radius and flags
/// while the sound itself is the same and no loop's flags changed — a change
/// of volume, of radius, a played sound's end of a stop.
fn sound_changed_silently(before: &Object, object: &Object, told: ScriptSounds) -> bool {
    if told == ScriptSounds::Messages || object.sound != before.sound {
        return false;
    }
    if !object.sound.is_nil() && object.sound_flags != before.sound_flags {
        return false;
    }
    let unsounded = Object {
        gain: before.gain,
        sound_flags: before.sound_flags,
        sound_radius: before.sound_radius,
        ..object.clone()
    };
    unsounded == *before
}

/// What writing a prim changed, for the host to apply after the tick.
#[derive(Debug, Default)]
pub(crate) struct Written {
    /// The record changed: the tick's burst sends it.
    pub(crate) changed: bool,
    /// The name or description changed: viewers holding the record's
    /// properties are told.
    pub(crate) renamed: bool,
}

/// Writes `prim` back as the prim `local_id` of `world`, `ground` the
/// height of the ground under a root's new position. A root that would end
/// outside the region stays where it was.
pub(crate) fn write(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    prim: &Prim,
    opensim_flavour: bool,
    ground: impl Fn(f32, f32) -> f32,
    edge: f32,
) -> Written {
    let mut written = Written::default();
    let root_id = world.root_of(local_id);
    let worn = world.objects.attached_to(root_id).is_some();
    let Some(before) = world.objects.get(local_id).cloned() else {
        return written;
    };
    let mut object = before.clone();
    let mut position = prim.local_position.clone();
    if root_id == local_id && !worn {
        let inside = (0.0..edge).contains(&position.x) && (0.0..edge).contains(&position.y);
        if inside {
            position.z = position.z.max(ground(position.x, position.y));
        } else {
            position = before.motion.position.clone();
        }
    }
    object.motion.position = position;
    object.motion.rotation = prim.local_rotation.clone();
    object.motion.angular_velocity = Vector {
        x: prim.omega.axis.x * prim.omega.spin,
        y: prim.omega.axis.y * prim.omega.spin,
        z: prim.omega.axis.z * prim.omega.spin,
    };
    object.scale = prim.scale.clone();
    object.shape = wire_shape(&prim.shape);
    object.extra.sculpt = wire_sculpt(&prim.shape);
    let count = prim.faces.len().max(1);
    let entry = sl_proto::decode_texture_entry(&before.texture_entry, count);
    let faces: Vec<TextureFace> = prim
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| wire_face(face, entry.faces.get(index)))
        .collect();
    object.texture_entry = sl_proto::encode_texture_entry(&sl_proto::TextureEntry { faces });
    object.text.clone_from(&prim.text.text);
    object.text_color = wire_text_color(&prim.text, opensim_flavour);
    object.extra.light = wire_light(&prim.light, before.extra.light.as_ref());
    object.extra.flexible = wire_flexible(&prim.flexible);
    object.extra.render_material = prim
        .faces
        .iter()
        .enumerate()
        .filter(|(_index, face)| face.render_material != NULL)
        .filter_map(|(index, face)| {
            Some(RenderMaterialRef {
                face: u8::try_from(index).ok()?,
                material_id: uuid::Uuid::parse_str(&face.render_material).ok()?,
            })
        })
        .collect();
    object.extra.reflection_probe = prim.reflection_probe.on.then(|| WireProbe {
        ambiance: prim.reflection_probe.ambiance,
        clip_distance: prim.reflection_probe.clip,
        flags: sl_proto::ReflectionProbeFlags::from_bits(
            u8::try_from(prim.reflection_probe.flags.clamp(0, 255)).unwrap_or(0),
        ),
    });
    object.extra.light_image = prim.projector.on.then(|| sl_proto::LightImage {
        texture: TextureKey::from(
            uuid::Uuid::parse_str(&prim.projector.texture).unwrap_or_default(),
        ),
        params: Vector {
            x: prim.projector.fov,
            y: prim.projector.focus,
            z: prim.projector.ambience,
        },
    });
    object.extra_params = sl_proto::encode_extra_params(&object.extra);
    object.material = u8::try_from(prim.material.clamp(0, 255)).unwrap_or(3);
    let flag = |on: bool, bit: u32, flags: u32| if on { flags | bit } else { flags & !bit };
    object.update_flags = flag(prim.phantom, prim_flags::PHANTOM, object.update_flags);
    object.update_flags = flag(prim.physics, prim_flags::USE_PHYSICS, object.update_flags);
    object.update_flags = flag(
        prim.temp_on_rez,
        prim_flags::TEMPORARY_ON_REZ,
        object.update_flags,
    );
    object.click_action = u8::try_from(prim.click_action.rem_euclid(256)).unwrap_or(0);
    object.texture_animation = prim.texture_anim.as_ref().map(wire_texture_anim);
    object.texture_anim = object
        .texture_animation
        .as_ref()
        .map_or_else(Vec::new, sl_proto::encode_texture_anim);
    let particles = prim.particles.as_ref().map(wire_particles);
    object.particle_system = particles
        .as_ref()
        .map_or_else(Vec::new, sl_proto::encode_particle_system);
    object.particles = particles;
    write_sound(&mut object, &before, prim, world.relays.script_sounds);
    if object != before {
        written.changed = !sound_changed_silently(&before, &object, world.relays.script_sounds);
        if let Some(mut stored) = world.object_mut(local_id) {
            if written.changed {
                *stored = object.clone();
            } else {
                *stored.quietly() = object.clone();
            }
        }
    }
    if let Some(mut properties) = world.properties_of(local_id)
        && (properties.name != prim.name || properties.description != prim.description)
    {
        properties.name.clone_from(&prim.name);
        properties.description.clone_from(&prim.description);
        if let Some(mut stored) = world.object_mut(local_id) {
            stored.properties = Some(properties);
        }
        written.renamed = true;
    }
    let written_record = world.objects.get(local_id).cloned().unwrap_or(object);
    world.objects.set_scripted(
        local_id,
        Scripted {
            prim: prim.clone(),
            written: written_record,
        },
    );
    written
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_lsl_runtime::vm::prim::{AttachedSound, Prim};
    use sl_proto::{Object, RegionLocalObjectId, SoundFlags};
    use sl_types::key::{AgentKey, ObjectKey};
    use sl_types::lsl::Vector;

    use super::{sound_changed_silently, write_sound};
    use crate::relays::ScriptSounds;

    /// The origin.
    const ZERO: Vector = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// A box nothing has sounded from.
    fn record() -> Object {
        crate::world::box_prim(
            RegionLocalObjectId(1),
            ObjectKey::from(uuid::Uuid::from_u128(0x10)),
            AgentKey::from(uuid::Uuid::from_u128(0x1)),
            ZERO,
            ZERO,
        )
    }

    /// The box's prim after `llStopSound`, its queueing switch as given.
    fn stopped(queueing: bool) -> Prim {
        let mut prim = Prim::new_box("box", "");
        prim.sound.stopped = true;
        prim.sound.queueing = queueing;
        prim
    }

    /// `record` with `prim`'s sound written as `told` writes it.
    fn written(record: &Object, prim: &Prim, told: ScriptSounds) -> Object {
        let mut object = record.clone();
        write_sound(&mut object, record, prim, told);
        object
    }

    /// A stop stays on the prim with the queueing switch on Second Life;
    /// OpenSim states the stop alone and gives the prim its 100 m.
    #[test]
    fn a_stop_is_stated_on_the_prim_as_each_grid_states_it() {
        let second_life = written(&record(), &stopped(true), ScriptSounds::Messages);
        assert_eq!(
            second_life.sound_flags,
            SoundFlags::STOP | SoundFlags::QUEUE
        );
        assert!(second_life.sound_radius.abs() < f32::EPSILON);
        let opensim = written(&record(), &stopped(true), ScriptSounds::OnThePrim);
        assert_eq!(opensim.sound_flags, SoundFlags::STOP);
        assert!((opensim.sound_radius - crate::relays::OPENSIM_SOUND_RANGE_M).abs() < f32::EPSILON);
    }

    /// OpenSim changes a loop's volume and the prim's radius without an
    /// update, and states a stop; Second Life states every change.
    #[test]
    fn opensim_changes_a_volume_and_a_radius_unannounced() {
        let mut looping = Prim::new_box("box", "");
        looping.sound.playing = Some(AttachedSound {
            sound: uuid::Uuid::from_u128(0x50).to_string(),
            volume: 0.5,
            looped: true,
            master: false,
            slave: false,
        });
        let before = written(&record(), &looping, ScriptSounds::OnThePrim);
        let mut quieter = looping.clone();
        if let Some(playing) = quieter.sound.playing.as_mut() {
            playing.volume = 0.25;
        }
        let after = written(&before, &quieter, ScriptSounds::OnThePrim);
        assert!(sound_changed_silently(
            &before,
            &after,
            ScriptSounds::OnThePrim
        ));
        assert!(!sound_changed_silently(
            &before,
            &after,
            ScriptSounds::Messages
        ));
        let mut wider = looping.clone();
        wider.sound.radius = 5.0;
        let after = written(&before, &wider, ScriptSounds::OnThePrim);
        assert!(sound_changed_silently(
            &before,
            &after,
            ScriptSounds::OnThePrim
        ));
        let after = written(&before, &stopped(false), ScriptSounds::OnThePrim);
        assert!(!sound_changed_silently(
            &before,
            &after,
            ScriptSounds::OnThePrim
        ));
    }
}
