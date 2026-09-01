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
    AssetKey, EnvironmentSettings, FlexibleData, InMemoryAssetSource, LightData, LightImage,
    MediaEntry, ObjectMediaState, ParticleSystem, ReflectionProbe, RegionLocalObjectId,
    RegionLocalParcelId, TextureAnimation, particle_pattern, texture_anim_mode,
};
use sl_types::key::{AgentKey, Key, MeshKey, ObjectKey, OwnerKey, TextureKey};
use sl_types::lsl::Vector;
use sl_wire::{LegacyMaterial, ReflectionProbeFlags};

use super::RegionFixture;
use super::prims::{FaceStyle, PrimFixture, SculptKind, linkset};
use crate::world::{SceneFixtures, region_wide_parcel};

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
pub const NAMES: [&str; 16] = [
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
            _unknown => objects.push(prim.build()),
        }
    }
    objects
}

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

/// The side, in pixels, of the catalogue's textures.
const TEXTURE_SIZE: u32 = 64;

/// The catalogue's binary assets: the checker every textured prim wears, the
/// sculpt map, the mesh, the PBR material, the particle texture and the normal
/// map — plus the four terrain detail solids the ground shades against.
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
    let sculpt = sl_test_assets::sculpt_sphere(TEXTURE_SIZE);
    register(&mut assets, AssetKey::from(SCULPT_MAP.uuid()), || {
        sculpt.j2c()
    });
    let particle =
        sl_test_assets::RgbaImage::solid(TEXTURE_SIZE / 4, sl_test_assets::markers::YELLOW);
    register(&mut assets, AssetKey::from(PARTICLE_TEXTURE.uuid()), || {
        particle.j2c()
    });
    // A flat normal map: the neutral (0, 0, 1) tangent-space normal, so the
    // legacy material is a *material* without also being a bump pattern.
    let normal = sl_test_assets::RgbaImage::solid(TEXTURE_SIZE / 4, [128, 128, 255, 255]);
    register(&mut assets, AssetKey::from(NORMAL_MAP.uuid()), || {
        normal.j2c()
    });
    match sl_test_assets::mesh::unit_cube_mesh_asset() {
        Ok(bytes) => {
            let _previous = assets.insert(AssetKey::from(MESH_ASSET.uuid()), bytes);
        }
        Err(error) => tracing::warn!("encoding the catalogue mesh failed: {error}"),
    }
    let _previous = assets.insert(
        AssetKey::from(PBR_MATERIAL),
        sl_test_assets::gltf_material_asset([1.0, 1.0, 1.0, 1.0], Some(CHECKER_TEXTURE.uuid())),
    );
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
        ] {
            assert!(fixture.assets.contains(key), "no asset for {key}");
        }
        // The ground's detail textures come along too.
        for id in sl_proto::DEFAULT_TERRAIN_DETAIL_TEXTURES {
            assert!(fixture.assets.contains(AssetKey::from(id)));
        }
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
