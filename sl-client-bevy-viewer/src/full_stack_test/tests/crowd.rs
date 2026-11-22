//! The **crowded scene**: many different objects in one frame.
//!
//! Every other subject of this tier is alone in its frame, so a draw that only
//! goes wrong beside other draws cannot fail it. A real region is the opposite:
//! hundreds of distinct translucent and rigged meshes share one sorted phase.
//! This wall puts every kind of geometry next to every other kind, each with
//! its own mesh asset, and asks of each one whether it is where it was put, in
//! the colour it was given.

use std::fmt::Write as _;

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, AssetKey, MeshKey, ObjectKey, TextureKey, Uuid};
use sl_fake_grid::fixtures::RegionFixture;
use sl_fake_grid::fixtures::prims::{FaceStyle, PrimFixture};
use sl_proto::{RegionLocalObjectId, Vector};
use sl_test_assets::{RgbaImage, markers};

use super::{ViewerHarness, entry, frame_the_subject, no_adapter, patch_at};
use crate::pixel_oracle::{Marker, coverage};
use crate::render_test::TestError;

/// How many columns and rows the wall has.
const WALL_SIDE: u16 = 6;

/// The distance between neighbouring subjects, in metres.
const WALL_SPACING: f32 = 1.25;

/// The region `y` the wall stands on: north of the arrival point and well clear
/// of the catalogue's row, which ends up behind the camera.
const WALL_Y: f32 = 200.0;

/// The region `x` of the wall's centre.
const WALL_X: f32 = 128.0;

/// The height of the wall's lowest row: a metre above the catalogue's ground.
const WALL_BASE_Z: f32 = 26.0;

/// How far south of the wall the camera stands, in metres.
const CAMERA_DISTANCE: f32 = 9.0;

/// The camera's height: at ground level, so what is behind the wall is sky.
const CAMERA_Z: f32 = 25.0;

/// The radius of the patch sampled on each subject, in pixels: well inside the
/// narrowest subject, a half-metre cylinder.
const PATCH_PX: f32 = 3.0;

/// How much of its patch a subject must paint in its own colour.
const OWN_COLOUR_SHARE: f32 = 0.6;

/// The opacity of a translucent subject: enough for its colour to dominate the
/// sky through it, and low enough to land in the blended phase.
const TRANSLUCENT: f32 = 0.7;

/// How far above its root an animesh cylinder draws, in metres. Its rig binds
/// it to the torso and chest with identity bind matrices, so at rest it stands
/// at those joints' height on the control avatar's skeleton.
const ANIMESH_RISE: f32 = 1.25;

/// The first region-local id of the wall, clear of the catalogue's ids.
const FIRST_LOCAL_ID: u32 = 0x4000;

/// The namespace of the wall's object ids.
const OBJECT_ID_BASE: u128 = 0xC80D_0000_0000;

/// The namespace of the wall's mesh assets, one per mesh subject.
const MESH_ID_BASE: u128 = 0xC80D_1000_0000;

/// The namespace of the wall's marker textures.
const TEXTURE_ID_BASE: u128 = 0xC80D_2000_0000;

/// The owner of every subject.
const OWNER: u128 = 0xC80D_3000_0000;

/// The edge of a marker texture, in texels.
const TEXTURE_SIZE: u32 = 64;

/// The three colours of the wall. Blue is left out because the sky is blue: a
/// missing subject would pass as a blue one.
const COLOURS: [(Marker, [u8; 4]); 3] = [
    (Marker::Red, markers::RED),
    (Marker::Green, markers::GREEN),
    (Marker::Yellow, markers::YELLOW),
];

/// What a subject is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A plain prim, drawn in the opaque phase.
    OpaquePrim,
    /// A prim with a translucent tint, drawn in the sorted phase.
    TranslucentPrim,
    /// A static mesh, opaque.
    OpaqueMesh,
    /// A static mesh with a translucent tint.
    TranslucentMesh,
    /// A rigged mesh standing in the world, which draws as a static one.
    RiggedInWorld,
    /// An animated object: a rigged mesh on its own skeleton.
    Animesh,
}

/// Every kind, in the order a row cycles through them.
const KINDS: [Kind; 6] = [
    Kind::OpaquePrim,
    Kind::TranslucentPrim,
    Kind::OpaqueMesh,
    Kind::TranslucentMesh,
    Kind::RiggedInWorld,
    Kind::Animesh,
];

