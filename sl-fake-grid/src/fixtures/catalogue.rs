//! The named catalogue: one region's worth of prims, one per rendering
//! feature, in a row a camera can sweep.
//!
//! This is the fixture the full-stack viewer harness, the `sl-conformance`
//! `Grid::Fake` cases and anyone pointing Firestorm at the standalone binary
//! all load, so "the mesh prim" means the same object with the same id at the
//! same place in every one of them. Each entry is a [`CatalogueEntry`] naming
//! what it is and where it stands; a test looks its subject up by
//! [`entry`] rather than hard-coding a local id.
//!
//! The prims stand in a west-to-east row a few metres north of the region's
//! arrival point, spaced far enough apart that each projects to its own patch
//! of screen.

use sl_proto::{
    AnimationKey, AssetKey, EnvironmentSettings, FlexibleData, InMemoryAssetSource, LightData,
    LightImage, MediaEntry, ObjectMediaState, ParticleSystem, ReflectionProbe, RegionLocalObjectId,
    RegionLocalParcelId, TextureAnimation, particle_pattern, texture_anim_mode,
};
use sl_types::key::{AgentKey, InventoryKey, Key, MeshKey, ObjectKey, OwnerKey, TextureKey};
use sl_types::lsl::Vector;
use sl_wire::{LegacyMaterial, ReflectionProbeFlags};

use super::RegionFixture;
use super::npcs::{NpcAppearance, NpcFixture};
use super::prims::{FaceStyle, PrimFixture, SculptKind, linkset};
use crate::world::{AvatarIdentity, ObjectAnimationFixture, SceneFixtures, region_wide_parcel};

/// The catalogue parcel's name.
pub const CATALOGUE_PARCEL_NAME: &str = "Fake Grid Catalogue";

/// The catalogue parcel's region-local id.
pub const CATALOGUE_PARCEL_LOCAL_ID: RegionLocalParcelId = RegionLocalParcelId(1);

/// The agent that owns and created every catalogue prim.
const CATALOGUE_OWNER: u128 = 0xCA7_0000;

/// The region-local id of the first catalogue prim; each later one is the
/// next id up. Deliberately clear of the arriving avatar's id and of the stock
/// scenario's scripted object.
const FIRST_LOCAL_ID: u32 = 0x100;

/// The `y` the catalogue row stands on, in region metres: north of the
/// arrival point, so a camera looking north from the arrival sees the row.
pub const ROW_Y: f32 = 136.0;

/// The `x` of the westmost catalogue prim.
pub const ROW_FIRST_X: f32 = 108.0;

/// The gap between neighbouring catalogue prims, in metres.
pub const ROW_SPACING: f32 = 4.0;

/// The `z` the catalogue row stands on: half a metre above the stock flat
/// ground ([`STOCK_TERRAIN_HEIGHT_M`](crate::scenario::STOCK_TERRAIN_HEIGHT_M)),
/// so a one-metre prim rests on it. The
/// two are tied together by a test rather than by arithmetic, because the
/// height is a `u8` and `f32::from` is not a `const fn`.
pub const ROW_Z: f32 = 25.5;

/// The checker texture every catalogue prim that shows a texture wears.
pub const CHECKER_TEXTURE: TextureKey = texture_key(0xCA7_0001);

/// The sculpt map the sculpty prim is shaped by.
pub const SCULPT_MAP: TextureKey = texture_key(0xCA7_0002);

/// The mesh asset the mesh prim is shaped by.
pub const MESH_ASSET: MeshKey = MeshKey(Key(uuid::Uuid::from_u128(0xCA7_0003)));

/// The GLTF (PBR) material asset the PBR prim's face wears.
pub const PBR_MATERIAL: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0004);

/// The legacy (`LLMaterial`) material the bumpy prim's face names.
pub const LEGACY_MATERIAL: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0005);

/// The normal map the legacy material uses.
pub const NORMAL_MAP: TextureKey = texture_key(0xCA7_0006);

/// The texture the particle emitter throws.
pub const PARTICLE_TEXTURE: TextureKey = texture_key(0xCA7_0007);

/// The **rigged** mesh asset the rigged and animesh prims are shaped by: the
/// two-bone cylinder of [`sl_test_assets::rigged::cylinder_mesh_asset`],
/// whose upper joint is the one [`NPC_ANIMATION`] rotates.
///
/// One asset serves both prims on purpose. A rigged mesh rezzed in-world with
/// no control avatar renders at its bind pose; the same asset on an animated
/// object bends. Two prims side by side, one asset between them, is the
/// cleanest way to see which of the two paths broke.
pub const RIGGED_MESH_ASSET: MeshKey = MeshKey(Key(uuid::Uuid::from_u128(0xCA7_0008)));

/// The bright EEP sky settings asset (`AT_SETTINGS`) the catalogue serves —
/// [`sl_test_assets::environment::noon_sky_asset`].
///
/// No prim in the row names it, because an environment is not a prim: these
/// four ids exist so a viewer pointed at the catalogue has a real settings
/// asset of each kind to fetch, which is the half of EEP the typed
/// `ExtEnvironment` capability cannot reach.
pub const NOON_SKY_ASSET: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0009);

/// The dark EEP sky settings asset — [`sl_test_assets::environment::night_sky_asset`].
/// Paired with [`NOON_SKY_ASSET`]: the two differ by a luminance, so "the
/// environment changed" is measurable rather than a matter of opinion.
pub const NIGHT_SKY_ASSET: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_000A);

/// The EEP water settings asset — [`sl_test_assets::environment::water_asset`].
pub const WATER_SETTINGS_ASSET: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_000B);

/// The EEP day-cycle settings asset — [`sl_test_assets::environment::day_cycle_asset`],
/// which runs from [`NIGHT_SKY_ASSET`]'s frame to [`NOON_SKY_ASSET`]'s. This is
/// the kind an environment *inventory* item holds.
pub const DAY_CYCLE_ASSET: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_000C);

/// The catalogue NPC's agent id.
pub const NPC_AGENT: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0100);

/// The catalogue NPC's first name.
pub const NPC_FIRST_NAME: &str = "Catalogue";

/// The catalogue NPC's last name.
pub const NPC_LAST_NAME: &str = "Resident";

/// The region-local id the catalogue NPC's avatar body is rezzed with. Clear
/// of the prim row (`FIRST_LOCAL_ID`…) and of its linkset children
/// (`FIRST_LOCAL_ID + 0x80`…).
pub const NPC_LOCAL_ID: RegionLocalObjectId = RegionLocalObjectId(0x200);

/// The region-local id of the box the catalogue NPC wears.
pub const NPC_ATTACHMENT_LOCAL_ID: RegionLocalObjectId = RegionLocalObjectId(0x201);

/// The attachment point the NPC wears its box on (`ATTACH_HEAD`, the skull).
pub const NPC_ATTACHMENT_POINT: u8 = 2;

/// The inventory item the NPC's attachment is worn from.
pub const NPC_ATTACHMENT_ITEM: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0101);

/// The full id of the box the catalogue NPC wears.
pub const NPC_ATTACHMENT_OBJECT: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0102);

/// The colour the catalogue NPC's bakes are painted, so a capture can tell
/// the avatar from the prims beside it.
pub const NPC_BAKE_COLOR: [u8; 4] = sl_test_assets::markers::BLUE;

/// The animation the catalogue NPC plays. It is the catalogue's **own** id,
/// not one of the built-in Linden animation UUIDs, so the fixture never
/// pretends to be a Linden asset and the viewer has to fetch the motion over
/// `ViewerAsset` like any other animation. The bytes behind it are
/// [`sl_test_assets::anim::chest_twist_animation_asset`], registered in the
/// catalogue's asset store like every other asset a catalogue prim names.
pub const NPC_ANIMATION: uuid::Uuid = uuid::Uuid::from_u128(0xCA7_0103);

/// The `x` the catalogue NPC stands on: one slot west of the prim row, so it
/// has its own patch of screen.
pub const NPC_X: f32 = ROW_FIRST_X - ROW_SPACING;

/// The `z` of the catalogue NPC's avatar object — its **centre**, so half its
/// 1.9 m height above the stock ground.
pub const NPC_Z: f32 = 25.95;

/// A fixed [`TextureKey`], as a `const fn` (`From<Uuid>` is not one).
const fn texture_key(id: u128) -> TextureKey {
    TextureKey(Key(uuid::Uuid::from_u128(id)))
}

/// One catalogue prim: what it is, the id it is rezzed with, and where it
/// stands. A test finds its subject with [`entry`] and asserts against the
/// pixels at [`position`](Self::position).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `CatalogueEntry` reads clearly"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogueEntry {
    /// The stable name: what the prim demonstrates, in kebab case.
    pub name: &'static str,
    /// The region-local id the prim is rezzed with.
    pub local_id: RegionLocalObjectId,
    /// The prim's full (asset-space) id.
    pub full_id: ObjectKey,
}

impl CatalogueEntry {
    /// The prim's region-local position: its slot in the row.
    #[must_use]
    pub fn position(&self) -> Vector {
        Vector {
            x: ROW_FIRST_X + slot_offset(self.local_id),
            y: ROW_Y,
            z: ROW_Z,
        }
    }
}

/// How far east of [`ROW_FIRST_X`] the prim with `local_id` stands.
fn slot_offset(local_id: RegionLocalObjectId) -> f32 {
    let slot = local_id.0.saturating_sub(FIRST_LOCAL_ID);
    f32::from(u16::try_from(slot).unwrap_or(u16::MAX)) * ROW_SPACING
}

/// The names of the catalogue's prims, in the order they stand in the row.
/// The index into this list is the prim's offset from the first catalogue
/// local id, so the row order and the id order are the same thing.
pub const NAMES: [&str; 18] = [
    "plain-box",
    "checker-box",
    "sphere",
    "faced-box",
    "mesh-cube",
    "sculpt-sphere",
    "pbr-box",
    "legacy-material-box",
    "light-box",
    "flexi-blade",
    "particle-emitter",
    "texture-anim-box",
    "hover-text-box",
    "media-face-box",
    "reflection-probe",
    "linkset-root",
    "rigged-mesh",
    "animesh-cylinder",
];

/// Every catalogue entry, in row order.
#[must_use]
pub fn entries() -> Vec<CatalogueEntry> {
    NAMES
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let slot = u32::try_from(index).unwrap_or(0);
            let local_id = RegionLocalObjectId(FIRST_LOCAL_ID.saturating_add(slot));
            CatalogueEntry {
                name,
                local_id,
                full_id: ObjectKey::from(uuid::Uuid::from_u128(
                    0xCA7_1000_u128.saturating_add(u128::from(slot)),
                )),
            }
        })
        .collect()
}

/// The catalogue entry called `name`, or `None` if the catalogue has none.
#[must_use]
pub fn entry(name: &str) -> Option<CatalogueEntry> {
    entries().into_iter().find(|entry| entry.name == name)
}

/// The whole catalogue as a [`RegionFixture`]: one region-wide parcel, the row
/// of feature prims, and every asset, material and media record they need.
///
/// Assets that fail to encode are simply not registered and a warning is
/// logged — the prim referencing one then renders as a missing texture, which
/// is a visible failure rather than a panic in a fixture.
#[must_use]
pub fn catalogue() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(CATALOGUE_OWNER));
    let mut world = SceneFixtures::new();
    world.parcels.push(region_wide_parcel(
        CATALOGUE_PARCEL_LOCAL_ID,
        OwnerKey::Agent(owner),
        CATALOGUE_PARCEL_NAME,
    ));
    world.objects = objects(owner);
    world.npcs = vec![npc()];
    world.object_animations = animesh_animations();

    RegionFixture {
        world,
        assets: assets(),
        materials: vec![(LEGACY_MATERIAL, legacy_material())],
        media: vec![(media_object(), media_state())],
        environment: None::<EnvironmentSettings>,
        terrain: crate::TerrainFixture::default(),
    }
}