/// One object of the wall.
#[derive(Debug, Clone)]
struct Subject {
    /// What it is made of.
    kind: Kind,
    /// The colour it must show.
    marker: Marker,
    /// Its column and row, for a failure message.
    cell: (u16, u16),
    /// Its centre, in region coordinates.
    position: Vector,
    /// Its mesh asset, if it has one.
    mesh: Option<MeshKey>,
}

/// The wall's subjects, row by row. Each row starts one kind further on, so
/// every kind has every other kind for a neighbour.
fn subjects() -> Vec<Subject> {
    let mut subjects = Vec::new();
    for row in 0..WALL_SIDE {
        for column in 0..WALL_SIDE {
            let index = usize::from(column).saturating_add(usize::from(row));
            let kind = KINDS
                .get(index.checked_rem(KINDS.len()).unwrap_or(0))
                .copied()
                .unwrap_or(Kind::OpaquePrim);
            let marker = COLOURS
                .get(index.checked_rem(COLOURS.len()).unwrap_or(0))
                .map_or(Marker::Red, |(marker, _rgba)| *marker);
            let from_centre = f32::from(column) - (f32::from(WALL_SIDE) - 1.0) / 2.0;
            let number = u128::from(row)
                .saturating_mul(u128::from(WALL_SIDE))
                .saturating_add(u128::from(column));
            let mesh = matches!(
                kind,
                Kind::OpaqueMesh | Kind::TranslucentMesh | Kind::RiggedInWorld | Kind::Animesh
            )
            .then(|| MeshKey::from(Uuid::from_u128(MESH_ID_BASE.saturating_add(number))));
            subjects.push(Subject {
                kind,
                marker,
                cell: (column, row),
                position: Vector {
                    x: WALL_X + from_centre * WALL_SPACING,
                    y: WALL_Y,
                    z: WALL_BASE_Z + f32::from(row) * WALL_SPACING,
                },
                mesh,
            });
        }
    }
    subjects
}

/// The texture painted in `marker`'s colour.
fn marker_texture(marker: Marker) -> TextureKey {
    let index = COLOURS
        .iter()
        .position(|(candidate, _rgba)| *candidate == marker)
        .unwrap_or(0);
    let offset = u128::try_from(index).unwrap_or(0);
    TextureKey::from(Uuid::from_u128(TEXTURE_ID_BASE.saturating_add(offset)))
}