/// The catalogue's objects, in row order. Every builder method of
/// [`PrimFixture`] appears at least once, so the catalogue is also the
/// coverage list of what a fixture can express.
fn objects(owner: AgentKey) -> Vec<sl_proto::Object> {
    let entries = entries();
    let mut objects = Vec::new();
    let mut next_child_id = FIRST_LOCAL_ID.saturating_add(0x80);

    for entry in &entries {
        let prim = PrimFixture::boxed(
            entry.local_id,
            entry.full_id,
            owner,
            entry.position(),
            unit(),
        );
        match entry.name {
            "plain-box" => objects.push(prim.build()),
            "checker-box" => objects.push(prim.textured(CHECKER_TEXTURE).build()),
            "sphere" => objects.push(prim.shape(sphere_shape()).textured(CHECKER_TEXTURE).build()),
            "faced-box" => objects.push(faced_box(prim)),
            "mesh-cube" => objects.push(prim.mesh(MESH_ASSET, 1).textured(CHECKER_TEXTURE).build()),
            "sculpt-sphere" => objects.push(
                prim.sculpt(SCULPT_MAP, SculptKind::Sphere)
                    .textured(CHECKER_TEXTURE)
                    .build(),
            ),
            "pbr-box" => objects.push(prim.textured(CHECKER_TEXTURE).pbr(0, PBR_MATERIAL).build()),
            "legacy-material-box" => objects.push(
                prim.textured(CHECKER_TEXTURE)
                    .face(
                        0,
                        &FaceStyle {
                            material: Some(LEGACY_MATERIAL),
                            shiny: 3,
                            ..FaceStyle::default()
                        },
                    )
                    .build(),
            ),
            "light-box" => objects.push(
                prim.faces(&FaceStyle {
                    color: [255, 240, 200],
                    fullbright: true,
                    glow: 0.2,
                    ..FaceStyle::default()
                })
                .light(LightData {
                    color: [255, 220, 160, 255],
                    radius: 10.0,
                    // A non-zero cutoff makes it a spot light, which is what a
                    // projected image needs.
                    cutoff: 1.2,
                    falloff: 0.75,
                })
                .projector(LightImage {
                    texture: CHECKER_TEXTURE,
                    params: Vector {
                        x: 1.0,
                        y: 2.0,
                        z: 0.5,
                    },
                })
                .build(),
            ),
            "flexi-blade" => objects.push(
                PrimFixture::boxed(
                    entry.local_id,
                    entry.full_id,
                    owner,
                    entry.position(),
                    Vector {
                        x: 0.2,
                        y: 1.0,
                        z: 3.0,
                    },
                )
                .textured(CHECKER_TEXTURE)
                .flexi(FlexibleData {
                    softness: 2,
                    tension: 1.0,
                    air_friction: 2.0,
                    gravity: 0.3,
                    wind_sensitivity: 1.0,
                    user_force: Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                })
                .build(),
            ),
            "particle-emitter" => objects.push(prim.particles(fountain()).build()),
            "texture-anim-box" => objects.push(
                prim.textured(CHECKER_TEXTURE)
                    .texture_anim(TextureAnimation {
                        mode: texture_anim_mode::ON | texture_anim_mode::LOOP,
                        face: -1,
                        size_x: 2,
                        size_y: 2,
                        start: 0.0,
                        length: 4.0,
                        rate: 2.0,
                    })
                    .build(),
            ),
            "hover-text-box" => objects.push(
                prim.textured(CHECKER_TEXTURE)
                    .hover_text("catalogue", [255, 255, 255, 255])
                    .build(),
            ),
            "media-face-box" => {
                let mut media = prim.textured(CHECKER_TEXTURE).face(
                    0,
                    &FaceStyle {
                        media: true,
                        ..FaceStyle::default()
                    },
                );
                // The pre-MOAP whole-object `MediaURL` field beside the
                // per-face `ObjectMedia` record, so both paths are covered.
                if let Some(url) = media_url() {
                    media = media.media_url(url);
                }
                objects.push(media.build());
            }
            "reflection-probe" => objects.push(
                prim.reflection_probe(ReflectionProbe {
                    ambiance: 0.5,
                    clip_distance: 1.0,
                    flags: ReflectionProbeFlags::BOX_VOLUME,
                })
                .build(),
            ),
            "linkset-root" => {
                let children = (0_u32..2)
                    .map(|index| {
                        let child_id = RegionLocalObjectId(next_child_id);
                        next_child_id = next_child_id.saturating_add(1);
                        PrimFixture::boxed(
                            child_id,
                            ObjectKey::from(uuid::Uuid::from_u128(
                                0xCA7_2000_u128.saturating_add(u128::from(index)),
                            )),
                            owner,
                            entry.position(),
                            unit(),
                        )
                        .textured(CHECKER_TEXTURE)
                        .child_of(
                            entry.local_id,
                            Vector {
                                x: 0.0,
                                y: 0.0,
                                // One metre up per child: the linkset is a
                                // stack, so a capture can tell root from child
                                // by height alone.
                                z: 1.0 + f32::from(u16::try_from(index).unwrap_or(0)),
                            },
                            sl_types::lsl::Rotation {
                                x: 0.0,
                                y: 0.0,
                                z: 0.0,
                                s: 1.0,
                            },
                        )
                    })
                    .collect();
                objects.extend(linkset(prim.textured(CHECKER_TEXTURE), children));
            }
            // The same rigged asset twice: at its bind pose, and bending.
            "rigged-mesh" => objects.push(
                prim.mesh(RIGGED_MESH_ASSET, 1)
                    .textured(CHECKER_TEXTURE)
                    .build(),
            ),
            "animesh-cylinder" => objects.push(
                prim.mesh(RIGGED_MESH_ASSET, 1)
                    .textured(CHECKER_TEXTURE)
                    .animated_mesh()
                    .build(),
            ),
            _unknown => objects.push(prim.build()),
        }
    }
    objects
}

/// The catalogue's animated objects: the `animesh-cylinder` prim playing
/// [`NPC_ANIMATION`].
///
/// It plays the *same* motion the NPC does, and deliberately so: the animation
/// rotates `mChest`, which is the rigged cylinder's **upper** joint, so the
/// prim's top half twists while its bottom half stays put. Two subjects on one
/// motion also means a capture that finds neither of them moving is looking at
/// a broken animation asset rather than a broken animesh path.
///
/// Empty if the catalogue has no `animesh-cylinder` entry, which only happens
/// if [`NAMES`] loses it.
#[must_use]
pub fn animesh_animations() -> Vec<ObjectAnimationFixture> {
    entry("animesh-cylinder")
        .map(|animesh| {
            vec![ObjectAnimationFixture::playing(
                animesh.full_id,
                AnimationKey::from(NPC_ANIMATION),
            )]
        })
        .unwrap_or_default()
}

/// The catalogue's NPC: a blue-baked avatar standing west of the prim row,
/// playing the built-in `stand` animation and wearing a checker box on its
/// skull.
///
/// This is what the full-stack tier asserts other-avatar rendering against —
/// the body classifies as [`NPC_BAKE_COLOR`], the attachment follows the body,
/// and the name tag reads `Catalogue Resident`.
#[must_use]
pub fn npc() -> NpcFixture {
    let agent = AgentKey::from(NPC_AGENT);
    NpcFixture::new(
        NPC_LOCAL_ID,
        AvatarIdentity::new(agent, NPC_FIRST_NAME, NPC_LAST_NAME),
        Vector {
            x: NPC_X,
            y: ROW_Y,
            z: NPC_Z,
        },
    )
    .looking(NpcAppearance::solid(agent, NPC_BAKE_COLOR))
    .animating(AnimationKey::from(NPC_ANIMATION))
    .wearing(
        PrimFixture::boxed(
            NPC_ATTACHMENT_LOCAL_ID,
            ObjectKey::from(NPC_ATTACHMENT_OBJECT),
            agent,
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Vector {
                x: 0.25,
                y: 0.25,
                z: 0.25,
            },
        )
        .textured(CHECKER_TEXTURE),
        NPC_ATTACHMENT_POINT,
        InventoryKey::from(NPC_ATTACHMENT_ITEM),
        // A quarter metre above the skull point, so the box clears the head.
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.25,
        },
        NO_ROTATION,
    )
}

/// The identity rotation.
const NO_ROTATION: sl_types::lsl::Rotation = sl_types::lsl::Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// An eighth of a turn about the Z axis, as a quaternion.
const YAW_45: sl_types::lsl::Rotation = sl_types::lsl::Rotation {
    x: 0.0,
    y: 0.0,
    // sin(22.5°), cos(22.5°).
    z: 0.382_683_43,
    s: 0.923_879_5,
};

/// The shape parameters of a Second Life **sphere**: a half-circle profile
/// swept along the sphere path (`LL_PCODE_PROFILE_CIRCLE_HALF` on
/// `LL_PCODE_PATH_CIRCLE2`), full top size on both axes.
fn sphere_shape() -> sl_proto::PrimShapeParams {
    sl_proto::PrimShapeParams {
        path_curve: 0x30,
        profile_curve: 0x05,
        path_scale_x: 100,
        path_scale_y: 100,
        ..sl_proto::PrimShapeParams::default()
    }
}

/// The URL the catalogue's media prim points at: a loopback port nothing
/// listens on, so a viewer shows its media placeholder rather than a page —
/// which is what a fixture wants to assert.
pub const MEDIA_URL: &str = "http://127.0.0.1:1/catalogue";

/// [`MEDIA_URL`] parsed, or `None` if it somehow does not (it does).
fn media_url() -> Option<url::Url> {
    MEDIA_URL.parse().ok()
}

/// A one-metre cube's scale.
const fn unit() -> Vector {
    Vector {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    }
}

/// The four-coloured box: one marker colour per face, the fifth glowing and
/// the sixth half transparent, so a single prim exercises tint, glow,
/// full-bright and alpha at once.
fn faced_box(prim: PrimFixture) -> sl_proto::Object {
    let tint = |color: [u8; 4]| {
        let [red, green, blue, _alpha] = color;
        [red, green, blue]
    };
    prim.textured(CHECKER_TEXTURE)
        // Turned an eighth of a turn about Z, so two tinted faces rather than
        // one are visible from a camera looking north up the row.
        .rotated(YAW_45)
        .face(
            0,
            &FaceStyle {
                color: tint(sl_test_assets::markers::RED),
                ..FaceStyle::default()
            },
        )
        .face(
            1,
            &FaceStyle {
                color: tint(sl_test_assets::markers::GREEN),
                ..FaceStyle::default()
            },
        )
        .face(
            2,
            &FaceStyle {
                color: tint(sl_test_assets::markers::BLUE),
                ..FaceStyle::default()
            },
        )
        .face(
            3,
            &FaceStyle {
                color: tint(sl_test_assets::markers::YELLOW),
                fullbright: true,
                ..FaceStyle::default()
            },
        )
        .face(
            4,
            &FaceStyle {
                glow: 0.5,
                ..FaceStyle::default()
            },
        )
        .face(
            5,
            &FaceStyle {
                alpha: 0.5,
                ..FaceStyle::default()
            },
        )
        .build()
}