/// The catalogue region with the wall added to it.
fn crowd_fixture() -> Result<RegionFixture, TestError> {
    let mut fixture = sl_fake_grid::catalogue();
    for (marker, rgba) in COLOURS {
        let bytes = RgbaImage::solid(TEXTURE_SIZE, rgba).j2c()?;
        let _previous = fixture
            .assets
            .insert(AssetKey::from(marker_texture(marker).uuid()), bytes);
    }
    let owner = AgentKey::from(Uuid::from_u128(OWNER));
    let scale = Vector {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    for (number, subject) in subjects().into_iter().enumerate() {
        let offset = u32::try_from(number)?;
        let wide = u128::from(offset);
        let local_id = RegionLocalObjectId(FIRST_LOCAL_ID.saturating_add(offset));
        let full_id = ObjectKey::from(Uuid::from_u128(OBJECT_ID_BASE + wide));
        let alpha = match subject.kind {
            Kind::TranslucentPrim | Kind::TranslucentMesh => TRANSLUCENT,
            Kind::OpaquePrim | Kind::OpaqueMesh | Kind::RiggedInWorld | Kind::Animesh => 1.0,
        };
        let style = FaceStyle {
            texture: Some(marker_texture(subject.marker)),
            alpha,
            fullbright: true,
            ..FaceStyle::default()
        };
        let mut root = subject.position.clone();
        if subject.kind == Kind::Animesh {
            root.z -= ANIMESH_RISE;
        }
        let prim = PrimFixture::boxed(local_id, full_id, owner, root, scale.clone());
        let prim = match (subject.kind, subject.mesh) {
            (Kind::OpaqueMesh | Kind::TranslucentMesh, Some(mesh)) => {
                let bytes = sl_test_assets::mesh::unit_cube_mesh_asset()?;
                let _previous = fixture.assets.insert(AssetKey::from(mesh.uuid()), bytes);
                prim.mesh(mesh, 1)
            }
            (Kind::RiggedInWorld, Some(mesh)) => {
                let bytes = sl_test_assets::rigged::cylinder_mesh_asset()?;
                let _previous = fixture.assets.insert(AssetKey::from(mesh.uuid()), bytes);
                prim.mesh(mesh, 1)
            }
            (Kind::Animesh, Some(mesh)) => {
                let bytes = sl_test_assets::rigged::cylinder_mesh_asset()?;
                let _previous = fixture.assets.insert(AssetKey::from(mesh.uuid()), bytes);
                prim.mesh(mesh, 1).animated_mesh()
            }
            _ => prim,
        };
        fixture.world.objects.push(prim.faces(&style).build());
    }
    Ok(fixture)
}

/// **Every object of a crowded frame is drawn where it stands, in its colour.**
#[test]
fn a_crowd_of_different_meshes_each_draws_in_its_own_place() -> Result<(), TestError> {
    let subjects = subjects();
    let mut harness = ViewerHarness::start(crowd_fixture()?)?;
    harness.login()?;
    let meshes: Vec<MeshKey> = subjects.iter().filter_map(|subject| subject.mesh).collect();
    harness.run_until("every mesh of the wall to decode", |harness| {
        let manager = harness.world().resource::<crate::meshes::MeshManager>();
        meshes
            .iter()
            .all(|mesh| manager.decoded(*mesh).is_some())
            .then_some(())
    })?;
    let centre_z = WALL_BASE_Z + (f32::from(WALL_SIDE) - 1.0) * WALL_SPACING / 2.0;
    harness.look_from(
        Vector {
            x: WALL_X,
            y: WALL_Y - CAMERA_DISTANCE,
            z: CAMERA_Z,
        },
        Vector {
            x: WALL_X,
            y: WALL_Y,
            z: centre_z,
        },
    );
    let Some(frame) = harness.capture()? else {
        no_adapter("the crowded scene");
        return Ok(());
    };
    let mut failures = String::new();
    for subject in &subjects {
        let patch = patch_at(&mut harness, &subject.position, PATCH_PX)?;
        let share = coverage(&frame, patch, subject.marker);
        if share < OWN_COLOUR_SHARE {
            writeln!(
                failures,
                "{:?} at column {}, row {} shows only {share:.2} of {}",
                subject.kind,
                subject.cell.0,
                subject.cell.1,
                subject.marker.name()
            )?;
        }
    }
    assert!(
        failures.is_empty(),
        "subjects missing from the crowded frame:\n{failures}"
    );
    harness.logout()
}

/// The region-local id of the wall behind the glass pane.
const GLASS_WALL_ID: u32 = 0x4100;

/// The region-local id of the glass pane.
const GLASS_PANE_ID: u32 = 0x4101;

/// The alpha of the glass texture: uniform and mid-range, so it is not a
/// one-bit mask however it is thresholded.
const GLASS_ALPHA: u8 = 150;

/// The texture of the glass pane.
const GLASS_TEXTURE: u128 = TEXTURE_ID_BASE + 0x10;

/// **A texture of uniform partial alpha is glass, not a solid sheet.**
///
/// The reference only alpha-masks a texture whose alpha histogram is two-valued
/// (`LLImageGL::analyzeAlphaData`); anything else blends. Masked at one half, a
/// 59% pane paints solid and hides what stands behind it.
#[test]
fn a_pane_of_partial_alpha_shows_what_is_behind_it() -> Result<(), TestError> {
    let mut fixture = sl_fake_grid::catalogue();
    let owner = AgentKey::from(Uuid::from_u128(OWNER));
    let glass = TextureKey::from(Uuid::from_u128(GLASS_TEXTURE));
    let [red, green, blue, _opaque] = markers::RED;
    let pane_bytes = RgbaImage::solid(TEXTURE_SIZE, [red, green, blue, GLASS_ALPHA]).j2c()?;
    let _previous = fixture
        .assets
        .insert(AssetKey::from(glass.uuid()), pane_bytes);
    let wall_bytes = RgbaImage::solid(TEXTURE_SIZE, markers::GREEN).j2c()?;
    let wall_texture = marker_texture(Marker::Green);
    let _previous = fixture
        .assets
        .insert(AssetKey::from(wall_texture.uuid()), wall_bytes);
    let at = |y: f32| Vector {
        x: WALL_X,
        y,
        z: WALL_BASE_Z + 2.0,
    };
    for (local, position, scale, texture) in [
        (
            GLASS_WALL_ID,
            at(WALL_Y + 1.0),
            Vector {
                x: 3.0,
                y: 0.2,
                z: 3.0,
            },
            wall_texture,
        ),
        (
            GLASS_PANE_ID,
            at(WALL_Y),
            Vector {
                x: 1.5,
                y: 0.05,
                z: 1.5,
            },
            glass,
        ),
    ] {
        let style = FaceStyle {
            texture: Some(texture),
            fullbright: true,
            ..FaceStyle::default()
        };
        let full_id = ObjectKey::from(Uuid::from_u128(OBJECT_ID_BASE + u128::from(local)));
        let prim = PrimFixture::boxed(RegionLocalObjectId(local), full_id, owner, position, scale);
        fixture.world.objects.push(prim.faces(&style).build());
    }
    let mut harness = ViewerHarness::start(fixture)?;
    harness.login()?;
    let pane = at(WALL_Y);
    harness.look_from(
        Vector {
            y: WALL_Y - 5.0,
            ..pane.clone()
        },
        pane.clone(),
    );
    let Some(frame) = harness.capture()? else {
        no_adapter("the glass pane");
        return Ok(());
    };
    let centre = patch_at(&mut harness, &pane, PATCH_PX)?.centre.as_uvec2();
    let pixel = frame
        .pixel(centre.x, centre.y)
        .ok_or("the pane's centre is outside the frame")?;
    assert!(
        Marker::Red.present_in(pixel) && Marker::Green.present_in(pixel),
        "the pane shows {pixel:?}: a 59% red pane over a green wall must carry both colours"
    );
    harness.logout()
}

/// The region-local id of the rigged subject of the legacy-material test.
const RIGGED_MATERIAL_ID: u32 = 0x4200;

/// The texture whose alpha channel is zero everywhere.
const ZERO_ALPHA_TEXTURE: u128 = TEXTURE_ID_BASE + 0x20;

/// The rigged mesh asset of the legacy-material test.
const RIGGED_MATERIAL_MESH: u128 = MESH_ID_BASE + 0x1000;

/// The legacy material of the legacy-material test.
const RIGGED_LEGACY_MATERIAL: u128 = OBJECT_ID_BASE + 0x9000;

/// **A rigged face honours its legacy material's alpha mode.**
///
/// A worn or animated mesh commonly carries a texture whose alpha channel means
/// nothing and a material set to "alpha mode: none". Ignoring the material
/// blends the face by that alpha, and a zero alpha draws nothing at all: the
/// shoes that are simply not there.
#[test]
fn a_rigged_face_honours_its_legacy_alpha_mode() -> Result<(), TestError> {
    let mut fixture = sl_fake_grid::catalogue();
    let owner = AgentKey::from(Uuid::from_u128(OWNER));
    let texture = TextureKey::from(Uuid::from_u128(ZERO_ALPHA_TEXTURE));
    let [red, green, blue, _opaque] = markers::RED;
    let bytes = RgbaImage::solid(TEXTURE_SIZE, [red, green, blue, 0]).j2c()?;
    let _previous = fixture.assets.insert(AssetKey::from(texture.uuid()), bytes);
    let mesh = MeshKey::from(Uuid::from_u128(RIGGED_MATERIAL_MESH));
    let _previous = fixture.assets.insert(
        AssetKey::from(mesh.uuid()),
        sl_test_assets::rigged::cylinder_mesh_asset()?,
    );
    let material = Uuid::from_u128(RIGGED_LEGACY_MATERIAL);
    // The catalogue's legacy material has diffuse alpha mode 0: none.
    fixture.materials.push((
        material,
        sl_fake_grid::fixtures::catalogue::legacy_material(),
    ));
    let seen = Vector {
        x: WALL_X,
        y: WALL_Y,
        z: WALL_BASE_Z + 2.0,
    };
    let root = Vector {
        z: seen.z - ANIMESH_RISE,
        ..seen.clone()
    };
    let style = FaceStyle {
        texture: Some(texture),
        fullbright: true,
        material: Some(material),
        ..FaceStyle::default()
    };
    let full_id = ObjectKey::from(Uuid::from_u128(
        OBJECT_ID_BASE + u128::from(RIGGED_MATERIAL_ID),
    ));
    let scale = Vector {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    let prim = PrimFixture::boxed(
        RegionLocalObjectId(RIGGED_MATERIAL_ID),
        full_id,
        owner,
        root,
        scale,
    );
    fixture
        .world
        .objects
        .push(prim.mesh(mesh, 1).animated_mesh().faces(&style).build());
    let mut harness = ViewerHarness::start(fixture)?;
    harness.login()?;
    harness.run_until("the rigged mesh to decode", |harness| {
        harness
            .world()
            .resource::<crate::meshes::MeshManager>()
            .decoded(mesh)
            .is_some()
            .then_some(())
    })?;
    harness.look_from(
        Vector {
            y: WALL_Y - 5.0,
            ..seen.clone()
        },
        seen.clone(),
    );
    let Some(frame) = harness.capture()? else {
        no_adapter("the rigged legacy material");
        return Ok(());
    };
    let patch = patch_at(&mut harness, &seen, PATCH_PX)?;
    let share = coverage(&frame, patch, Marker::Red);
    assert!(
        share > OWN_COLOUR_SHARE,
        "a rigged face with alpha mode none shows only {share:.2} of its colour"
    );
    harness.logout()
}

/// The region-local id of the rigged subject of the PBR test.
const RIGGED_PBR_ID: u32 = 0x4300;

/// The rigged mesh asset of the PBR test.
const RIGGED_PBR_MESH: u128 = MESH_ID_BASE + 0x2000;

/// The PBR material asset of the PBR test.
const RIGGED_PBR_MATERIAL: u128 = OBJECT_ID_BASE + 0xA000;

/// **A rigged face draws its PBR material.**
///
/// A PBR mesh commonly leaves a transparent placeholder as its `TextureEntry`
/// diffuse, with the real texture in the material. A rigged face that never
/// joins its material draws the placeholder: nothing.
#[test]
fn a_rigged_face_draws_its_pbr_material() -> Result<(), TestError> {
    let mut fixture = sl_fake_grid::catalogue();
    let owner = AgentKey::from(Uuid::from_u128(OWNER));
    let placeholder = TextureKey::from(Uuid::from_u128(ZERO_ALPHA_TEXTURE));
    let [red, green, blue, _opaque] = markers::RED;
    let bytes = RgbaImage::solid(TEXTURE_SIZE, [red, green, blue, 0]).j2c()?;
    let _previous = fixture
        .assets
        .insert(AssetKey::from(placeholder.uuid()), bytes);
    let painted = marker_texture(Marker::Red);
    let bytes = RgbaImage::solid(TEXTURE_SIZE, markers::RED).j2c()?;
    let _previous = fixture.assets.insert(AssetKey::from(painted.uuid()), bytes);
    let mesh = MeshKey::from(Uuid::from_u128(RIGGED_PBR_MESH));
    let _previous = fixture.assets.insert(
        AssetKey::from(mesh.uuid()),
        sl_test_assets::rigged::cylinder_mesh_asset()?,
    );
    let material = Uuid::from_u128(RIGGED_PBR_MATERIAL);
    let _previous = fixture.assets.insert(
        AssetKey::from(material),
        sl_test_assets::gltf_material_asset([1.0; 4], Some(painted.uuid())),
    );
    let seen = Vector {
        x: WALL_X,
        y: WALL_Y,
        z: WALL_BASE_Z + 2.0,
    };
    let root = Vector {
        z: seen.z - ANIMESH_RISE,
        ..seen.clone()
    };
    let style = FaceStyle {
        texture: Some(placeholder),
        ..FaceStyle::default()
    };
    let full_id = ObjectKey::from(Uuid::from_u128(OBJECT_ID_BASE + u128::from(RIGGED_PBR_ID)));
    let scale = Vector {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    let prim = PrimFixture::boxed(
        RegionLocalObjectId(RIGGED_PBR_ID),
        full_id,
        owner,
        root,
        scale,
    );
    fixture.world.objects.push(
        prim.mesh(mesh, 1)
            .animated_mesh()
            .faces(&style)
            .pbr(0, material)
            .build(),
    );
    let mut harness = ViewerHarness::start(fixture)?;
    harness.login()?;
    harness.run_until("the rigged mesh to decode", |harness| {
        harness
            .world()
            .resource::<crate::meshes::MeshManager>()
            .decoded(mesh)
            .is_some()
            .then_some(())
    })?;
    harness.look_from(
        Vector {
            y: WALL_Y - 5.0,
            ..seen.clone()
        },
        seen.clone(),
    );
    let Some(frame) = harness.capture()? else {
        no_adapter("the rigged PBR material");
        return Ok(());
    };
    let patch = patch_at(&mut harness, &seen, PATCH_PX)?;
    let share = coverage(&frame, patch, Marker::Red);
    assert!(
        share > OWN_COLOUR_SHARE,
        "a rigged PBR face shows only {share:.2} of its material's colour"
    );
    harness.logout()
}

/// How much of the patch above the fountain must show its yellow sprites. The
/// sprites are small and spread out, about 0.9% of the patch when measured, and
/// the sky behind them holds no yellow at all, so a third of that is a clear
/// signal without depending on where the random launches happened to go.
const FOUNTAIN_SPRITE_SHARE: f32 = 0.003;

/// **A particle emitter puts its sprites on screen.**
///
/// The simulation having live particles says nothing about the picture: this
/// looks at the air above the catalogue's fountain, against the sky, for the
/// colour of its sprite.
#[test]
fn a_particle_fountain_is_drawn() -> Result<(), TestError> {
    let subject = entry("particle-emitter").ok_or("the catalogue has no particle emitter")?;
    let mut harness = ViewerHarness::start(sl_fake_grid::catalogue())?;
    harness.login()?;
    frame_the_subject(&mut harness, &subject);
    let _settled = harness.capture()?;
    let Some(frame) = harness.capture_after(1.5)? else {
        no_adapter("the particle fountain");
        return Ok(());
    };
    let at = subject.position();
    let above = Vector {
        z: at.z + 1.2,
        ..at
    };
    let patch = patch_at(&mut harness, &above, 40.0)?;
    let share = coverage(&frame, patch, Marker::Yellow);
    assert!(
        share > FOUNTAIN_SPRITE_SHARE,
        "only {share:.4} of the air above the fountain shows its yellow sprites"
    );
    harness.logout()
}

/// The region-local id of the alpha-blended emitter.
const ALPHA_EMITTER_ID: u32 = 0x4400;

/// The sprite of the alpha-blended emitter.
const ALPHA_SPRITE: u128 = TEXTURE_ID_BASE + 0x30;

/// **An ordinary alpha-blended, emissive emitter is drawn**, with the parameters
/// of a real one read off Second Life: source-alpha over one-minus-source-alpha,
/// which every script gets unless it asks otherwise.
#[test]
fn an_alpha_blended_emitter_is_drawn() -> Result<(), TestError> {
    let mut fixture = sl_fake_grid::catalogue();
    let owner = AgentKey::from(Uuid::from_u128(OWNER));
    let sprite = TextureKey::from(Uuid::from_u128(ALPHA_SPRITE));
    let bytes = RgbaImage::solid(TEXTURE_SIZE, markers::RED).j2c()?;
    let _previous = fixture.assets.insert(AssetKey::from(sprite.uuid()), bytes);
    let at = Vector {
        x: WALL_X,
        y: WALL_Y,
        z: WALL_BASE_Z + 2.0,
    };
    let system = sl_proto::ParticleSystem {
        crc: 12,
        flags: 2,
        pattern: 2,
        max_age: 0.0,
        start_age: 0.0,
        inner_angle: 0.0,
        outer_angle: 0.0,
        burst_rate: 0.1,
        burst_radius: 0.0,
        burst_speed_min: 0.05,
        burst_speed_max: 0.16,
        burst_part_count: 5,
        angular_velocity: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        acceleration: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.3,
        },
        texture_id: Some(sprite),
        target_id: None,
        part_flags: 275,
        part_max_age: 3.2,
        part_start_color: [255, 255, 255, 255],
        part_end_color: [255, 255, 255, 128],
        part_start_scale: [0.78, 0.78],
        part_end_scale: [0.4, 0.4],
        part_start_glow: 0.0,
        part_end_glow: 0.0,
        part_blend_func_source: 7,
        part_blend_func_dest: 9,
    };
    let full_id = ObjectKey::from(Uuid::from_u128(
        OBJECT_ID_BASE + u128::from(ALPHA_EMITTER_ID),
    ));
    let small = Vector {
        x: 0.1,
        y: 0.1,
        z: 0.1,
    };
    let prim = PrimFixture::boxed(
        RegionLocalObjectId(ALPHA_EMITTER_ID),
        full_id,
        owner,
        at.clone(),
        small,
    );
    fixture.world.objects.push(prim.particles(system).build());
    let mut harness = ViewerHarness::start(fixture)?;
    harness.login()?;
    harness.look_from(
        Vector {
            y: WALL_Y - 5.0,
            ..at.clone()
        },
        at.clone(),
    );
    let _settled = harness.capture()?;
    let Some(frame) = harness.capture_after(1.5)? else {
        no_adapter("the alpha-blended emitter");
        return Ok(());
    };
    let patch = patch_at(&mut harness, &at, 12.0)?;
    let share = coverage(&frame, patch, Marker::Red);
    assert!(
        share > OWN_COLOUR_SHARE,
        "an alpha-blended emitter paints only {share:.3} of the patch around it"
    );
    harness.logout()
}