/// The object whose per-face media the `ObjectMedia` capability answers for.
fn media_object() -> ObjectKey {
    entry("media-face-box")
        .map_or_else(|| ObjectKey::from(uuid::Uuid::nil()), |entry| entry.full_id)
}

/// The catalogue's `ObjectMedia` record: media on face 0 only, pointing at a
/// URL nothing has to actually serve (the viewer shows its placeholder until
/// the page loads, which is what a fixture asserts).
fn media_state() -> ObjectMediaState {
    let mut faces = vec![None; super::DEFAULT_FACE_COUNT];
    if let Some(slot) = faces.first_mut() {
        *slot = Some(MediaEntry {
            home_url: media_url(),
            current_url: media_url(),
            auto_play: true,
            auto_scale: true,
            width_pixels: 256,
            height_pixels: 256,
            ..MediaEntry::default()
        });
    }
    ObjectMediaState {
        version: format!("x-mv:0000000001/{}", uuid::Uuid::from_u128(CATALOGUE_OWNER)),
        faces,
    }
}

/// The catalogue's legacy material: a normal map at one repeat, medium
/// specular.
#[must_use]
pub const fn legacy_material() -> LegacyMaterial {
    LegacyMaterial {
        normal_map: NORMAL_MAP,
        normal_offset: (0.0, 0.0),
        normal_repeat: (1.0, 1.0),
        normal_rotation: 0.0,
        specular_map: texture_key(0),
        specular_offset: (0.0, 0.0),
        specular_repeat: (1.0, 1.0),
        specular_rotation: 0.0,
        specular_color: [255; 4],
        specular_exponent: 128,
        environment_intensity: 64,
        // No alpha blending: the material is about the normal map.
        diffuse_alpha_mode: 0,
        alpha_mask_cutoff: 0,
    }
}

/// A modest upward particle fountain: enough particles, slow enough, that a
/// capture a second apart differs.
const fn fountain() -> ParticleSystem {
    ParticleSystem {
        crc: 1,
        flags: 0,
        pattern: particle_pattern::ANGLE_CONE,
        max_age: 0.0,
        start_age: 0.0,
        inner_angle: 0.0,
        outer_angle: 0.4,
        burst_rate: 0.1,
        burst_radius: 0.1,
        burst_speed_min: 1.0,
        burst_speed_max: 2.0,
        burst_part_count: 8,
        angular_velocity: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        acceleration: Vector {
            x: 0.0,
            y: 0.0,
            z: -1.0,
        },
        texture_id: Some(PARTICLE_TEXTURE),
        target_id: None,
        part_flags: 0,
        part_max_age: 3.0,
        part_start_color: [255, 255, 255, 255],
        part_end_color: [255, 255, 255, 0],
        part_start_scale: [0.2, 0.2],
        part_end_scale: [0.05, 0.05],
        part_start_glow: 0.0,
        part_end_glow: 0.0,
        part_blend_func_source: 0,
        part_blend_func_dest: 0,
    }
}

/// The side, in pixels, of the catalogue's picture textures — the size a real
/// Second Life diffuse texture is.
///
/// It has to be this big to look right: a 64² fixture texture is sharp in a
/// decode test and reads as a stuck low-LOD blur in the viewer, because a one
/// metre prim face at conversational range covers several hundred screen
/// pixels and the LOD driver has nothing finer to fetch. The encoded cost of
/// the honest size is about 13 kB (a solid is ~300 bytes at any size).
const TEXTURE_SIZE: u32 = 512;

/// The side, in pixels, of the catalogue's sculpt map. A sculpt map is
/// *geometry* — one vertex per texel — and the reference viewer reads at most
/// a 64² grid, so this deliberately does not follow [`TEXTURE_SIZE`].
const SCULPT_MAP_SIZE: u32 = 64;

/// The side, in pixels, of the catalogue's flat (single-colour) textures. A
/// solid carries no detail, so it needs no more than a small tile.
const SOLID_TEXTURE_SIZE: u32 = 128;

/// The catalogue's binary assets: the checker every textured prim wears, the
/// sculpt map, the mesh, the PBR material, the particle texture, the normal
/// map and the NPC's animation — plus the four terrain detail solids the
/// ground shades against and the four EEP settings assets nothing in the row
/// names ([`NOON_SKY_ASSET`] and friends).
fn assets() -> InMemoryAssetSource {
    let mut assets = crate::scenario::default_assets();
    let checker = sl_test_assets::RgbaImage::checker(
        TEXTURE_SIZE,
        TEXTURE_SIZE / 4,
        sl_test_assets::markers::RED,
        sl_test_assets::markers::GREEN,
    );
    register(&mut assets, AssetKey::from(CHECKER_TEXTURE.uuid()), || {
        checker.j2c()
    });
    let sculpt = sl_test_assets::sculpt_sphere(SCULPT_MAP_SIZE);
    register(&mut assets, AssetKey::from(SCULPT_MAP.uuid()), || {
        sculpt.j2c()
    });
    let particle =
        sl_test_assets::RgbaImage::solid(SOLID_TEXTURE_SIZE, sl_test_assets::markers::YELLOW);
    register(&mut assets, AssetKey::from(PARTICLE_TEXTURE.uuid()), || {
        particle.j2c()
    });
    // A flat normal map: the neutral (0, 0, 1) tangent-space normal, so the
    // legacy material is a *material* without also being a bump pattern.
    let normal = sl_test_assets::RgbaImage::solid(SOLID_TEXTURE_SIZE, [128, 128, 255, 255]);
    register(&mut assets, AssetKey::from(NORMAL_MAP.uuid()), || {
        normal.j2c()
    });
    match sl_test_assets::mesh::unit_cube_mesh_asset() {
        Ok(bytes) => {
            let _previous = assets.insert(AssetKey::from(MESH_ASSET.uuid()), bytes);
        }
        Err(error) => tracing::warn!("encoding the catalogue mesh failed: {error}"),
    }
    match sl_test_assets::rigged::cylinder_mesh_asset() {
        Ok(bytes) => {
            let _previous = assets.insert(AssetKey::from(RIGGED_MESH_ASSET.uuid()), bytes);
        }
        Err(error) => tracing::warn!("encoding the catalogue rigged mesh failed: {error}"),
    }
    let _previous = assets.insert(
        AssetKey::from(PBR_MATERIAL),
        sl_test_assets::gltf_material_asset([1.0, 1.0, 1.0, 1.0], Some(CHECKER_TEXTURE.uuid())),
    );
    let _previous = assets.insert(
        AssetKey::from(NPC_ANIMATION),
        sl_test_assets::anim::chest_twist_animation_asset(),
    );
    for (id, bytes) in [
        (
            NOON_SKY_ASSET,
            sl_test_assets::environment::noon_sky_asset(),
        ),
        (
            NIGHT_SKY_ASSET,
            sl_test_assets::environment::night_sky_asset(),
        ),
        (
            WATER_SETTINGS_ASSET,
            sl_test_assets::environment::water_asset(),
        ),
        (
            DAY_CYCLE_ASSET,
            sl_test_assets::environment::day_cycle_asset(),
        ),
    ] {
        let _previous = assets.insert(AssetKey::from(id), bytes);
    }
    assets
}

/// Registers an encoded texture, logging (and skipping) an encoder failure —
/// a fixture with a missing texture is a visible failure, a panicking fixture
/// is a broken test run.
fn register<E: std::fmt::Display>(
    assets: &mut InMemoryAssetSource,
    key: AssetKey,
    encode: impl FnOnce() -> Result<Vec<u8>, E>,
) {
    match encode() {
        Ok(bytes) => {
            let _previous = assets.insert(key, bytes);
        }
        Err(error) => tracing::warn!("encoding catalogue asset {key} failed: {error}"),
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{decode_extra_params, decode_particle_system, decode_texture_entry};
    use sl_types::key::SculptOrMeshKey;

    use super::*;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// Every name in the catalogue resolves, no two prims share an id, and
    /// every prim stands in its own slot of the row.
    #[expect(
        clippy::float_cmp,
        reason = "the row positions are the same sums of exactly-representable \
                  constants the code computes, so exact equality is the test"
    )]
    #[test]
    fn the_catalogue_names_are_unique_and_ordered() {
        let entries = entries();
        assert_eq!(entries.len(), NAMES.len());
        let mut ids: Vec<u32> = entries.iter().map(|entry| entry.local_id.0).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate catalogue local ids");
        for (index, entry) in entries.iter().enumerate() {
            let expected = ROW_FIRST_X + f32::from(u16::try_from(index).unwrap_or(0)) * ROW_SPACING;
            assert_eq!(entry.position().x, expected, "{} is out of row", entry.name);
        }
        assert_eq!(
            entry("mesh-cube").map(|found| found.name),
            Some("mesh-cube")
        );
        assert_eq!(entry("no-such-prim"), None);
    }

    /// The row stands half a metre above the stock ground, which is what
    /// keeps the prims resting on it rather than sunk into or floating over
    /// it. `ROW_Z` cannot say so in `const` arithmetic, so it says so here.
    #[expect(
        clippy::float_cmp,
        reason = "both sides are exactly-representable constants, so exact \
                  equality is the test"
    )]
    #[test]
    fn the_row_rests_on_the_stock_ground() {
        assert_eq!(
            ROW_Z,
            f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M) + 0.5
        );
        assert_eq!(
            catalogue().terrain.height_at(ROW_FIRST_X, ROW_Y),
            ROW_Z - 0.5
        );
    }

    /// Every asset the catalogue's objects reference is in its asset store —
    /// no prim in the catalogue points at a 404.
    #[test]
    fn every_referenced_asset_is_served() {
        let fixture = catalogue();
        for key in [
            AssetKey::from(CHECKER_TEXTURE.uuid()),
            AssetKey::from(SCULPT_MAP.uuid()),
            AssetKey::from(MESH_ASSET.uuid()),
            AssetKey::from(PARTICLE_TEXTURE.uuid()),
            AssetKey::from(NORMAL_MAP.uuid()),
            AssetKey::from(PBR_MATERIAL),
            AssetKey::from(NPC_ANIMATION),
        ] {
            assert!(fixture.assets.contains(key), "no asset for {key}");
        }
        // The ground's detail textures come along too.
        for id in sl_proto::DEFAULT_TERRAIN_DETAIL_TEXTURES {
            assert!(fixture.assets.contains(AssetKey::from(id)));
        }
    }

    /// The NPC's bakes are served, but only once the fixture has become a
    /// scenario — the appearance names the ids and `into_scenario` is what
    /// registers the bytes behind them.
    #[test]
    fn the_npc_bakes_reach_the_asset_store() {
        let fixture = catalogue();
        let bakes: Vec<AssetKey> = npc()
            .appearance
            .bakes
            .iter()
            .map(|bake| AssetKey::from(bake.texture.uuid()))
            .collect();
        assert_eq!(bakes.len(), 3, "one bake per body region");
        for key in &bakes {
            assert!(
                !fixture.assets.contains(*key),
                "the raw fixture should not carry the bake {key} yet"
            );
        }
        let scenario = fixture.into_scenario();
        for key in &bakes {
            assert!(scenario.assets.contains(*key), "no bake served for {key}");
        }
    }

    /// The animation the NPC plays is the catalogue's own asset: the id names
    /// no built-in Linden animation, so a viewer has to fetch it, and what it
    /// fetches decodes as a keyframe motion that moves a joint.
    #[test]
    fn the_npc_animation_is_a_fixture_asset() -> Result<(), Box<dyn core::error::Error>> {
        assert!(
            sl_anim::builtin_animation(NPC_ANIMATION).is_none(),
            "the catalogue's animation id collides with a built-in"
        );
        let fixture = catalogue();
        let bytes = sl_proto::AssetSource::get(&fixture.assets, AssetKey::from(NPC_ANIMATION))
            .ok_or("the catalogue serves no animation")?;
        let motion = sl_anim::Motion::from_bytes(bytes)?;
        let joint = motion.joints.first().ok_or("the motion animates nothing")?;
        assert!(
            joint.rotation_keys.len() >= 2,
            "one keyframe cannot move anything"
        );
        Ok(())
    }

    /// The catalogue serves one EEP settings asset of every kind, each under
    /// its own id, and each decodes through the viewer's own settings-asset
    /// decoder into the kind its name promises.
    #[test]
    fn the_environment_assets_are_served_and_decode() -> Result<(), TestError> {
        let fixture = catalogue();
        let decode = |id: uuid::Uuid| {
            sl_proto::AssetSource::get(&fixture.assets, AssetKey::from(id))
                .and_then(|bytes| sl_proto::environment_asset_from_bytes(&id.to_string(), bytes))
                .ok_or("no settings asset served")
        };
        assert!(matches!(
            decode(NOON_SKY_ASSET)?,
            sl_proto::EnvironmentAsset::Sky(_)
        ));
        assert!(matches!(
            decode(WATER_SETTINGS_ASSET)?,
            sl_proto::EnvironmentAsset::Water(_)
        ));
        let sl_proto::EnvironmentAsset::DayCycle(cycle) = decode(DAY_CYCLE_ASSET)? else {
            return Err("the day-cycle asset is not a day cycle".into());
        };
        // The cycle carries both skies, so a viewer moving through it sees the
        // dark end and the bright one.
        assert_eq!(cycle.sky_frames.len(), 2);

        // The two skies are the pair a luminance oracle compares.
        let brightness = |id: uuid::Uuid| match decode(id) {
            Ok(sl_proto::EnvironmentAsset::Sky(sky)) => Ok(sky.sunlight_color.red()),
            _other => Err("not a sky asset"),
        };
        assert!(brightness(NOON_SKY_ASSET)? > brightness(NIGHT_SKY_ASSET)?);
        Ok(())
    }

    /// The catalogue's NPC is on the region's world, stands clear of the prim
    /// row, and carries the body, the appearance and the attachment a viewer
    /// needs to draw another avatar.
    #[expect(
        clippy::float_cmp,
        reason = "the row positions are the same sums of exactly-representable \
                  constants the code computes, so exact equality is the test"
    )]
    #[test]
    fn the_catalogue_npc_stands_west_of_the_row() {
        let fixture = catalogue();
        let npc = fixture.world.npcs.first().cloned().unwrap_or_else(npc);
        assert_eq!(npc.agent_id(), AgentKey::from(NPC_AGENT));
        assert_eq!(npc.position.x, ROW_FIRST_X - ROW_SPACING);
        assert_eq!(npc.position.y, ROW_Y);
        // Its ids are its own: no prim, linkset child or NPC part collides.
        let mut ids: Vec<u32> = fixture
            .world
            .all_objects()
            .iter()
            .map(|object| object.local_id.0)
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate region-local ids");
        assert!(ids.contains(&NPC_LOCAL_ID.0));
        assert!(ids.contains(&NPC_ATTACHMENT_LOCAL_ID.0));

        let record = npc.appearance_record();
        assert_eq!(record.avatar_id, AgentKey::from(NPC_AGENT));
        assert_eq!(
            record.attachments.first().map(|worn| worn.attachment_point),
            Some(NPC_ATTACHMENT_POINT)
        );
        assert_eq!(
            npc.playing_animations()
                .first()
                .map(|animation| animation.anim_id),
            Some(NPC_ANIMATION)
        );
    }

    /// The prims carry what their names claim, read back out of the raw wire
    /// blobs rather than the typed views beside them.
    #[test]
    fn each_prim_carries_the_feature_it_is_named_for() {
        let fixture = catalogue();
        let find = |name: &str| {
            let wanted = entry(name).map(|found| found.local_id);
            fixture
                .world
                .objects
                .iter()
                .find(|object| Some(object.local_id) == wanted)
                .cloned()
        };

        let mesh = find("mesh-cube").and_then(|object| {
            decode_extra_params(&object.extra_params)
                .sculpt
                .map(|sculpt| sculpt.texture)
        });
        assert_eq!(mesh, Some(SculptOrMeshKey::Mesh(MESH_ASSET)));

        let sculpt = find("sculpt-sphere").and_then(|object| {
            decode_extra_params(&object.extra_params)
                .sculpt
                .map(|sculpt| (sculpt.texture, sculpt.sculpt_type))
        });
        assert_eq!(
            sculpt,
            Some((
                SculptOrMeshKey::Sculpt(SCULPT_MAP),
                SculptKind::Sphere.code()
            ))
        );

        let light =
            find("light-box").and_then(|object| decode_extra_params(&object.extra_params).light);
        assert_eq!(light.map(|light| light.radius), Some(10.0));

        let flexi = find("flexi-blade")
            .and_then(|object| decode_extra_params(&object.extra_params).flexible);
        assert_eq!(flexi.map(|flexi| flexi.softness), Some(2));

        let probe = find("reflection-probe")
            .and_then(|object| decode_extra_params(&object.extra_params).reflection_probe);
        assert_eq!(probe.map(|probe| probe.ambiance), Some(0.5));

        let pbr =
            find("pbr-box").map(|object| decode_extra_params(&object.extra_params).render_material);
        assert_eq!(
            pbr,
            Some(vec![sl_proto::RenderMaterialRef {
                face: 0,
                material_id: PBR_MATERIAL
            }])
        );

        let particles = find("particle-emitter")
            .and_then(|object| decode_particle_system(&object.particle_system));
        assert_eq!(
            particles.and_then(|system| system.texture_id),
            Some(PARTICLE_TEXTURE)
        );

        let anim = find("texture-anim-box").map(|object| object.texture_anim.len());
        assert_eq!(anim, Some(16));

        let text = find("hover-text-box").map(|object| object.text.clone());
        assert_eq!(text.as_deref(), Some("catalogue"));

        let material = find("legacy-material-box")
            .and_then(|object| {
                decode_texture_entry(&object.texture_entry, 6)
                    .face(0)
                    .copied()
            })
            .and_then(|face| face.material_id);
        assert_eq!(material, Some(LEGACY_MATERIAL));

        let media = find("media-face-box")
            .and_then(|object| {
                decode_texture_entry(&object.texture_entry, 6)
                    .face(0)
                    .copied()
            })
            .map(sl_proto::TextureFace::media_enabled);
        assert_eq!(media, Some(true));
    }

    /// Both rigged prims name the rigged asset, only the animesh one carries
    /// the animated-object flag, and the region has an `ObjectAnimation` for
    /// exactly that one — the three facts that make the pair a *pair* rather
    /// than two copies of the same prim.
    #[test]
    fn the_animesh_prim_is_the_rigged_one_plus_a_flag() {
        let fixture = catalogue();
        let find = |name: &str| -> Option<sl_proto::Object> {
            let wanted = entry(name).map(|found| found.local_id);
            fixture
                .world
                .objects
                .iter()
                .find(|object| Some(object.local_id) == wanted)
                .cloned()
        };
        let shaped_by = |name: &str| -> Option<SculptOrMeshKey> {
            find(name).and_then(|object| {
                decode_extra_params(&object.extra_params)
                    .sculpt
                    .map(|sculpt| sculpt.texture)
            })
        };
        let animated = |name: &str| -> bool {
            find(name).is_some_and(|object| {
                decode_extra_params(&object.extra_params)
                    .extended_mesh
                    .is_some_and(|mesh| mesh.flags & 0x1 != 0)
            })
        };

        // One asset, two prims.
        let rigged = Some(SculptOrMeshKey::Mesh(RIGGED_MESH_ASSET));
        assert_eq!(shaped_by("rigged-mesh"), rigged);
        assert_eq!(shaped_by("animesh-cylinder"), rigged);
        // The flag is the only difference.
        assert!(!animated("rigged-mesh"));
        assert!(animated("animesh-cylinder"));

        // And the animation is pushed for the animesh prim alone, numbered
        // from one the way a simulator numbers a fresh set.
        let animesh = entry("animesh-cylinder").map(|found| found.full_id);
        let animations = &fixture.world.object_animations;
        assert_eq!(animations.len(), 1);
        let played = animations.first().map(|record| record.object);
        assert_eq!(played, animesh);
        assert_eq!(
            animations.first().map(ObjectAnimationFixture::wire),
            Some(vec![sl_proto::ObjectPlayingAnimation {
                anim_id: AnimationKey::from(NPC_ANIMATION),
                sequence_id: 1,
            }])
        );
    }

    /// The rigged mesh asset the two prims name is in the region's store, and
    /// decodes back into a skinned cylinder — a prim naming an asset nothing
    /// serves renders as nothing at all.
    #[test]
    fn the_rigged_mesh_asset_is_served_and_carries_its_skin() -> Result<(), TestError> {
        let fixture = catalogue();
        let bytes =
            sl_proto::AssetSource::get(&fixture.assets, AssetKey::from(RIGGED_MESH_ASSET.uuid()))
                .ok_or("the rigged mesh asset is not served")?;
        let (header, header_size) = sl_mesh::parse_header(bytes).ok_or("no mesh header")?;
        let block = header.skin.ok_or("a rigged mesh needs a skin block")?;
        let (start, end) = block.range(header_size);
        let skin = sl_mesh::decode_skin(bytes.get(start..end).ok_or("skin out of range")?)?;
        assert_eq!(
            skin.joint_names,
            sl_test_assets::rigged::RIG_JOINTS.to_vec()
        );
        Ok(())
    }

    /// The linkset's children hang off its root, and stand above it.
    #[test]
    fn the_linkset_children_hang_off_the_root() {
        let fixture = catalogue();
        let root = entry("linkset-root").map_or(RegionLocalObjectId(0), |found| found.local_id);
        let children: Vec<&sl_proto::Object> = fixture
            .world
            .objects
            .iter()
            .filter(|object| object.parent_id == root && root != RegionLocalObjectId(0))
            .collect();
        assert_eq!(children.len(), 2);
        let heights: Vec<f32> = children
            .iter()
            .map(|child| child.motion.position.z)
            .collect();
        assert_eq!(heights, vec![1.0, 2.0]);
    }

    /// The media record names the media prim and puts its entry on face 0.
    #[test]
    fn the_media_record_belongs_to_the_media_prim() {
        let fixture = catalogue();
        let (object, state) = fixture.media.first().cloned().unwrap_or_else(|| {
            (
                ObjectKey::from(uuid::Uuid::nil()),
                ObjectMediaState {
                    version: String::new(),
                    faces: Vec::new(),
                },
            )
        });
        assert_eq!(Some(object), entry("media-face-box").map(|e| e.full_id));
        assert!(state.faces.first().is_some_and(Option::is_some));
        assert!(state.faces.iter().skip(1).all(Option::is_none));
    }
}
