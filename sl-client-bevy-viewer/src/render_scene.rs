//! The **render scene registry** (`viewer-render-test-harness`): the one list of
//! things this viewer renders, built with **no login, no region, no OAR and no
//! UUID lookup** — shared by the gallery a human looks at
//! ([`crate::render_gallery`]) and the checks a machine runs
//! ([`crate::render_test`]).
//!
//! This is [`crate::ui_element`]'s 3D counterpart, and deliberately the same
//! shape: one registry, entries that construct themselves without wiring, and
//! declared intent carried as components. Every argument that module makes
//! applies here and applies harder — a rendering bug is currently found by a
//! human logging into OpenSim, rezzing an object, flying a camera at it and
//! squinting, and the `R*` list in `roadmap/bugs/` is what that process misses.
//!
//! # Why a scene, and not an object
//!
//! The task this implements said "render one object". That is the common case
//! and not the general one, so the registry's unit is a **scene**: geometry,
//! lights and a camera together.
//!
//! The reason is that a whole class of rendering is *about the interaction
//! between things*, and has no single-object form to test. A projector light is
//! correct or not according to what it throws light **on**; a reflective surface
//! can only be checked against what it **reflects**; one prim's shadow lands on
//! another. A registry of lone objects cannot express any of that, and would
//! have to grow a second mechanism the first time somebody tried — which is
//! exactly the retrofit this file exists to avoid. Most scenes here do hold one
//! object ([`prim_box`]); the ones that cannot ([`projector_light_on_wall`],
//! [`metallic_sphere_among_prims`]) hold several, through the same entry type.
//!
//! # Why a timeline, and not a frame
//!
//! For the same reason, in the other axis. Particles, flexi prims, texture
//! animation, avatar animation and body physics are **not functions of one
//! frame** — a single-frame capture cannot tell "emits particles" from "emits
//! nothing", and cannot tell whether an animation is playing at all. So every
//! scene carries a [`Timeline`]: the times its checks are sampled at.
//!
//! A [`Timeline::STATIC`] scene is sampled once. A scene that declares more than
//! one sample is declaring *that something happens over time*, and
//! `crate::render_test` holds it to that: a multi-sample scene whose geometry is
//! identical at every sample has failed, because the thing it exists to exercise
//! did not run. That is the check that catches a dead emitter, and it needs no
//! opt-in beyond the timeline itself.
//!
//! Time is driven by [`TimeUpdateStrategy::ManualDuration`], never the wall
//! clock — see `crate::render_test`. A harness whose results depend on how fast
//! the machine ran it is a harness that flakes.
//!
//! [`TimeUpdateStrategy::ManualDuration`]: bevy::time::TimeUpdateStrategy::ManualDuration
//!
//! # The rule this registry enforces: geometry without a session
//!
//! [`crate::ui_element`]'s rule is "constructible without its wiring". The 3D
//! rule is the same statement about a different dependency: **an object must be
//! constructible without a session**. A mesh that can only be spawned by a
//! `Session` handing it an `ObjectUpdate` is a mesh that can never be tested.
//!
//! Most of this already held before the harness existed, and it is worth being
//! precise about where the line falls, because it is the reason this file is
//! mostly fixtures rather than plumbing:
//!
//! | Layer | Where it lives | Session? |
//! | --- | --- | --- |
//! | bytes → decoded asset | `sl-mesh`, `sl-sculpt`, `sl-avatar` | no — sans-I/O |
//! | decoded → geometry | `sl-prim`, `sl-tree`, `sl_client_bevy::to_bevy_*` | no — pure |
//! | geometry → entities | `crate::objects`, `crate::avatars` | **yes** |
//!
//! The fixtures below enter at the **second** row: they synthesize a decoded
//! asset (a [`PrimShapeFloat`], a [`Submesh`], a sculpt map) and run the real
//! converter over it. That is the layer under test — whether `sl-mesh` decodes
//! its own bytes correctly is `sl-mesh`'s 20 tests' job, and duplicating it here
//! would test the decoder twice and the renderer not at all.
//!
//! The one exception is [`avatar_base_part`], because [`BaseMesh`]'s fields are
//! private and `from_bytes` is the only way to build one — so it is the only
//! fixture that needs a committed asset.
//!
//! # Fixtures are procedural
//!
//! Nothing here is committed except that one `.llm`. A sculpt map is *computed*
//! ([`sculpt_sphere_map`]), a mesh is *synthesized* ([`cube_submesh`]), a prim is
//! *tessellated* from parameters. This is the task's own advice ("generate
//! procedurally where possible") and it pays twice: no binaries in review, and a
//! fixture whose intent is readable — `sculpt_sphere_map` says it is a sphere in
//! the code, where a committed `.tga` would say it in a comment nobody can check.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bytes::Bytes;
use sl_client_bevy::{
    BaseMesh, BevySkeleton, ParticleSystem, ReflectionProbe, ReflectionProbeFlags, Skeleton,
    Vector, VertexWeights,
};
use sl_client_bevy::{
    CloudMaterial, Color as SlColor, ColorAlpha, DecodedMesh, DecodedTexture, DiscardLevel,
    FlexiChain, FlexibleData, Glow, HoleType, JointOverrides, LegacyMaterial, MeshLod, MeshSkin,
    MorphWeights, PathCurve, PrimFaceId, PrimLod, PrimShapeFloat, ProfileCurve, RegionHandle,
    ResolvedParams, SkeletalDeformations, SkyMaterial, SkySettings, StarMaterial, StarParams,
    Submesh, SunDiscMaterial, SunDiscParams, TerrainLayerType, TerrainMaterial, TerrainPatch,
    TextureAnimation, TextureFace, TextureKey, TreeLod, Uuid, VolumeDeformations, WaterMaterial,
    WaterSettings, azimuth_altitude_to_rotation, grass_geometry, grass_species,
    rigged_inverse_bindposes, tessellate, tessellate_sculpt, tessellate_with_path,
    texture_anim_mode, to_bevy_base_mesh, to_bevy_grass_mesh, to_bevy_image, to_bevy_mesh,
    to_bevy_morphed_mesh, to_bevy_prim_meshes, to_bevy_rigged_mesh, to_bevy_tree_mesh,
    tree_billboard_geometry, tree_geometry, tree_species,
};
use sl_terrain::TerrainComposition;

use std::path::Path;

use crate::avatar_assets::AvatarAssetLibrary;
use crate::bump::{apply_surface_flags, generate_normal_map};
use crate::coords::sl_to_bevy_rotation;
use crate::flexi::{FLEXI_LOD, FlexiSimState, ObjectFlexi, flexi_attributes, simulate_flexi};
use crate::legacy_materials::{apply_legacy_scalars, build_linear_image};
use crate::objects::{FaceTextureDebug, PrimFaceEntity};
use crate::particles::{ObjectParticleSystem, ParticleSim, drive_particles, float_to_u8};
use crate::probes::ObjectReflectionProbe;
use crate::sky::{
    MOON_DISK_RADIUS, SCENE_LIGHT_ILLUMINANCE, SKY_DOME_RADIUS, STAR_DOME_RADIUS, SUN_DISK_RADIUS,
    build_cloud_dome_mesh, build_star_mesh, cloud_params, disc_transform,
    placeholder_image as sky_placeholder_image, resolve_sky, shadow_cascades,
};
use crate::terrain::{
    PatchKey, TerrainSurface, build_patch_mesh, placeholder_image as terrain_placeholder_image,
};
use crate::texture_anim::{ObjectTextureAnimation, drive_texture_animations};
use crate::textures::TextureManager;
use crate::water::{DEFAULT_WATER_HEIGHT, water_normal_image, water_params};

/// The environment variable naming a Linden `character/` directory — **the same
/// one the viewer itself reads** (`--viewer-assets` / `SL_VIEWER_ASSETS`), so a
/// shell already set up to run the viewer runs the gallery against the real body
/// with no extra ceremony.
const VIEWER_ASSETS_ENV: &str = "SL_VIEWER_ASSETS";

/// The mini base-body part from `sl-avatar`'s own test fixtures — the **only**
/// committed asset this registry uses, for the one decode whose output cannot be
/// synthesized ([`BaseMesh`]'s fields are private, so `from_bytes` is the sole
/// constructor).
///
/// Taken by path from the sibling crate rather than copied here. A copy would be
/// a second 641-byte binary in review that silently rots when the real one
/// changes; this way `sl-avatar` moving or reshaping its fixture breaks *this*
/// build loudly, which is the correct direction for that failure.
const MINI_BASEMESH: &[u8] = include_bytes!("../../sl-avatar/tests/fixtures/mini_basemesh.llm");

/// The mini skeleton the [`MINI_BASEMESH`]'s weights bind against, from the same
/// fixture set.
///
/// Needed, not optional: the part carries skin weights, so `to_bevy_base_mesh`
/// gives its mesh the `JOINT_INDEX` / `JOINT_WEIGHT` attributes — and Bevy then
/// specializes the **skinned** pipeline for it. An entity with those attributes
/// and no `SkinnedMesh` hands the skinned pipeline a model-only bind group, which
/// is not a wrong picture but a wgpu validation error that kills the process. See
/// [`avatar_base_part`].
const MINI_SKELETON: &str = include_str!("../../sl-avatar/tests/fixtures/mini_skeleton.xml");

/// What varies across a matrix cell — the render harness's counterpart of
/// [`ElementCx`](crate::ui_element::ElementCx).
///
/// One axis so far, and it is the one that matters: a prim / sculpt / tree is
/// tessellated **on the client** at a detail level chosen by on-screen size, so
/// every such object has four geometries rather than one, and the coarse ones
/// are precisely the ones nobody looks at. A vertex count that collapses to
/// nothing at [`PrimLod::Lowest`], a normal that stops being unit length, a UV
/// that inverts — all of that is invisible until a user walks far enough away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SceneCx {
    /// The client-tessellation detail level to build prim / sculpt geometry at.
    pub(crate) lod: PrimLod,
}

impl SceneCx {
    /// The resting cell: full detail, the way an object a metre from the camera
    /// is built.
    pub(crate) const fn new() -> Self {
        Self { lod: PrimLod::High }
    }
}

impl Default for SceneCx {
    fn default() -> Self {
        Self::new()
    }
}

/// The times, in seconds, a scene's checks are sampled at.
///
/// See the [module documentation](self): more than one sample is a *declaration*
/// that the scene changes over time, and the harness enforces it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Timeline {
    /// The sample times, ascending, in seconds from the scene spawning.
    pub(crate) samples: &'static [f32],
}

impl Timeline {
    /// A scene that does not move: checked once, at rest.
    pub(crate) const STATIC: Self = Self { samples: &[0.0] };

    /// A scene sampled at each of `samples` seconds. Declaring more than one
    /// sample asserts the scene *changes* between the first and the last.
    pub(crate) const fn at(samples: &'static [f32]) -> Self {
        Self { samples }
    }

    /// Whether this timeline declares the scene changes over time.
    pub(crate) const fn is_dynamic(self) -> bool {
        self.samples.len() > 1
    }
}

/// Who lights a scene.
///
/// The gallery lights every scene from a fixed key/fill/ambient rig, so that a
/// difference on screen is a difference in the *thing* rather than in the
/// lighting. For most scenes that is exactly right.
///
/// It is exactly wrong for the scenes whose subject **is** the lighting. A
/// projector throwing a cone onto a wall is invisible against an 8000-lux key
/// light: the wall is already white. So those scenes say so, and the gallery
/// stands its own rig down and lets the scene's lights be the only ones — which
/// is the only way to see what they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SceneLighting {
    /// The gallery's fixed key / fill / ambient rig lights it.
    Stage,
    /// The scene brings its own lights, and the stage rig would drown them.
    Own,
}

/// Where the camera stands, in Second Life **region-local** metres (Z-up) — the
/// same frame as the viewer's `--camera-position` / `--camera-look-at` debug CLI,
/// so a scene's pose reads the same way as a pose typed at the real viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SceneCamera {
    /// The camera position.
    pub(crate) position: Vec3,
    /// The point it aims at.
    pub(crate) look_at: Vec3,
}

impl SceneCamera {
    /// The default framing: back along `-Y`, a little above the origin, looking
    /// at it. `distance` is metres, chosen per scene to fit the object.
    ///
    /// Per scene, and it has to be: a Linden tree is tens of metres tall while a
    /// prim is one, so a single distance either fills the frame with bark or
    /// renders a speck. The gallery opens on this pose, so getting it wrong means
    /// a human sees the wrong thing before touching anything.
    pub(crate) const fn framing(distance: f32) -> Self {
        Self {
            position: Vec3::new(0.0, -distance, distance * 0.35),
            look_at: Vec3::ZERO,
        }
    }

    /// Framing for something that stands on the ground and **faces `+X`**, seen
    /// from the front.
    ///
    /// Second Life's avatars face `+X`, and so does anything modelled to stand
    /// beside one. [`framing_at`](Self::framing_at) looks from `-Y`, which for
    /// those is the *side*: the measured base body spans 0.39 m on X and 1.68 m
    /// on Y (a T-pose's arm span), so the default framing shows an avatar
    /// edge-on, which is the least informative view of it there is.
    pub(crate) const fn framing_front_at(distance: f32, height: f32) -> Self {
        Self {
            position: Vec3::new(distance, 0.0, height),
            look_at: Vec3::new(0.0, 0.0, height),
        }
    }

    /// Framing for something that stands **on the ground** rather than straddling
    /// the origin: look at `height` metres up, from `distance` back at that
    /// height.
    ///
    /// [`framing`](Self::framing) aims at the origin, which is right for a prim
    /// centred there and wrong for everything rooted at `z = 0`. A Linden tree is
    /// ~36 m tall with its trunk base at the origin, so aiming at the origin puts
    /// the camera on its roots and the canopy off the top of the screen — which
    /// is exactly what the first gallery run showed.
    pub(crate) const fn framing_at(distance: f32, height: f32) -> Self {
        Self {
            position: Vec3::new(0.0, -distance, height),
            look_at: Vec3::new(0.0, 0.0, height),
        }
    }
}

/// The asset collections a scene's fixture spawns into.
///
/// Bundled rather than passed one argument per collection so a fixture's signature
/// stays readable, and so adding a collection does not touch every scene.
///
/// A [`SystemParam`] rather than a hand-built struct of borrows, which is what it
/// was while there were four collections. The viewer does not render everything out
/// of `StandardMaterial`: terrain splats, the sky, the sun disc, the clouds, the
/// stars and the water each have their own `AsBindGroup` material and their own
/// `Assets` collection, and a scene for each ([`terrain_patch`], [`sky_dome`], …)
/// needs to reach it. Threading ten `ResMut`s through both the harness and the
/// gallery by hand would have put the gallery's key handler past Bevy's
/// system-parameter limit; this way each app names one parameter and the list lives
/// here.
#[derive(SystemParam)]
pub(crate) struct SceneAssets<'w> {
    /// The mesh collection the fixture's geometry is added to.
    pub(crate) meshes: ResMut<'w, Assets<Mesh>>,
    /// The material collection.
    pub(crate) materials: ResMut<'w, Assets<StandardMaterial>>,
    /// The image collection, for a fixture that needs a texture.
    pub(crate) images: ResMut<'w, Assets<Image>>,
    /// The inverse-bindpose collection a rigged fixture's `SkinnedMesh` binds
    /// against.
    pub(crate) inverse_bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    /// The terrain splat-material collection ([`terrain_patch`]).
    pub(crate) terrain_materials: ResMut<'w, Assets<TerrainMaterial>>,
    /// The atmosphere-material collection ([`sky_dome`]).
    pub(crate) sky_materials: ResMut<'w, Assets<SkyMaterial>>,
    /// The sun / moon billboard-material collection ([`sky_dome`]).
    pub(crate) sun_disc_materials: ResMut<'w, Assets<SunDiscMaterial>>,
    /// The cloud-material collection ([`cloud_dome`]).
    pub(crate) cloud_materials: ResMut<'w, Assets<CloudMaterial>>,
    /// The star-material collection ([`star_field`]).
    pub(crate) star_materials: ResMut<'w, Assets<StarMaterial>>,
    /// The water-material collection ([`water_surface`]).
    pub(crate) water_materials: ResMut<'w, Assets<WaterMaterial>>,
}

/// Everything a registered scene needs to be **driven** — the viewer's own
/// time-varying systems and the resources they read — added by the harness
/// ([`crate::render_test`]) and the gallery ([`crate::render_gallery`]) alike.
///
/// One plugin rather than two lists, and the reason is a failure both apps have
/// already had. A dynamic scene's renderable does not exist until its driver has
/// run: the particle fountain's cloud is built by [`drive_particles`], not by its
/// fixture. So a driver missing from an app is not a compile error and not a
/// failing check — it is a scene that quietly renders **nothing**, which the
/// harness reports as a valid empty world and the gallery shows as an empty screen.
/// That happened to the gallery with the one dynamic scene there was; with flexi
/// and texture animation added there are three drivers to forget instead of one,
/// and the two lists were already only kept in step by a comment.
///
/// The custom material collections are registered here for the same reason and with
/// one wrinkle: `init_asset` is **not** idempotent (it inserts a fresh, empty
/// `Assets<M>`), and the gallery separately adds each `MaterialPlugin` for the
/// render side, which registers the same collection. So each is registered only if
/// nothing has registered it already — otherwise plugin order would decide whether
/// the gallery's materials survived.
pub(crate) struct SceneRuntimePlugin;

impl Plugin for SceneRuntimePlugin {
    fn build(&self, app: &mut App) {
        init_scene_asset::<TerrainMaterial>(app);
        init_scene_asset::<SkyMaterial>(app);
        init_scene_asset::<SunDiscMaterial>(app);
        init_scene_asset::<CloudMaterial>(app);
        init_scene_asset::<StarMaterial>(app);
        init_scene_asset::<WaterMaterial>(app);
        // `TextureManager` sits at its `Default`: no capability URL, so it never
        // fetches — the state the real viewer is in before its seed caps arrive
        // (`sl-client-viewer-fetch-defer-until-cap`).
        app.init_resource::<ParticleSim>()
            .init_resource::<TextureManager>()
            .add_systems(Startup, crate::particles::setup_particles)
            .add_systems(
                Update,
                (drive_particles, simulate_flexi, drive_texture_animations),
            );
    }
}

/// Register an asset collection a scene builds into, unless something already has.
///
/// See [`SceneRuntimePlugin`]: `init_asset` overwrites rather than skips, so an
/// unconditional call would empty a collection a `MaterialPlugin` had already set
/// up — and which of the two won would depend on the order the app added them.
fn init_scene_asset<M: Asset>(app: &mut App) {
    if !app.world().contains_resource::<Assets<M>>() {
        app.init_asset::<M>();
    }
}

/// One registered scene.
///
/// **A new renderable belongs in [`SCENES`].** That is the whole obligation, and
/// it buys the scene every check that exists now and every check added later, at
/// every LOD, at every sample of its timeline — the compounding
/// [`crate::ui_element`] describes, in three dimensions.
pub(crate) struct RenderScene {
    /// The stable id a failure names and the gallery labels — kebab-case.
    pub(crate) id: &'static str,
    /// One line: what this scene is for, and what it would catch.
    pub(crate) what: &'static str,
    /// When this scene's checks are sampled. See [`Timeline`].
    pub(crate) timeline: Timeline,
    /// Who lights it. See [`SceneLighting`].
    pub(crate) lighting: SceneLighting,
    /// Where to look from.
    pub(crate) camera: SceneCamera,
    /// Build the scene under `root`, which already carries the Second Life →
    /// Bevy basis change ([`scene_root_transform`]) — so a fixture places its
    /// geometry in ordinary Second Life Z-up metres, exactly as
    /// [`crate::objects`] does for a real prim.
    pub(crate) spawn: fn(SceneCx, Entity, &mut Commands, &mut SceneAssets<'_>),
}

/// The scenes every check runs against.
///
/// Chosen for the distinct ways each **breaks**, not for coverage of Second
/// Life's content: a prim is client-tessellated from parameters, a sculpt from a
/// texture, a mesh from an uploaded blob, a rigged mesh adds a skin, the avatar
/// body adds morphs and a skeleton. The light and particle scenes are here
/// because their correctness is not a property of geometry at all.
pub(crate) const SCENES: &[RenderScene] = &[
    RenderScene {
        id: "prim-box",
        what: "the default box: the simplest client-tessellated prim, and the baseline every \
               other prim scene is a deviation from",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: prim_box,
    },
    RenderScene {
        id: "prim-textured-tiling",
        what: "a prim whose texture tiles: the R22h class — Bevy's default sampler clamps, so a \
               face whose UVs leave the unit square smears its edge texel instead of repeating",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: prim_textured_tiling,
    },
    RenderScene {
        id: "prim-hollow-cut-cylinder",
        what: "hollow + a profile cut: the two shape features that add faces the solid prim has \
               none of (inner wall, cut edges), where a tessellator drops or inverts them",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: prim_hollow_cut_cylinder,
    },
    RenderScene {
        id: "prim-twisted-torus",
        what: "a circular path with twist: the curved-path tessellation whose step count follows \
               the LOD, so it is the prim most changed by the LOD axis",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: prim_twisted_torus,
    },
    RenderScene {
        id: "sculpt-sphere",
        what: "a sculpt map stitched into geometry: the path where the texture *is* the mesh, so \
               a decode or stitch error shows as shape rather than colour",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: sculpt_sphere,
    },
    RenderScene {
        id: "mesh-cube",
        what: "an uploaded mesh asset: the `to_bevy_mesh` conversion, including the V flip \
               between Second Life's bottom-up UVs and Bevy's top-down sampling",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: mesh_cube,
    },
    RenderScene {
        id: "rigged-mesh",
        what: "a rigged mesh with a skin block: the R1 class — Bevy does not renormalize skin \
               weights, so an un-normalized vertex is dragged toward the mesh origin",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: rigged_mesh,
    },
    RenderScene {
        id: "avatar-base-part",
        what: "a decoded base-body part on a real skeleton. NOTE: the committed fixture is \
               sl-avatar's 4-vertex mini mesh, so it looks like a flat scrap, not a body — it \
               exercises the skin path, not the shape. A real body needs SL_VIEWER_ASSETS \
               (viewer-render-scene-coverage)",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing_front_at(3.4, 0.95),
        spawn: avatar_base_part,
    },
    RenderScene {
        id: "avatar-morphed-body",
        what: "the whole system body, shaped by a resolved appearance: the morph bake, the \
               deformed skeleton and the collision volumes together — the R11 / R12 / R13 / R22 \
               cluster, none of which lives in the skin path `avatar-base-part` covers. Falls \
               back to the mini fixture without SL_VIEWER_ASSETS",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing_front_at(3.4, 0.95),
        spawn: avatar_morphed_body,
    },
    RenderScene {
        id: "terrain-patch",
        what: "one composited land patch: the heightfield, its computed normals and its \
               four-way splat weights. NOTE: the detail textures are the olive placeholder a \
               real region wears until they decode — there is no grid to fetch them from, so \
               the shape is the subject",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera {
            position: Vec3::new(8.0, -24.0, 32.0),
            look_at: Vec3::new(8.0, 8.0, 21.0),
        },
        spawn: terrain_patch,
    },
    RenderScene {
        id: "terrain-patch-seam",
        what: "a 2x2 block of neighbouring patches off one continuous surface: each patch's far \
               edge is sampled from its neighbour so the meshes meet exactly, and the block's \
               own outer edge — where the neighbour is another region — keeps the flat strip \
               instead. The seam and the region edge, which the terrain unit tests do not reach",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera {
            position: Vec3::new(16.0, -30.0, 44.0),
            look_at: Vec3::new(16.0, 16.0, 21.0),
        },
        spawn: terrain_patch_seam,
    },
    RenderScene {
        id: "flexi-streamer",
        what: "a flexible prim's chain simulation: geometry that is a function of solver state \
               rather than of parameters, so a single frame cannot tell a chain that swings \
               from one that was seeded and never stepped",
        // Seeded straight, then pulled by gravity and a steady user force: by 0.4 s
        // it is bending, by 2.0 s it has swung and is settling, so the samples
        // straddle a real change rather than a warm-up.
        timeline: Timeline::at(&[0.0, 0.4, 2.0]),
        lighting: SceneLighting::Stage,
        camera: SceneCamera {
            position: Vec3::new(1.2, -4.5, -0.8),
            look_at: Vec3::new(0.0, 0.0, -1.2),
        },
        spawn: flexi_streamer,
    },
    RenderScene {
        id: "texture-anim-flipbook",
        what: "a prim paging through a 4x4 texture atlas: the first scene whose change over time \
               is in the material rather than the vertices — its UV transform is rewritten every \
               frame while its geometry never moves",
        // A frame every 1/8 s over a 16-frame grid: by 0.5 s it has paged four
        // frames, and 1.9 s is most of the way through the grid, so the samples
        // straddle several steps without wrapping back to where they started.
        timeline: Timeline::at(&[0.0, 0.5, 1.9]),
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(3.0),
        spawn: texture_anim_flipbook,
    },
    RenderScene {
        id: "bump-face",
        what: "the four surface flags — bump, shiny, glow, fullbright — one prim each. The bump \
               prim generates a normal map from its own diffuse, which is the fifth \
               sampler-setting texture path and the one the R22h check had never seen",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(7.0),
        spawn: bump_face,
    },
    RenderScene {
        id: "legacy-material-face",
        what: "the pre-PBR normal/specular materials, at both ends of the glossiness ramp: what \
               `legacy_materials` maps onto roughness and reflectance, plus its own (linear, \
               repeating) normal-map upload",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(5.0),
        spawn: legacy_material_face,
    },
    RenderScene {
        id: "sky-sunrise",
        what: "the whole sky at sunrise — dome, clouds, stars, both discs — plus a box on the ground for \
               the low sun to throw a long shadow of. Sunrise and sunset are mirror images here: \
               the reference's named presets are grid assets, so what varies is where the sun is, \
               not an authored haze palette",
        timeline: Timeline::STATIC,
        // The sky is the light. A stage rig would flatten the very shadow this
        // scene exists to show, and wash the low sun's colour out of it.
        lighting: SceneLighting::Own,
        camera: SKY_CAMERA,
        spawn: sky_sunrise,
    },
    RenderScene {
        id: "sky-midday",
        what: "the same sky with the sun 80 degrees up: the short shadow and the near-white light the \
               atmosphere yields when its path through the air is shortest",
        timeline: Timeline::STATIC,
        // The sky is the light. A stage rig would flatten the very shadow this
        // scene exists to show, and wash the low sun's colour out of it.
        lighting: SceneLighting::Own,
        camera: SKY_CAMERA,
        spawn: sky_midday,
    },
    RenderScene {
        id: "sky-sunset",
        what: "the sun low on the far side. Same camera as `sky-sunrise`, so the pair is the \
               check that the light and the shadow follow the sun's azimuth — 180 degrees apart \
               — rather than being baked",
        timeline: Timeline::STATIC,
        // The sky is the light. A stage rig would flatten the very shadow this
        // scene exists to show, and wash the low sun's colour out of it.
        lighting: SceneLighting::Own,
        camera: SKY_CAMERA,
        spawn: sky_sunset,
    },
    RenderScene {
        id: "sky-midnight",
        what: "the only time the night half of the stack runs: the sun below the horizon, the moon lit \
               and up, the stars faded in by the sky's own star_brightness, and the shadow thrown \
               by moonlight rather than sun",
        timeline: Timeline::STATIC,
        // The sky is the light. A stage rig would flatten the very shadow this
        // scene exists to show, and wash the low sun's colour out of it.
        lighting: SceneLighting::Own,
        camera: SKY_CAMERA,
        spawn: sky_midnight,
    },
    RenderScene {
        id: "water-surface",
        what: "the endless ocean and a region's own water plane at the same height: the depth \
               bias that decides which one wins is only meaningful with both of them there",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Own,
        camera: SceneCamera {
            position: WATER_CAMERA,
            look_at: Vec3::new(0.0, 40.0, 20.0),
        },
        spawn: water_surface,
    },
    RenderScene {
        id: "tree",
        what: "generated Linden tree geometry, from the species table rather than any asset. \
               NOTE: untextured — a species' bark/leaf texture is a grid asset UUID, so there is \
               nothing to fetch it from here; the shape is the subject",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing_at(62.0, 17.0),
        spawn: tree,
    },
    RenderScene {
        id: "grass",
        what: "generated Linden grass: species-table geometry whose blades are alpha-masked \
               billboards rather than solid. NOTE: untextured for the same reason as the tree — \
               without its texture the alpha mask has nothing to cut out",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing_at(3.0, 0.5),
        spawn: grass,
    },
    RenderScene {
        id: "tree-billboard",
        what: "the far-distance tree impostor — two crossed alpha quads the reference viewer \
               swaps in below the coarsest branch LOD. A different generator from `tree`, and \
               the geometry most of a region's trees are actually made of",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing_at(62.0, 17.0),
        spawn: tree_billboard,
    },
    RenderScene {
        id: "projector-light-on-wall",
        what: "a spotlight projector aimed at a wall: correctness is what the light falls *on*, \
               so this scene has no single-object form",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Own,
        camera: SceneCamera::framing(6.0),
        spawn: projector_light_on_wall,
    },
    RenderScene {
        id: "point-light-between-prims",
        what: "a point light between two prims: the local-light budget and falloff, checked by \
               the near faces being lit and the far ones not",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Own,
        camera: SceneCamera::framing(6.0),
        spawn: point_light_between_prims,
    },
    RenderScene {
        id: "metallic-sphere-among-prims",
        what: "a mirror-metallic sphere surrounded by strongly coloured prims: it reflects them \
               through the viewer's own real-time reflection probe (P33), so which colour lands \
               where is the check the SSR / mirror tasks will want",
        timeline: Timeline::STATIC,
        lighting: SceneLighting::Own,
        camera: SceneCamera::framing(6.0),
        spawn: metallic_sphere_among_prims,
    },
    RenderScene {
        id: "particles-fountain",
        what: "a live particle source: the flagship time-varying scene — one frame cannot tell \
               an emitter that works from one that emits nothing",
        // A burst every 0.1 s with a 4 s particle age: by 0.5 s the cloud is
        // established, by 2.0 s it has both emitted and moved under gravity, so
        // the samples straddle a real change rather than a warm-up.
        timeline: Timeline::at(&[0.0, 0.5, 2.0]),
        lighting: SceneLighting::Stage,
        camera: SceneCamera::framing(8.0),
        spawn: particles_fountain,
    },
];

/// The transform the scene root carries: the single Second Life → Bevy basis
/// change, applied once at the root exactly as the viewer applies it to a real
/// region's objects ([`crate::objects`]).
///
/// So a fixture below writes ordinary Second Life coordinates — Z-up, metres —
/// and geometry checks read the same numbers a protocol message would carry.
/// Doing it any other way would mean the harness tested geometry in a frame the
/// viewer never uses.
pub(crate) fn scene_root_transform() -> Transform {
    Transform::from_rotation(sl_to_bevy_rotation())
}

/// The components every scene root carries: the basis change, and a
/// [`Visibility`].
///
/// The `Visibility` is not optional and was missing at first. A `Mesh3d` inherits
/// its visibility from **every** ancestor, so a root without one breaks the
/// propagation chain for the whole scene and Bevy warns once per renderable
/// (B0004). Bundled here rather than left to each caller because there are two —
/// `crate::render_test` and `crate::render_gallery` — and the first version of
/// this had the bug in both.
pub(crate) fn scene_root() -> impl Bundle {
    (
        scene_root_transform(),
        Visibility::default(),
        Name::new("scene-root"),
    )
}

// ---------------------------------------------------------------------------
// Declared intent. The `AlignmentGroup` tier, for geometry.
// ---------------------------------------------------------------------------

/// **Declared.** The local-space size this geometry is supposed to be.
///
/// Nothing in a vertex buffer says how big the object was *meant* to be, so the
/// fixture says. This is the check that catches a tessellator that silently
/// halves a prim, or a scale applied twice — neither of which breaks any
/// universal invariant, and both of which look like a slightly different object.
///
/// Half-extents, in Second Life metres, in the object's own local space (before
/// its [`Transform`]) — the frame a prim's declared `scale` is in.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DeclaredBounds {
    /// The expected half-extents of the local-space bounding box.
    pub(crate) half_extents: Vec3,
    /// How far each half-extent may be off, in metres.
    ///
    /// Not zero, and not a rounding allowance: a client tessellator inscribes a
    /// curved profile in the declared size, so a cylinder at a coarse LOD is
    /// legitimately a few percent under its nominal radius. The tolerance is the
    /// scene's statement of how much of that it expects.
    pub(crate) tolerance: f32,
}

/// **Declared.** The geometry is symmetric about a plane through its local
/// origin.
///
/// A symmetry is a property the source *has* and the pipeline can silently
/// destroy — a mirrored vertex dropped by a weld, a normal flipped on one side,
/// a morph applied to half a body. None of that violates an invariant; the
/// object is simply lopsided, which is exactly the kind of thing that survives
/// review and ships.
///
/// Only declared where the source really is symmetric. A twisted prim is not,
/// and saying so would make the check a liar.
/// Every axis is checked, rather than one, because most symmetric geometry is
/// symmetric about **several** planes and declaring a single one silently
/// under-checks it: a cube mirrors about X, Y *and* Z, and a fixture naming only
/// X would go on passing while a bug flattened it along Z.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SymmetricAbout {
    /// The axes the geometry mirrors across (each plane's normal).
    pub(crate) axes: &'static [SymmetryAxis],
    /// Why this geometry is symmetric — so a failure, and a reader of the
    /// gallery, can weigh the claim rather than take it on faith.
    pub(crate) reason: &'static str,
}

/// The axis a [`SymmetricAbout`] mirrors across, in the object's local Second
/// Life space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymmetryAxis {
    /// Mirrored left ↔ right.
    X,
    /// Mirrored front ↔ back.
    Y,
    /// Mirrored top ↔ bottom.
    Z,
}

/// **Declared.** This geometry samples an **atlas**, so its UVs must stay inside
/// the unit square.
///
/// Note the direction, which is the opposite of the obvious one and was set by
/// the harness's first honest run. A UV outside `[0, 1]` looks like a bug and is
/// not: the viewer samples every face with a `Repeat` address mode
/// (`crate::legacy_materials`, `crate::bump`), because Second Life faces tile —
/// so a prim UV of 1.025 is a face repeating its texture slightly, exactly as
/// intended. A universal "UVs are in `[0, 1]`" rule fires on correct prims and
/// correct trees, and a check that fires on correct geometry is one that gets
/// ignored and then deleted.
///
/// But some geometry samples a *packed atlas* rather than a tiling texture — the
/// avatar's baked body regions above all — and there a UV outside the unit square
/// does not tile, it samples **a different body part**. That is a real bug with
/// no other signature, so the geometry that has an atlas says so, and only it
/// carries the rule.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct UvsInUnitSquare {
    /// Which atlas this geometry samples, and hence why leaving the unit square
    /// is wrong rather than merely unusual.
    pub(crate) reason: &'static str,
}

/// **Declared exception.** This geometry legitimately reaches far beyond a
/// region, so the universal distance rule is raised to `max_extent` for it.
///
/// The second rule this registry has found to be *backwards*, and it was found the
/// same way the first was — by writing the scenes for a path that had none. The
/// universal check rejects a vertex more than `MAX_COORDINATE` (1 km) from its
/// object origin, on the reasoning that "every scene here is a few metres across
/// and a Second Life region is 256 m, so nothing legitimate comes near this". That
/// reasoning was true of the fourteen scenes that existed and is not true of the
/// viewer: the atmosphere is drawn as a **3 km sky dome**, the clouds as a **15 km
/// one**, the stars at 2.9 km, the sun and moon 2 km out, and the endless ocean
/// spans 40 km. Every one of those is correct and every one trips the rule.
///
/// So, as with [`UvsInUnitSquare`], the rule is not deleted — deleting it would
/// lose the failure it exists for, a vertex flung somewhere absurd by a garbage
/// matrix, which is otherwise a perfectly finite number no other check objects to.
/// It is *declared*: the geometry that is genuinely sky-scale says how far it
/// reaches and why, and is still held to that bound.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WorldScaleGeometry {
    /// The furthest any vertex may sit from the object origin, in metres.
    pub(crate) max_extent: f32,
    /// Why this geometry is drawn at a scale no region has — so a reader can weigh
    /// the claim rather than take it on faith.
    pub(crate) reason: &'static str,
}

/// **Declared exception.** This face's texture may sample with a clamping
/// address mode instead of repeating.
///
/// The universal rule is that **every** texture repeats, and it is not a
/// stylistic preference — it is a bug that has been chased down more than once
/// here. Second Life samples with `GL_REPEAT` by default and the reference
/// viewer sets clamp only for the rare texture-entry clamp flag; Bevy's default
/// sampler does the opposite and clamps to the edge. A face whose UVs leave the
/// unit square — which is *normal*, see [`UvsInUnitSquare`] — then renders as a
/// flat smear of the texture's edge texel instead of the tiled image. That is
/// R22h, the "white torso" on an otherwise-correct bake, and the streaks
/// `sl-client-prim-texture-debugging` records; both cost real time to localise
/// precisely because the geometry, the UVs and the decode are all *correct*.
///
/// Four separate places in this viewer set the mode today ([`to_bevy_image`],
/// `crate::textures`, `crate::legacy_materials`, `crate::bump`) and a fifth
/// texture path that forgets would reproduce the same bug. Hence the rule is
/// universal and the exception is opt-in, carries a reason, and is greppable.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SamplerMayClamp {
    /// Why this face's texture is allowed to clamp — in practice, only the
    /// texture-entry clamp flag.
    pub(crate) reason: &'static str,
}

// ---------------------------------------------------------------------------
// Spawn helpers, shared by the fixtures below.
// ---------------------------------------------------------------------------

/// Spawn one named piece of geometry under `parent`.
///
/// Every fixture goes through here so that every renderable in every scene ends
/// up with a [`Name`] — which is what lets a check say "`prim-box/face-2` has a
/// non-unit normal" instead of naming an entity id the reader has no way to
/// resolve.
fn spawn_geometry(
    name: impl Into<String>,
    mesh: Mesh,
    material: StandardMaterial,
    transform: Transform,
    parent: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) -> Entity {
    let mesh = assets.meshes.add(mesh);
    let material = assets.materials.add(material);
    commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Name::new(name.into()),
            ChildOf(parent),
        ))
        .id()
}

/// A plain matte material, the neutral surface most scenes want: nothing about
/// the material should be what makes a geometry check pass or fail.
fn matte(colour: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: colour,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..default()
    }
}

/// The dequantized shape a default Second Life box has: a square profile swept
/// along a straight path, no cut, no hollow, no twist, no taper.
///
/// Spelled out field by field rather than derived from a `PrimShapeParams`,
/// because the fixture's *intent* is the float shape — going through the wire
/// quantization would mean a reader has to dequantize sixteen bytes in their head
/// to see what shape this is, and would put `sl-proto`'s dequantizer inside a
/// test that is not about it. Each fixture below is this, with one field moved.
const fn base_shape() -> PrimShapeFloat {
    PrimShapeFloat {
        path_curve: PathCurve::Line,
        profile_curve: ProfileCurve::Square,
        hole_type: HoleType::Same,
        path_begin: 0.0,
        path_end: 1.0,
        path_scale_x: 1.0,
        path_scale_y: 1.0,
        path_shear_x: 0.0,
        path_shear_y: 0.0,
        twist_begin: 0.0,
        twist_end: 0.0,
        radius_offset: 0.0,
        taper_x: 0.0,
        taper_y: 0.0,
        revolutions: 1.0,
        skew: 0.0,
        profile_begin: 0.0,
        profile_end: 1.0,
        hollow: 0.0,
    }
}

/// Tessellate a prim shape and spawn it as an **object entity with one child
/// per face**, named `<part>` and `<part>/face-<n>`.
///
/// Two levels, not one, and it is load-bearing in both directions.
///
/// It is how the viewer builds a prim ([`crate::objects`]): each face is its own
/// entity with its own material, because each is textured from its own
/// texture-entry slot, and they hang off one object entity that carries the
/// prim's transform. A harness that merged the faces into one mesh would be
/// checking geometry the viewer never builds.
///
/// And it is what makes a *whole-prim* check expressible. A single face of a box
/// is a flat quad — open by construction — so "does this enclose a volume" is
/// meaningless per face and meaningful per prim. The object entity is the group
/// key `crate::render_test` unions the faces under.
#[expect(
    clippy::too_many_arguments,
    reason = "the shape, its detail, its colour, its placement, its parent and the three \
              asset collections are each genuinely independent inputs; bundling them would \
              only move the list"
)]
fn spawn_prim(
    part: &str,
    shape: &PrimShapeFloat,
    lod: PrimLod,
    colour: Color,
    transform: Transform,
    parent: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) -> Entity {
    let object = commands
        .spawn((
            transform,
            // `Visibility`, not just a `Transform`: a `Mesh3d` child inherits its
            // visibility from every ancestor, so an intermediate entity without
            // one breaks the propagation chain and Bevy warns (B0004). The viewer
            // spawns its object entities with one for the same reason; a fixture
            // that skipped it would be testing a hierarchy the viewer never
            // builds.
            Visibility::default(),
            Name::new(part.to_owned()),
            ChildOf(parent),
        ))
        .id();
    let prim = tessellate(shape, lod);
    for (index, mesh) in to_bevy_prim_meshes(&prim).into_iter().enumerate() {
        spawn_geometry(
            format!("{part}/face-{index}"),
            mesh,
            matte(colour),
            // Identity: the prim's placement lives on the object entity, exactly
            // as the viewer puts it there.
            Transform::IDENTITY,
            object,
            commands,
            assets,
        );
    }
    object
}

// ---------------------------------------------------------------------------
// The fixtures.
// ---------------------------------------------------------------------------

/// [`SCENES`] `prim-box`: the default box.
fn prim_box(cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    let object = spawn_prim(
        "prim-box",
        &base_shape(),
        cx.lod,
        Color::srgb(0.8, 0.75, 0.7),
        Transform::IDENTITY,
        root,
        commands,
        assets,
    );
    commands.entity(object).insert(DeclaredBounds {
        // A default box is the unit cube: the tessellator inscribes the square
        // profile in ±0.5 on each axis, and the object's `scale` (1 m here) is
        // what a region would multiply it by.
        half_extents: Vec3::splat(0.5),
        // A square profile has no curve to inscribe, so unlike a cylinder there
        // is nothing for a coarse LOD to shave off: the box is the same size at
        // every detail level, and anything else is a bug.
        tolerance: 1.0e-4,
    });
}

/// A **UV reference grid**, decoded the way a fetched texture is: a checker whose
/// two colour ramps make the texture's orientation readable at a glance.
///
/// A flat or single-colour texture is useless for both scenes that use this, and
/// for opposite reasons:
///
/// - For [`mesh_cube`], the thing worth seeing is the **V flip** between Second
///   Life's bottom-up UVs and Bevy's top-down sampling (`to_bevy_mesh` does it).
///   A white cube cannot show an inverted V — the geometry is identical, the
///   shading is identical, and a flipped texture looks exactly like an unflipped
///   one. So `u` drives red and `v` drives green: an upside-down face is *green
///   at the wrong end*, which is visible instantly and needs no reference shot.
/// - For [`prim_textured_tiling`], the failure is a **clamping sampler** smearing
///   the edge texel across the face. The edge texel of a uniform image looks
///   exactly like the image, so the bug is invisible on flat colour. The grid
///   lines are what make "tiled" and "smeared" different pictures.
///
/// The blue corner block is the origin marker: it pins `(0, 0)` unambiguously, so
/// a 90° rotation is distinguishable from a flip, which the two ramps alone
/// cannot do.
fn uv_reference_texture() -> DecodedTexture {
    const SIZE: u32 = 64;
    const CHECK: u32 = 8;
    let extent = f32::from(u16::try_from(SIZE).unwrap_or(1));
    let mut pixels: Vec<u8> = Vec::new();
    for y in 0..SIZE {
        let v = f32::from(u16::try_from(y).unwrap_or(0)) / (extent - 1.0);
        for x in 0..SIZE {
            let u = f32::from(u16::try_from(x).unwrap_or(0)) / (extent - 1.0);
            // The checker, as a brightness the ramps ride on: dark cells stay
            // readable rather than becoming a second colour.
            let lit = ((x / CHECK) % 2) == ((y / CHECK) % 2);
            let gain = if lit { 1.0_f32 } else { 0.45 };
            // The origin marker: the first cell, in blue.
            let origin = x < CHECK && y < CHECK;
            let (red, green, blue) = if origin {
                (0.1_f32, 0.1_f32, 1.0_f32)
            } else {
                (u * gain, v * gain, 0.15 * gain)
            };
            for channel in [red, green, blue] {
                pixels.push(float_to_u8((channel * 255.0).round()));
            }
            pixels.push(255);
        }
    }
    DecodedTexture {
        width: SIZE,
        height: SIZE,
        components: 4,
        discard_level: DiscardLevel::FULL,
        pixels: Bytes::from(pixels),
        aux: None,
    }
}

/// [`SCENES`] `prim-textured-tiling`: a prim whose texture must repeat.
///
/// The material's `uv_transform` scales the UVs by four, so every face samples
/// well outside the unit square — which is what a Second Life face with a
/// repeats-per-face of 4 does, and what makes the sampler's address mode decide
/// the picture rather than merely decorate it.
fn prim_textured_tiling(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    // Through the real converter, which is where the address mode is set — a
    // fixture that built the `Image` by hand would be testing the fixture.
    let image = assets.images.add(to_bevy_image(&uv_reference_texture()));
    let material = assets.materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(image),
        uv_transform: Affine2::from_scale(Vec2::splat(4.0)),
        perceptual_roughness: 0.9,
        ..default()
    });
    let object = commands
        .spawn((
            Transform::IDENTITY,
            // See `spawn_prim`: an ancestor of a `Mesh3d` needs `Visibility`.
            Visibility::default(),
            Name::new("prim-textured-tiling"),
            ChildOf(root),
        ))
        .id();
    let prim = tessellate(&base_shape(), cx.lod);
    for (index, mesh) in to_bevy_prim_meshes(&prim).into_iter().enumerate() {
        let mesh = assets.meshes.add(mesh);
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            Name::new(format!("prim-textured-tiling/face-{index}")),
            ChildOf(object),
        ));
    }
    commands.entity(object).insert(DeclaredBounds {
        half_extents: Vec3::splat(0.5),
        tolerance: 1.0e-4,
    });
}

/// [`SCENES`] `prim-hollow-cut-cylinder`: a circular profile, hollowed and cut.
fn prim_hollow_cut_cylinder(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let shape = PrimShapeFloat {
        profile_curve: ProfileCurve::Circle,
        hole_type: HoleType::Circle,
        hollow: 0.5,
        // A quarter cut out of the ring, which opens it and adds the two cut-edge
        // faces a solid cylinder does not have.
        profile_begin: 0.0,
        profile_end: 0.75,
        ..base_shape()
    };
    spawn_prim(
        "prim-hollow-cut-cylinder",
        &shape,
        cx.lod,
        Color::srgb(0.7, 0.8, 0.75),
        Transform::IDENTITY,
        root,
        commands,
        assets,
    );
}

/// [`SCENES`] `prim-twisted-torus`: a circular path with twist along it.
fn prim_twisted_torus(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let shape = PrimShapeFloat {
        path_curve: PathCurve::Circle,
        profile_curve: ProfileCurve::Circle,
        // Half a revolution of twist between the path's start and end: the
        // tessellator has to interpolate the profile's rotation per path step, so
        // a coarse LOD shows it as faceting rather than as a missing feature.
        twist_begin: 0.0,
        twist_end: 0.5,
        ..base_shape()
    };
    spawn_prim(
        "prim-twisted-torus",
        &shape,
        cx.lod,
        Color::srgb(0.75, 0.7, 0.8),
        Transform::IDENTITY,
        root,
        commands,
        assets,
    );
}

/// A sculpt map of a sphere: the RGB-encoded position field a sculpted prim's
/// geometry *is*.
///
/// A sculpt map stores a surface as an image — each texel's RGB is a point on
/// the surface, scaled into `[0, 255]` about the prim's centre. So a sphere map
/// is the sphere's parameterization written into pixels, which is what this
/// computes: `u` around, `v` from pole to pole.
///
/// 64×64 because that is the smallest map that reproduces the class without
/// making the stitcher's own subdivision the dominant error — a sculpt is
/// resampled to `sl_sculpt::WORKING_SUBDIVISIONS` regardless of the map's size,
/// so a tiny map would be measuring interpolation rather than stitching.
fn sculpt_sphere_map() -> DecodedTexture {
    const SIZE: u32 = 64;
    let extent = f32::from(u16::try_from(SIZE).unwrap_or(1));
    let mut pixels: Vec<u8> = Vec::new();
    for y in 0..SIZE {
        // `v` spans pole to pole: 0 at the south pole, 1 at the north.
        let v = f32::from(u16::try_from(y).unwrap_or(0)) / (extent - 1.0);
        let polar = v * PI;
        for x in 0..SIZE {
            // `u` goes once around.
            let u = f32::from(u16::try_from(x).unwrap_or(0)) / (extent - 1.0);
            let azimuth = u * TAU;
            // A unit-diameter sphere about the origin, so the encoded position
            // spans the full [-0.5, 0.5] the byte range maps onto.
            let radius = 0.5;
            let position = Vec3::new(
                radius * polar.sin() * azimuth.cos(),
                radius * polar.sin() * azimuth.sin(),
                radius * polar.cos(),
            );
            // Encode [-0.5, 0.5] into [0, 255]: the sculpt map's own convention.
            for component in [position.x, position.y, position.z] {
                pixels.push(float_to_u8(((component + 0.5) * 255.0).round()));
            }
            // Opaque: a sculpt map's alpha carries no position data.
            pixels.push(255);
        }
    }
    DecodedTexture {
        width: SIZE,
        height: SIZE,
        components: 3,
        discard_level: DiscardLevel::FULL,
        pixels: Bytes::from(pixels),
        aux: None,
    }
}

/// [`SCENES`] `sculpt-sphere`: a sculpted prim stitched from [`sculpt_sphere_map`].
fn sculpt_sphere(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let map = sculpt_sphere_map();
    // Sculpt type 1 = the sphere stitch (`LL_SCULPT_TYPE_SPHERE`): the map's
    // top/bottom rows collapse to poles and its left/right edges join.
    let prim = tessellate_sculpt(&map, 1);
    for (index, mesh) in to_bevy_prim_meshes(&prim).into_iter().enumerate() {
        let face = spawn_geometry(
            format!("sculpt-sphere/face-{index}"),
            mesh,
            matte(Color::srgb(0.8, 0.8, 0.75)),
            Transform::IDENTITY,
            root,
            commands,
            assets,
        );
        commands.entity(face).insert(SymmetricAbout {
            // Z only: `sculpt_sphere_map` walks `u` once around and `v` pole to
            // pole, so the equator mirrors exactly — but the seam where `u`
            // wraps puts the X/Y mirror of a vertex a fraction of a step away
            // rather than on it, which is a property of the parameterization
            // and not a defect.
            axes: &[SymmetryAxis::Z],
            reason: "sculpt_sphere_map computes a sphere, whose poles mirror about the equator",
        });
    }
}

/// A synthesized uploaded-mesh cube: eight corners, twelve triangles, per-vertex
/// normals and a full-face UV.
///
/// Built as a [`Submesh`] rather than encoded to `application/vnd.ll.mesh` bytes
/// and decoded back. The layer under test is decoded → geometry (see the [module
/// documentation](self)); routing through the encoder would test `sl-mesh`'s
/// decode a second time and this crate's conversion no better.
///
/// Flat-shaded — each face gets its own four corners rather than sharing the
/// cube's eight — because a shared corner cannot carry a per-face normal, and a
/// real uploaded cube is exported the same way.
fn cube_submesh() -> Submesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Each face as (normal, the two in-plane axes): the quad is the normal offset
    // by ± each axis, which gives a consistent counter-clockwise winding when the
    // axes are ordered right-handed about the normal.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    for (normal, axis_u, axis_v) in faces {
        let base = u32::try_from(positions.len()).unwrap_or(0);
        let normal_vec = Vec3::from_array(normal);
        let u_vec = Vec3::from_array(axis_u);
        let v_vec = Vec3::from_array(axis_v);
        // The face's four corners: centre + half the normal, ± half each axis.
        for (su, sv, uv) in [
            (-1.0_f32, -1.0_f32, [0.0_f32, 0.0_f32]),
            (1.0, -1.0, [1.0, 0.0]),
            (1.0, 1.0, [1.0, 1.0]),
            (-1.0, 1.0, [0.0, 1.0]),
        ] {
            positions.push([
                (normal_vec.x + su * u_vec.x + sv * v_vec.x) * 0.5,
                (normal_vec.y + su * u_vec.y + sv * v_vec.y) * 0.5,
                (normal_vec.z + su * u_vec.z + sv * v_vec.z) * 0.5,
            ]);
            normals.push(normal);
            uvs.push(uv);
        }
        for offset in [0_u32, 1, 2, 0, 2, 3] {
            indices.push(base.saturating_add(offset));
        }
    }

    Submesh {
        positions,
        normals,
        uvs,
        indices,
        weights: None,
        normalized_scale: [1.0, 1.0, 1.0],
        no_geometry: false,
    }
}

/// [`SCENES`] `mesh-cube`: an uploaded mesh asset through the real converter.
///
/// Textured with the [`uv_reference_texture`], because this scene's whole claim
/// is about UVs: `to_bevy_mesh` flips V between Second Life's bottom-up
/// convention and Bevy's top-down sampling, and an untextured cube cannot show
/// whether it did. With the grid on it, an inverted V is a face whose green ramp
/// runs the wrong way.
fn mesh_cube(_cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    let decoded = DecodedMesh {
        lod: MeshLod::High,
        submeshes: vec![cube_submesh()],
    };
    let image = assets.images.add(to_bevy_image(&uv_reference_texture()));
    for (index, submesh) in decoded.submeshes.iter().enumerate() {
        let mesh = assets.meshes.add(to_bevy_mesh(submesh));
        let material = assets.materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(image.clone()),
            perceptual_roughness: 0.9,
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Name::new(format!("mesh-cube/face-{index}")),
            ChildOf(root),
            DeclaredBounds {
                // `cube_submesh` builds a unit cube about the origin.
                half_extents: Vec3::splat(0.5),
                // Exact: no tessellator, no curve to inscribe — the fixture writes
                // these corners literally, so anything but the declared size means
                // the conversion moved a vertex.
                tolerance: 1.0e-5,
            },
            SymmetricAbout {
                axes: &[SymmetryAxis::X, SymmetryAxis::Y, SymmetryAxis::Z],
                reason: "cube_submesh places every corner at ±0.5 on each axis, so the cube \
                         mirrors about all three planes through its centre",
            },
        ));
    }
}

/// A synthesized rigged mesh: a two-joint strip whose vertices blend between the
/// joints along its length.
///
/// The point is the **weights**, not the shape. Second Life stores each influence
/// as an independently quantized fraction, so a real rig's per-vertex weights do
/// not sum to one — and Bevy's skinning shader, unlike the reference viewer's,
/// does not renormalize, so the shortfall blends in a fraction of the zero matrix
/// and drags the vertex toward the mesh origin. That is the R1 distortion
/// (`sl-client-rigged-mesh-skinning`), and `to_bevy_rigged_mesh` is what fixes
/// it.
///
/// So the fixture is deliberately **malformed the way the wire is**: the weights
/// below sum to 0.9, not 1.0. A fixture that carried tidy weights would let a
/// regression in the renormalization pass unnoticed — the check would be looking
/// at data that never needed fixing.
pub(crate) fn rigged_strip() -> (Submesh, MeshSkin) {
    const SEGMENTS: usize = 8;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut weights: Vec<VertexWeights> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for segment in 0..=SEGMENTS {
        let along = f32::from(u16::try_from(segment).unwrap_or(0))
            / f32::from(u16::try_from(SEGMENTS).unwrap_or(1));
        // The strip runs along Z, a metre long, a quarter metre wide.
        let z = along - 0.5;
        for side in [-1.0_f32, 1.0] {
            positions.push([side * 0.125, 0.0, z]);
            normals.push([0.0, -1.0, 0.0]);
            uvs.push([(side + 1.0) * 0.5, along]);
            // Blend from joint 0 at the bottom to joint 1 at the top — and sum to
            // 0.9, exactly as a quantized wire rig does. See the doc comment.
            weights.push(VertexWeights {
                influences: vec![(0_u8, (1.0 - along) * 0.9), (1_u8, along * 0.9)],
            });
        }
        if segment < SEGMENTS {
            let base = u32::try_from(segment.saturating_mul(2)).unwrap_or(0);
            for offset in [0_u32, 1, 2, 1, 3, 2] {
                indices.push(base.saturating_add(offset));
            }
        }
    }

    let submesh = Submesh {
        positions,
        normals,
        uvs,
        indices,
        weights: Some(weights),
        normalized_scale: [1.0, 1.0, 1.0],
        no_geometry: false,
    };
    // Identity binds: the rest pose is the mesh as authored, so any vertex that
    // moves at rest moved because the skinning maths is wrong and not because the
    // bind pose said so.
    let identity = Mat4::IDENTITY.to_cols_array();
    let skin = MeshSkin {
        joint_names: vec!["mPelvis".to_owned(), "mTorso".to_owned()],
        inverse_bind_matrix: vec![identity, identity],
        bind_shape_matrix: identity,
        alt_inverse_bind_matrix: Vec::new(),
        pelvis_offset: None,
        lock_scale_if_joint_position: false,
    };
    (submesh, skin)
}

/// [`SCENES`] `rigged-mesh`: a rigged strip on a two-joint skeleton.
fn rigged_mesh(_cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    let (submesh, skin) = rigged_strip();
    // The joint entities the skin binds to, spawned in `joint_names` order — the
    // order `rigged_inverse_bindposes` builds its matrices in.
    let joints: Vec<Entity> = skin
        .joint_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let height = f32::from(u16::try_from(index).unwrap_or(0)) * 0.5;
            commands
                .spawn((
                    Transform::from_xyz(0.0, 0.0, height),
                    Name::new(format!("rigged-mesh/joint/{name}")),
                    ChildOf(root),
                ))
                .id()
        })
        .collect();
    // Through the real `rigged_inverse_bindposes`, which folds the skin's
    // bind-shape matrix into each joint's inverse bind — the conversion the
    // viewer uses, and therefore the one worth testing.
    let inverse_bindposes = assets
        .inverse_bindposes
        .add(SkinnedMeshInverseBindposes::from(rigged_inverse_bindposes(
            &skin,
        )));
    let mesh = assets.meshes.add(to_bevy_rigged_mesh(&submesh));
    let material = assets.materials.add(matte(Color::srgb(0.8, 0.7, 0.7)));
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Name::new("rigged-mesh/strip"),
        ChildOf(root),
        SkinnedMesh {
            inverse_bindposes,
            joints,
        },
        // Skinned geometry moves away from its mesh-local bounds, so Bevy's
        // frustum cull (which reads the un-skinned AABB) can drop it wrongly.
        NoFrustumCulling,
        SymmetricAbout {
            // X only: the strip runs along Z and is a flat ribbon, so it mirrors
            // left-to-right and in no other plane.
            axes: &[SymmetryAxis::X],
            reason: "rigged_strip places every vertex at ±0.125 on X, mirrored about the \
                     strip's centre line",
        },
    ));
}

/// Spawn a skeleton's joint entities under `root`, parented to each other so a
/// joint's world transform is the product of its ancestors' — exactly how the
/// viewer spawns an avatar's skeleton instance.
///
/// Returns the entities in the skeleton's own joint order, which is the order
/// [`BaseMeshSkin::joints`](sl_client_bevy::BaseMeshSkin) indexes into.
fn spawn_skeleton(
    label: &str,
    skeleton: &BevySkeleton,
    root: Entity,
    commands: &mut Commands,
) -> Vec<Entity> {
    let joints: Vec<Entity> = skeleton
        .local_transforms()
        .iter()
        .enumerate()
        .map(|(index, transform)| {
            commands
                .spawn((
                    *transform,
                    Visibility::default(),
                    Name::new(format!("{label}/joint-{index}")),
                ))
                .id()
        })
        .collect();
    for (index, parent) in skeleton.parents().iter().enumerate() {
        let Some(&joint) = joints.get(index) else {
            continue;
        };
        // A root joint hangs off the scene root, so it inherits the Second Life ->
        // Bevy basis change with everything else.
        let parent = parent
            .and_then(|parent| joints.get(parent).copied())
            .unwrap_or(root);
        commands.entity(joint).insert(ChildOf(parent));
    }
    joints
}

/// Spawn one base-body part, skinned onto `joints` **if it carries weights**.
///
/// The conditional is the whole subtlety, and both halves of it have now crashed
/// this scene in turn. Bevy specializes its render pipeline from the mesh's
/// *vertex attributes*, while the bind group comes from the *entity's*
/// components, and nothing makes the two agree:
///
/// - A part **with** weights gets `JOINT_INDEX` / `JOINT_WEIGHT` from
///   [`to_bevy_base_mesh`], so it gets the skinned pipeline — and without a
///   `SkinnedMesh` it is handed a `model_only_mesh_bind_group`. That was the
///   first crash (the mini fixture, spawned with no skeleton at all).
/// - A part **without** weights gets no such attributes and the model-only
///   pipeline — and *with* a `SkinnedMesh` it is handed a
///   `skinned_mesh_bind_group` instead. That was the second crash, and only the
///   real Linden body found it: the mini fixture is weighted, so the fallback
///   path never exercised it. Some real parts (the eyes) are not.
///
/// Either way wgpu rejects the draw and Bevy quits. `crate::render_test`'s
/// `unskinned_violations` checks **both** directions now.
fn spawn_base_part(
    name: String,
    base: &BaseMesh,
    skeleton: &BevySkeleton,
    joints: &[Entity],
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let mesh = assets.meshes.add(to_bevy_base_mesh(base));
    let material = assets.materials.add(matte(Color::srgb(0.85, 0.7, 0.6)));
    let part = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Name::new(name),
            ChildOf(root),
            UvsInUnitSquare {
                reason: "an avatar body part samples its region's baked atlas (head / upper / \
                         lower), so a UV outside the unit square samples a different body region \
                         rather than tiling",
            },
        ))
        .id();

    // Exactly the condition `to_bevy_base_mesh` adds the skin attributes on. See
    // the doc comment: attaching a `SkinnedMesh` to a part without weights is as
    // fatal as omitting one from a part with them.
    if base.weights().is_empty() {
        return;
    }
    let Some(skin) = skeleton.base_mesh_skin(base) else {
        return;
    };
    let render_joints: Vec<Entity> = skin
        .joints
        .iter()
        .filter_map(|&index| joints.get(index).copied())
        .collect();
    let inverse_bindposes = assets
        .inverse_bindposes
        .add(SkinnedMeshInverseBindposes::from(skin.inverse_bindposes));
    commands.entity(part).insert((
        SkinnedMesh {
            inverse_bindposes,
            joints: render_joints,
        },
        // Skinned geometry moves away from its mesh-local bounds, so Bevy's
        // frustum cull (which reads the un-skinned AABB) can drop it wrongly.
        NoFrustumCulling,
    ));
}

/// [`SCENES`] `avatar-base-part`: the system avatar's body on its skeleton.
///
/// **The real Linden body when `SL_VIEWER_ASSETS` is set**, and a 4-vertex
/// fixture when it is not. That split is deliberate and is the only one in this
/// registry, so it is worth defending.
///
/// The avatar is the one renderable whose input cannot be synthesized: [`BaseMesh`]'s
/// fields are private, so `from_bytes` is the only constructor, and the body's
/// real shape lives in Linden's `avatar_*.llm` — megabytes of asset that have no
/// business in this repository. So:
///
/// - **With the env var** (the same one the viewer itself reads, pointing at a
///   Firestorm `character/` dir) the scene loads the real body through the real
///   [`AvatarAssetLibrary`] — the same loader a live login uses. That is what a
///   human wants to look at, and it is where R1 / R13 / R22 actually live.
/// - **Without it** the scene falls back to `sl-avatar`'s committed mini fixture,
///   so `cargo test` still exercises the skin path — the weights, the render
///   list, the pipeline agreement — on every machine, with no assets and no
///   skipping. It looks like a flat scrap, because it is one; it is a floor, not
///   a body.
///
/// A scene that simply skipped without the assets would be a check that protects
/// nothing on CI, which is the failure this whole harness is built to avoid.
fn avatar_base_part(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    if let Some(dir) = std::env::var_os(VIEWER_ASSETS_ENV)
        && let Ok(library) = AvatarAssetLibrary::load(Path::new(&dir))
    {
        let skeleton = library.skeleton();
        let joints = spawn_skeleton("avatar-base-part", skeleton, root, commands);
        for (index, part) in library.parts().iter().enumerate() {
            spawn_base_part(
                format!("avatar-base-part/part-{index}"),
                &part.mesh,
                skeleton,
                &joints,
                root,
                commands,
                assets,
            );
        }
        return;
    }

    // The fixtures are compiled in, so a decode failure is a broken build rather
    // than a missing asset — but a fixture that panicked would take the whole
    // suite out rather than reporting one scene. On failure the scene spawns
    // nothing, and `every_scene_actually_renders` reports it.
    let (Ok(base), Ok(skeleton)) = (
        BaseMesh::from_bytes(MINI_BASEMESH),
        Skeleton::from_xml(MINI_SKELETON),
    ) else {
        return;
    };
    let skeleton = BevySkeleton::from_skeleton(&skeleton);
    let joints = spawn_skeleton("avatar-base-part", &skeleton, root, commands);
    spawn_base_part(
        "avatar-base-part/body".to_owned(),
        &base,
        &skeleton,
        &joints,
        root,
        commands,
        assets,
    );
}

/// The appearance the [`avatar_morphed_body`] scene shapes its body to.
///
/// A real `AvatarAppearance.visual_params` vector is what the *server* echoes back
/// for an avatar, and it is just bytes — no session required to make one, which is
/// the whole reason this scene can exist. Each byte is one param's weight over its
/// own `[min, max]` range.
///
/// **Not `128`, and this is the one number in this file with a bug behind it.** The
/// midpoint looks like the neutral choice and is not: an asymmetric body morph's
/// default is `0`, not its range midpoint, so a `128` vector half-applies *every*
/// one of them — the bloated body and spiked head of R12, which shipped precisely
/// because a placeholder appearance was published as all-`128`, the sim stored it,
/// and the viewer rendered back what it had sent. So the fixture varies the params
/// deliberately instead: a spread that is not the default, not the midpoint, and
/// different per param, so a morph that is silently not applied and a morph that is
/// applied to everything look different from each other and from this.
fn shaped_appearance() -> Vec<u8> {
    // The wire vector is positional — param `i` of the ordered visual-param table
    // — so the length is what decides which params are addressed. 218 is the
    // standard `AvatarAppearance` count; a short vector leaves the rest at default.
    (0..218_u16)
        .map(|index| {
            // A deterministic spread over the byte range, coprime-strided so
            // neighbouring params (which tend to be related sliders) do not all
            // land on the same weight.
            u8::try_from(index.saturating_mul(37) % 251).unwrap_or(0)
        })
        .collect()
}

/// [`SCENES`] `avatar-morphed-body`: the whole system body, shaped.
///
/// The scene [`avatar_base_part`] is not. That one puts a decoded part on a
/// skeleton and stops, which covers the skin path and nothing else — and the
/// avatar bugs that have actually cost time do not live in the skin path alone:
///
/// - **R12** — the body rendered from a *resolved appearance*, where a wrong
///   resolve bloats it. Reached here, and nowhere else in the registry, by
///   [`ResolvedParams`] and [`MorphWeights::from_resolved_static`].
/// - **R13 / R11** — a base-mesh vertex weighted onto a joint outside the render
///   list. `avatar-base-part` reaches this too, but only on whichever parts happen
///   to be loaded; the bug was in the *upper body* and it was invisible at rest
///   except in one armpit.
/// - **R22** — the multi-part body, where the parts have to agree with each other.
///
/// And the shape is not only morphs: the same resolved params drive the
/// **skeleton** ([`SkeletalDeformations`]) and the **collision volumes**
/// ([`VolumeDeformations`]), and the body is only correct if the three agree.
/// A morph that shortens the legs against a skeleton that did not is exactly the
/// class of bug a login shows as "slightly wrong" and nothing catches.
///
/// **Needs `SL_VIEWER_ASSETS`**, and falls back rather than skipping — see
/// [`avatar_base_part`], which makes the same trade for the same reason. Without
/// the Linden `character/` directory there is no `avatar_lad.xml`, so there is no
/// visual-param table, no morph targets and nothing to resolve; the fallback is the
/// unshaped mini fixture, so `cargo test` still sweeps the scene everywhere.
fn avatar_morphed_body(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let Some(dir) = std::env::var_os(VIEWER_ASSETS_ENV) else {
        avatar_base_part(_cx, root, commands, assets);
        return;
    };
    let Ok(library) = AvatarAssetLibrary::load(Path::new(&dir)) else {
        avatar_base_part(_cx, root, commands, assets);
        return;
    };

    // The resolve, exactly as `crate::avatars`' `shape_avatars` does it: one
    // `ResolvedParams` feeding the morph weights, the skeletal deformations and the
    // collision-volume displacements, so the three cannot disagree by construction.
    let resolved = ResolvedParams::from_appearance(library.params(), &shaped_appearance());
    let weights = MorphWeights::from_resolved_static(library.params(), &resolved);
    let deform = SkeletalDeformations::from_resolved(library.params(), &resolved);
    let volumes = VolumeDeformations::from_resolved_with_skeleton(
        library.params(),
        &resolved,
        library.character_skeleton(),
    );

    let skeleton = library.skeleton();
    // The **deformed** skeleton, not the rest one: the joints the shaped body's
    // weights are actually skinned against. Spawning the rest skeleton under a
    // morphed body is a real bug shape — the body is the right size and stands in
    // the wrong pose — and it is what a fixture that skipped this would test.
    let locals =
        skeleton.deformed_local_transforms_with(&deform, &volumes, &JointOverrides::default());
    let joints = spawn_deformed_skeleton("avatar-morphed-body", skeleton, &locals, root, commands);

    for (index, part) in library.parts().iter().enumerate() {
        // The morph bake: the part's rest geometry blended by the resolved weights.
        // Unmasked — the clothing-morph mask is keyed off the region's *decoded
        // bake*, which is a grid texture, so a no-grid scene resolves the body's own
        // shape and leaves the garment masking to a login.
        let morphed = weights.apply(&part.mesh);
        let mesh = assets
            .meshes
            .add(to_bevy_morphed_mesh(&part.mesh, &morphed));
        let material = assets.materials.add(matte(Color::srgb(0.85, 0.7, 0.6)));
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Name::new(format!("avatar-morphed-body/part-{index}")),
                ChildOf(root),
                UvsInUnitSquare {
                    reason: "an avatar body part samples its region's baked atlas (head / upper \
                             / lower), so a UV outside the unit square samples a different body \
                             region rather than tiling — and a morph must not move a UV at all",
                },
            ))
            .id();

        // The same pipeline-agreement rule `spawn_base_part` documents: skin
        // attributes and a `SkinnedMesh` are both present or both absent, or wgpu
        // rejects the draw and takes the process with it.
        if part.mesh.weights().is_empty() {
            continue;
        }
        let Some(skin) = skeleton.base_mesh_skin(&part.mesh) else {
            continue;
        };
        let render_joints: Vec<Entity> = skin
            .joints
            .iter()
            .filter_map(|&index| joints.get(index).copied())
            .collect();
        let inverse_bindposes = assets
            .inverse_bindposes
            .add(SkinnedMeshInverseBindposes::from(skin.inverse_bindposes));
        commands.entity(entity).insert((
            SkinnedMesh {
                inverse_bindposes,
                joints: render_joints,
            },
            NoFrustumCulling,
        ));
    }
}

/// Spawn a skeleton's joints at `locals` — the shaped-body counterpart of
/// [`spawn_skeleton`], which stands them at their rest transforms.
fn spawn_deformed_skeleton(
    label: &str,
    skeleton: &BevySkeleton,
    locals: &[Transform],
    root: Entity,
    commands: &mut Commands,
) -> Vec<Entity> {
    let joints: Vec<Entity> = locals
        .iter()
        .enumerate()
        .map(|(index, transform)| {
            commands
                .spawn((
                    *transform,
                    Visibility::default(),
                    Name::new(format!("{label}/joint-{index}")),
                ))
                .id()
        })
        .collect();
    for (index, parent) in skeleton.parents().iter().enumerate() {
        let Some(&joint) = joints.get(index) else {
            continue;
        };
        let parent = parent
            .and_then(|parent| joints.get(parent).copied())
            .unwrap_or(root);
        commands.entity(joint).insert(ChildOf(parent));
    }
    joints
}

/// [`SCENES`] `tree`: generated Linden tree geometry.
fn tree(_cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    let Some(species) = tree_species(0) else {
        return;
    };
    let mesh = to_bevy_tree_mesh(&tree_geometry(species, TreeLod::High));
    spawn_geometry(
        "tree/canopy",
        mesh,
        matte(Color::srgb(0.35, 0.5, 0.3)),
        Transform::IDENTITY,
        root,
        commands,
        assets,
    );
}

/// [`SCENES`] `grass`: generated Linden grass geometry.
fn grass(_cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    let Some(species) = grass_species(0) else {
        return;
    };
    let mesh = to_bevy_grass_mesh(&grass_geometry(species, 1.0, 1.0, 8));
    spawn_geometry(
        "grass/clump",
        mesh,
        matte(Color::srgb(0.4, 0.55, 0.3)),
        Transform::IDENTITY,
        root,
        commands,
        assets,
    );
}

/// [`SCENES`] `projector-light-on-wall`: a spotlight aimed at a wall.
fn projector_light_on_wall(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    // The wall: a box flattened along Y, standing at the origin.
    spawn_prim(
        "projector-light-on-wall/wall",
        &base_shape(),
        cx.lod,
        Color::WHITE,
        Transform::from_xyz(0.0, 2.0, 0.0).with_scale(Vec3::new(6.0, 0.1, 4.0)),
        root,
        commands,
        assets,
    );
    // The projector: in front of the wall, aimed at it. Bevy's spotlight looks
    // along its local -Z, which is what `looking_at` orients.
    commands.spawn((
        SpotLight {
            intensity: 400_000.0,
            range: 20.0,
            inner_angle: 0.2,
            outer_angle: 0.4,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, -2.0, 0.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Z),
        Name::new("projector-light-on-wall/projector"),
        ChildOf(root),
    ));
}

/// [`SCENES`] `point-light-between-prims`: a point light flanked by two prims.
fn point_light_between_prims(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    for (side, name) in [(-1.5_f32, "left"), (1.5, "right")] {
        spawn_prim(
            &format!("point-light-between-prims/{name}"),
            &base_shape(),
            cx.lod,
            Color::WHITE,
            Transform::from_xyz(side, 0.0, 0.0),
            root,
            commands,
            assets,
        );
    }
    commands.spawn((
        PointLight {
            intensity: 300_000.0,
            range: 10.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        Name::new("point-light-between-prims/light"),
        ChildOf(root),
    ));
}

/// [`SCENES`] `metallic-sphere-among-prims`: a mirror surrounded by colour.
fn metallic_sphere_among_prims(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    // The reflector, at the centre.
    // A Second Life sphere is a **half-circle profile swept around the `Circle`
    // path** — a solid of revolution. Not `Circle2`, which looks like the sphere
    // path from its name and is not: the reference (`LLPath::generate`,
    // `LL_PCODE_PATH_CIRCLE2`) runs `genNGon` and then *overrides every path
    // point's X to alternate between +0.5 and -0.5*. `sl-prim` reproduces that
    // faithfully, so a half-circle on `Circle2` sweeps a 2 x 1 x 1 pill — which is
    // what this fixture built at first, and exactly what it looked like.
    let sphere = tessellate(
        &PrimShapeFloat {
            path_curve: PathCurve::Circle,
            profile_curve: ProfileCurve::HalfCircle,
            ..base_shape()
        },
        cx.lod,
    );
    let sphere_object = commands
        .spawn((
            Transform::IDENTITY,
            Visibility::default(),
            Name::new("metallic-sphere-among-prims/sphere"),
            ChildOf(root),
            DeclaredBounds {
                // A unit sphere. Declared because the pill this fixture used to
                // build broke *no* invariant — it was finite, unit-normalled and
                // correctly indexed, just the wrong shape, and only a human
                // looking at it noticed. This is the check that notices next time.
                half_extents: Vec3::splat(0.5),
                // A circle profile is inscribed, so a coarse LOD legitimately
                // falls slightly inside the nominal radius.
                tolerance: 0.06,
            },
        ))
        .id();
    for (index, mesh) in to_bevy_prim_meshes(&sphere).into_iter().enumerate() {
        spawn_geometry(
            format!("metallic-sphere-among-prims/sphere/face-{index}"),
            mesh,
            StandardMaterial {
                base_color: Color::WHITE,
                // A mirror: nothing to see on it but what is around it, which is
                // the point of the scene.
                metallic: 1.0,
                perceptual_roughness: 0.05,
                ..default()
            },
            Transform::IDENTITY,
            sphere_object,
            commands,
            assets,
        );
    }
    // The reflection probe: a **separate, larger prim** centred on the sphere,
    // which is how this is built in-world — you rez a probe prim covering the area
    // and put the shiny thing inside it. It carries no geometry of its own; a probe
    // is a prim with an `LLReflectionProbeParams` block, not a shape.
    //
    // Separate, and larger, for a reason that is easy to get wrong: a probe's
    // influence volume is derived from its prim's scale (a sphere probe's radius is
    // `scale.x * 0.5`), so putting the block on the **sphere itself** gives a 1 m
    // ball a 0.5 m influence radius — and the ball's own surface then sits exactly
    // *on* the volume's boundary, where `SPHERE_FALLOFF` tapers the local probe out
    // and the view's default probe fills in behind it. That renders as two
    // reflections: the local one, and a faint ghost of the default probe's
    // viewpoint-captured cube, in the wrong place. The volume has to *contain* what
    // it lights.
    commands.spawn((
        Transform::IDENTITY,
        Visibility::default(),
        Name::new("metallic-sphere-among-prims/probe"),
        ChildOf(root),
        ObjectReflectionProbe {
            data: ReflectionProbe {
                ambiance: 0.0,
                clip_distance: 0.0,
                flags: ReflectionProbeFlags::MIRROR,
            },
            // A 6 m sphere volume: it contains the mirror and its neighbours with
            // room to spare, so nothing being lit sits in the falloff.
            scale: [6.0, 6.0, 6.0],
        },
    ));

    // What it has to reflect: strongly and distinctly coloured boxes on each side,
    // so a reflection check can tell *which* neighbour a pixel came from rather
    // than only that something was reflected.
    for (offset, colour, name) in [
        (Vec3::new(-2.0, 0.0, 0.0), Color::srgb(0.9, 0.1, 0.1), "red"),
        (
            Vec3::new(2.0, 0.0, 0.0),
            Color::srgb(0.1, 0.9, 0.1),
            "green",
        ),
        (Vec3::new(0.0, 2.0, 0.0), Color::srgb(0.1, 0.1, 0.9), "blue"),
        (
            Vec3::new(0.0, 0.0, -2.0),
            Color::srgb(0.9, 0.9, 0.1),
            "yellow",
        ),
    ] {
        spawn_prim(
            &format!("metallic-sphere-among-prims/{name}"),
            &base_shape(),
            cx.lod,
            colour,
            Transform::from_translation(offset),
            root,
            commands,
            assets,
        );
    }
    commands.spawn((
        PointLight {
            intensity: 500_000.0,
            range: 20.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, -3.0, 3.0),
        Name::new("metallic-sphere-among-prims/light"),
        ChildOf(root),
    ));
}

/// The particle system the fountain emits: an upward cone, gravity-pulled,
/// fading out.
///
/// Every field is written out rather than defaulted, because [`ParticleSystem`]
/// is the wire block and has no `Default` — and because in a fixture whose whole
/// purpose is "does anything come out", a silently-zero `burst_part_count` would
/// make the scene pass by emitting nothing.
const fn fountain_system() -> ParticleSystem {
    ParticleSystem {
        // Non-zero: a zero CRC is the reference viewer's "null system" stop
        // sentinel, and `particles_from_object` would reject it as not a source.
        crc: 1,
        flags: 0,
        // Pattern 4 = `LL_PART_SRC_PATTERN_ANGLE_CONE`: emit within a cone.
        pattern: 4,
        // Forever: the timeline decides how long the scene runs, not the system.
        max_age: 0.0,
        start_age: 0.0,
        inner_angle: 0.0,
        outer_angle: 0.35,
        burst_rate: 0.1,
        burst_radius: 0.0,
        burst_speed_min: 3.0,
        burst_speed_max: 4.0,
        burst_part_count: 8,
        angular_velocity: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        // Gravity, so the particles arc rather than fly straight — the sample at
        // 2.0 s differs from the one at 0.5 s in shape and not only in count.
        acceleration: Vector {
            x: 0.0,
            y: 0.0,
            z: -9.8,
        },
        // The viewer's own procedural default sprite, so the scene needs no asset.
        texture_id: None,
        target_id: None,
        part_flags: 0,
        part_max_age: 4.0,
        part_start_color: [255, 220, 160, 255],
        part_end_color: [255, 80, 40, 0],
        part_start_scale: [0.2, 0.2],
        part_end_scale: [0.05, 0.05],
        part_start_glow: 0.0,
        part_end_glow: 0.0,
        part_blend_func_source: 0,
        part_blend_func_dest: 0,
    }
}

// ---------------------------------------------------------------------------
// Terrain.
// ---------------------------------------------------------------------------

/// The edge length, in samples and in metres, of a land patch: a Second Life
/// region is 16×16 patches of 16×16 one-metre cells.
const TERRAIN_PATCH_SIZE: u32 = 16;

/// The region the terrain fixtures' patches belong to.
///
/// The local test grid's own `Default Region` corner (grid 1000, 1000 → 256 000 m),
/// rather than a nominal zero, because the composition's Perlin transition band is
/// sampled at the region's **global** origin — so at zero the fixture would be
/// looking at a corner of the noise field no real region ever sits on.
fn terrain_region() -> RegionHandle {
    RegionHandle::from_global(256_000, 256_000)
}

/// The elevation bands the terrain fixtures composite against: the four detail
/// textures spread over the height range the fixture's ground actually spans, so
/// every patch has several of them blending across it rather than one flat weight.
const fn terrain_composition() -> TerrainComposition {
    TerrainComposition::new(
        [16.0, 18.0, 16.0, 18.0],
        [12.0, 10.0, 10.0, 12.0],
        256.0,
        [256_000.0, 256_000.0],
    )
}

/// A small region cell / metre count as an `f32`; there is no `From<u32>` for
/// `f32`, and every value here is far under `u16::MAX` so the conversion is exact.
fn terrain_coord(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// The fixture's ground: a smooth hill, as a function of **region-local** metres.
///
/// One continuous function of the region coordinate rather than a per-patch shape,
/// and that is the whole point of the seam scene: adjacent patches sample the same
/// surface, so where their meshes meet is decided by [`build_patch_mesh`]'s shared
/// far edge and nothing else. A per-patch shape would meet at the seam by
/// construction and prove nothing.
fn terrain_height(x: f32, y: f32) -> f32 {
    let hill = (x / 40.0).sin() * (y / 34.0).cos();
    // A base near the default water height, so the scene reads as ground rather
    // than as an abstract surface, and a few metres of relief.
    21.0 + 5.0 * hill
}

/// One land patch at grid position (`patch_x`, `patch_y`) of [`terrain_region`],
/// sampled from [`terrain_height`].
fn land_patch(patch_x: u32, patch_y: u32) -> TerrainPatch {
    let size = TERRAIN_PATCH_SIZE;
    let origin_x = patch_x.saturating_mul(size);
    let origin_y = patch_y.saturating_mul(size);
    let mut values: Vec<f32> = Vec::new();
    for y in 0..size {
        for x in 0..size {
            values.push(terrain_height(
                terrain_coord(origin_x.saturating_add(x)),
                terrain_coord(origin_y.saturating_add(y)),
            ));
        }
    }
    TerrainPatch {
        region_handle: terrain_region(),
        layer: TerrainLayerType::Land,
        patch_x,
        patch_y,
        size,
        values,
    }
}

/// Build and spawn the land patches at `grid`, through the viewer's real
/// [`build_patch_mesh`] and its real [`TerrainMaterial`].
///
/// The patches are placed in plain Second Life metres under the scene root, which
/// is *not* what the viewer does: `crate::terrain`'s `patch_transform` carries the
/// Second Life → Bevy basis change on each patch entity, because the viewer spawns
/// its patches at the world root with no basis-changed ancestor. Here the scene
/// root already carries it (once, for every scene), so applying it again would
/// rotate the ground on its side. The region-offset half of `patch_transform` has
/// its own test; what has none, and what this builds, is the mesh.
///
/// The detail textures are the flat olive placeholder every region's terrain wears
/// until its four ground textures decode — there is no grid here to fetch them
/// from. So the splat weights are real and only their palette is not, which is why
/// the `what` line says the shape is the subject.
fn spawn_terrain(
    label: &str,
    grid: &[(u32, u32)],
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let mut patches: HashMap<PatchKey, TerrainPatch> = HashMap::new();
    for &(patch_x, patch_y) in grid {
        let _replaced = patches.insert(
            (terrain_region(), patch_x, patch_y),
            land_patch(patch_x, patch_y),
        );
    }
    let composition = terrain_composition();
    let placeholder = assets.images.add(terrain_placeholder_image());
    let material = assets.terrain_materials.add(TerrainMaterial {
        detail0: placeholder.clone(),
        detail1: placeholder.clone(),
        detail2: placeholder.clone(),
        detail3: placeholder,
    });
    // Iterated over `grid` rather than the map, so the entities spawn in a stable
    // order whatever the hash seed — a failure that named a different patch on
    // every run would be a failure nobody could act on.
    for &(patch_x, patch_y) in grid {
        let key = (terrain_region(), patch_x, patch_y);
        let Some(mesh) = build_patch_mesh(&patches, Some(&composition), key) else {
            continue;
        };
        let mesh = assets.meshes.add(mesh);
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(
                terrain_coord(patch_x.saturating_mul(TERRAIN_PATCH_SIZE)),
                terrain_coord(patch_y.saturating_mul(TERRAIN_PATCH_SIZE)),
                0.0,
            ),
            Name::new(format!("{label}/patch-{patch_x}-{patch_y}")),
            ChildOf(root),
            // As the viewer marks a land patch, so the ground probe would accept
            // it: the component costs nothing and keeps the fixture the same
            // entity the viewer builds.
            TerrainSurface,
        ));
    }
}

/// [`SCENES`] `terrain-patch`: one land patch of a composited region.
fn terrain_patch(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    spawn_terrain("terrain-patch", &[(0, 0)], root, commands, assets);
}

/// [`SCENES`] `terrain-patch-seam`: a 2×2 block of neighbouring patches.
fn terrain_patch_seam(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    spawn_terrain(
        "terrain-patch-seam",
        &[(0, 0), (1, 0), (0, 1), (1, 1)],
        root,
        commands,
        assets,
    );
}

// ---------------------------------------------------------------------------
// Flexi.
// ---------------------------------------------------------------------------

/// [`SCENES`] `flexi-streamer`: a flexible prim hanging from the origin.
///
/// **Dynamic, and the second scene that has to be.** A flexi prim's geometry is not
/// a function of its parameters — it is a function of a chain simulation's *state*,
/// which [`simulate_flexi`] steps every frame. A single capture cannot tell a chain
/// that hangs and swings from one that was seeded and never stepped, because both
/// look like a prim, and the seeded rest chain is a perfectly plausible one.
///
/// The chain is seeded at the object entity's own pose (the origin, since the
/// entity is at identity under the scene root) rather than somewhere else, because
/// [`simulate_flexi`] re-anchors it from the live world transform on the first step
/// — so seeding it elsewhere would show as a one-frame jump that means nothing.
fn flexi_streamer(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    // A round streamer: a circular profile swept along a straight path, which is
    // what a flexi prim almost always is in-world.
    let shape = PrimShapeFloat {
        profile_curve: ProfileCurve::Circle,
        ..base_shape()
    };
    let data = FlexibleData {
        // Softness 2 → `1 << 2` chain sections: enough that the chain visibly
        // curves rather than hinging once.
        softness: 2,
        tension: 1.0,
        air_friction: 2.0,
        gravity: 0.3,
        // No wind: the viewer's wind is a region field this scene has no grid to
        // get, and a zero keeps the motion a function of the fixture alone.
        wind_sensitivity: 0.0,
        // A steady sideways push, so the chain settles into a *bend* rather than
        // hanging straight down — a straight hang is exactly the picture a chain
        // that never ran also produces.
        user_force: Vector {
            x: 0.35,
            y: 0.0,
            z: 0.0,
        },
    };
    // Long and thin, as a flexi prim is: the chain's length is its Z scale.
    let scale = [0.16, 0.16, 2.5];
    let attributes = flexi_attributes(&data);
    let base_position = [0.0, 0.0, 0.0];
    let base_rotation = [0.0, 0.0, 0.0, 1.0];
    let chain = FlexiChain::new(&shape, &attributes, scale, base_position, base_rotation);
    let path = chain.path(base_position, base_rotation, scale);
    let prim = tessellate_with_path(&shape, FLEXI_LOD, &path);

    let object = commands
        .spawn((
            // Identity, and it has to be: a flexi prim's geometry is baked in
            // absolute metres by the chain, so the viewer gives it no geometry
            // holder scale either (`crate::objects`' `holder_transform`). A scale
            // here would shear the bent cross-section.
            Transform::IDENTITY,
            Visibility::default(),
            Name::new("flexi-streamer"),
            ChildOf(root),
            ObjectFlexi {
                data: data.clone(),
                scale,
            },
        ))
        .id();
    let face_entities: Vec<Entity> = to_bevy_prim_meshes(&prim)
        .into_iter()
        .enumerate()
        .map(|(index, mesh)| {
            let face = spawn_geometry(
                format!("flexi-streamer/face-{index}"),
                mesh,
                matte(Color::srgb(0.75, 0.7, 0.55)),
                Transform::IDENTITY,
                object,
                commands,
                assets,
            );
            // The bent geometry leaves the spawn-time AABB, so Bevy's frustum cull
            // (which reads it) can drop the prim wrongly — the viewer opts a flexi
            // face out for the same reason.
            commands.entity(face).insert(NoFrustumCulling);
            face
        })
        .collect();
    commands.entity(object).insert(FlexiSimState {
        chain,
        shape,
        softness: data.softness,
        face_entities,
    });
}

// ---------------------------------------------------------------------------
// Texture animation.
// ---------------------------------------------------------------------------

/// [`SCENES`] `texture-anim-flipbook`: a prim paging through a texture atlas.
///
/// **Dynamic, and dynamic in a way no earlier scene was.** Nothing about this prim's
/// *geometry* changes: [`drive_texture_animations`] rewrites each face material's
/// `uv_transform`, and the vertex buffer it samples through is the same one at
/// every sample. That is why `crate::render_test`'s notion of "did anything happen"
/// reads the material and the world transform as well as the vertices — a
/// vertex-only digest reports this scene as frozen while it plays perfectly.
///
/// The animation is a 4×4 flipbook at 8 frames a second: the reference viewer's
/// `llSetTextureAnim(ANIM_ON | LOOP, ALL_SIDES, 4, 4, 0.0, 0.0, 8.0)`, the form
/// nearly every animated texture in-world takes. `length` of zero means "every
/// frame in the grid", so the sixteenth frame wraps back to the first — which is
/// the case a scene would catch animating off the end of its atlas.
fn texture_anim_flipbook(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let anim = TextureAnimation {
        mode: texture_anim_mode::ON | texture_anim_mode::LOOP,
        // Every face, as `ALL_SIDES` does.
        face: -1,
        size_x: 4,
        size_y: 4,
        start: 0.0,
        // Zero: the whole `size_x * size_y` grid.
        length: 0.0,
        rate: 8.0,
    };
    let image = assets.images.add(to_bevy_image(&uv_reference_texture()));
    let object = commands
        .spawn((
            Transform::IDENTITY,
            Visibility::default(),
            Name::new("texture-anim-flipbook"),
            ChildOf(root),
            ObjectTextureAnimation { anim },
        ))
        .id();
    let prim = tessellate(&base_shape(), cx.lod);
    for (index, mesh) in to_bevy_prim_meshes(&prim).into_iter().enumerate() {
        let mesh = assets.meshes.add(mesh);
        // One material per face, unshared — as the viewer's `face_material` builds
        // them, and as this scene needs: the driver writes each face's own
        // `uv_transform`, so a shared material would have the faces fight over it.
        let material = assets.materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(image.clone()),
            perceptual_roughness: 0.9,
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Name::new(format!("texture-anim-flipbook/face-{index}")),
            ChildOf(object),
            // The two components the driver joins a face to its animation by: the
            // Linden face index it addresses (`ALL_SIDES` or one of them) and the
            // static texture entry it falls back to for every component the
            // animation does not drive.
            PrimFaceEntity {
                face_id: PrimFaceId::new(u16::try_from(index).unwrap_or(0)),
            },
            FaceTextureDebug(TextureFace::new(TextureKey::from(Uuid::nil()))),
        ));
    }
}

/// [`SCENES`] `particles-fountain`: a live particle source.
fn particles_fountain(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    _assets: &mut SceneAssets<'_>,
) {
    // The emitter carries no geometry of its own: `drive_particles` reads the
    // source's `ObjectParticleSystem` and world pose and builds the cloud mesh
    // itself, on its own entity. So this scene's *renderable* does not exist
    // until time has passed — which is the whole reason the timeline exists.
    commands.spawn((
        ObjectParticleSystem {
            system: fountain_system(),
        },
        Transform::from_xyz(0.0, 0.0, -2.0),
        Name::new("particles-fountain/emitter"),
        ChildOf(root),
    ));
}

// ---------------------------------------------------------------------------
// Surface flags: bump, shiny, glow, fullbright — and legacy materials.
// ---------------------------------------------------------------------------

/// The bump-code the [`bump_face`] scene embosses with: `BE_BRIGHTNESS`, which
/// derives its normal map from the face's **own diffuse luminance**.
///
/// Deliberately not one of the fifteen standard emboss codes (woodgrain, gravel,
/// siding, …). Those are correct and they are also a **fetch**: the code names a
/// fixed Linden texture UUID the viewer pulls over the asset capability, which is
/// exactly the grid this registry does not have. Brightness is the one bump source
/// that is a pure function of a texture already in hand, so it is the one a scene
/// can drive — the same reasoning that keeps [`tree`] untextured.
const BUMP_BRIGHTNESS: u8 = 1;

/// A [`TextureFace`] with the packed bump / shiny / fullbright byte and glow set.
///
/// The packing is the reference's `LLTextureEntry`: bump in the low 5 bits, shiny
/// in bits 6–7, fullbright in bit 8. Written here rather than reached for through
/// a setter because the fixture's intent *is* the packed byte — this is the value
/// the wire carries, and `crate::bump`'s `apply_surface_flags` is what has to read
/// it back out correctly.
fn flagged_face(bump: u8, shiny: u8, fullbright: bool, glow: f32) -> TextureFace {
    let packed = (bump & 0x1F) | ((shiny & 0x03) << 5) | if fullbright { 0x80 } else { 0x00 };
    TextureFace {
        bump_shiny_fullbright: packed,
        glow,
        ..TextureFace::new(TextureKey::from(Uuid::nil()))
    }
}

/// [`SCENES`] `bump-face`: a row of prims carrying the four surface flags.
///
/// Four prims rather than one, because the four flags are not independent pictures
/// of the same thing and a single prim carrying all of them would be a picture of
/// none of them: a fullbright face is unlit, so a shiny highlight on it is
/// invisible, and a glow ramp on top of that is unreadable.
///
/// The bump prim is the one with a check behind it. `crate::bump` generates its
/// normal map from the diffuse's luminance and uploads it — and a normal map is a
/// **texture**, so it is R22h's shape all over again: a map that clamps where the
/// face repeats smears its edge texel across the surface. That is the fifth
/// sampler-setting path `sampler_violations` was written for, and until this scene
/// existed the check had never seen it.
fn bump_face(cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    let decoded = Arc::new(uv_reference_texture());
    let diffuse = assets.images.add(to_bevy_image(&decoded));
    // Through the real generator, which is where the sampler is set — building the
    // normal map by hand here would be testing the fixture.
    let normal = assets
        .images
        .add(generate_normal_map(&decoded, /* invert */ false));

    for (offset, name, face, bumped) in [
        (
            -2.4_f32,
            "bump",
            flagged_face(BUMP_BRIGHTNESS, 0, false, 0.0),
            true,
        ),
        // Shiny 3 = the reference's `SHINY_HIGH`.
        (-0.8, "shiny", flagged_face(0, 3, false, 0.0), false),
        (0.8, "glow", flagged_face(0, 0, false, 0.6), false),
        (2.4, "fullbright", flagged_face(0, 0, true, 0.0), false),
    ] {
        let mut material = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(diffuse.clone()),
            perceptual_roughness: 0.9,
            ..default()
        };
        // The single chokepoint every face path in the viewer funnels through
        // (`crate::textures`' `face_material`), so this is the real mapping and not
        // a re-statement of it.
        apply_surface_flags(&mut material, &face);
        if bumped {
            material.normal_map_texture = Some(normal.clone());
        }
        let material = assets.materials.add(material);
        let object = commands
            .spawn((
                Transform::from_xyz(offset, 0.0, 0.0),
                Visibility::default(),
                Name::new(format!("bump-face/{name}")),
                ChildOf(root),
            ))
            .id();
        let prim = tessellate(&base_shape(), cx.lod);
        for (index, mesh) in to_bevy_prim_meshes(&prim).into_iter().enumerate() {
            let mesh = assets.meshes.add(mesh);
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                Name::new(format!("bump-face/{name}/face-{index}")),
                ChildOf(object),
                FaceTextureDebug(face),
            ));
        }
    }
}

/// [`SCENES`] `legacy-material-face`: a prim wearing a legacy (normal / specular)
/// material.
///
/// The pre-PBR materials system, and still most of what is actually built in
/// Second Life. Two prims, at the two ends of the glossiness ramp, because what
/// `crate::legacy_materials` does with a legacy material is *map* it — glossiness
/// onto `perceptual_roughness`, environment intensity onto `reflectance` — and a
/// single sample cannot show a mapping.
///
/// The normal map is generated rather than fetched: the material's `normal_map` is
/// a grid asset UUID and there is nothing here to fetch it from, so the scene
/// supplies the decoded pixels and runs the real upload
/// ([`build_linear_image`] — linear, not sRGB, as a normal map must be) that
/// `apply_legacy_normal_maps` would have run. The scalars go through the real
/// [`apply_legacy_scalars`].
fn legacy_material_face(
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let decoded = Arc::new(uv_reference_texture());
    let diffuse = assets.images.add(to_bevy_image(&decoded));
    let normal = assets.images.add(build_linear_image(&Arc::new(
        // A normal map is not a colour image; the fixture's is the generated one,
        // so the scene shows a plausible surface rather than a colour ramp read as
        // normals.
        normal_map_texture(&decoded),
    )));

    for (offset, name, glossiness, environment) in
        [(-1.2_f32, "matte", 20_u8, 10_u8), (1.2, "glossy", 240, 200)]
    {
        let material = LegacyMaterial {
            // Nil: there is no grid, so the fetch half is not what this exercises.
            // `apply_legacy_scalars` reads it, and a nil id is also the in-world
            // case of a material with only a specular set.
            normal_map: TextureKey::from(Uuid::nil()),
            normal_offset: (0.0, 0.0),
            normal_repeat: (1.0, 1.0),
            normal_rotation: 0.0,
            specular_map: TextureKey::from(Uuid::nil()),
            specular_offset: (0.0, 0.0),
            specular_repeat: (1.0, 1.0),
            specular_rotation: 0.0,
            specular_color: [255; 4],
            specular_exponent: glossiness,
            environment_intensity: environment,
            // `DIFFUSE_ALPHA_MODE_NONE`: an opaque face. Not the `0 = blend` the
            // wire type's doc used to claim — the reference's enum is
            // none / blend / mask / emissive.
            diffuse_alpha_mode: 0,
            alpha_mask_cutoff: 0,
        };
        let mut standard = StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(diffuse.clone()),
            ..default()
        };
        apply_legacy_scalars(&mut standard, &material);
        // What `apply_legacy_normal_maps` drops in once the fetch lands.
        standard.normal_map_texture = Some(normal.clone());
        let standard = assets.materials.add(standard);
        let object = commands
            .spawn((
                Transform::from_xyz(offset, 0.0, 0.0),
                Visibility::default(),
                Name::new(format!("legacy-material-face/{name}")),
                ChildOf(root),
            ))
            .id();
        let prim = tessellate(&base_shape(), cx.lod);
        for (index, mesh) in to_bevy_prim_meshes(&prim).into_iter().enumerate() {
            let mesh = assets.meshes.add(mesh);
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(standard.clone()),
                Transform::IDENTITY,
                Name::new(format!("legacy-material-face/{name}/face-{index}")),
                ChildOf(object),
            ));
        }
    }
}

/// The [`uv_reference_texture`] as a tangent-space normal map: the same generator
/// `crate::bump` runs, re-read as a [`DecodedTexture`] so it can go back through
/// the legacy path's own upload.
///
/// A round trip on purpose. `crate::bump` owns "luminance → normals" and
/// `crate::legacy_materials` owns "decoded normal map → linear `Image`"; in-world
/// the second half's input is a fetched asset, which is what this stands in for.
fn normal_map_texture(source: &Arc<DecodedTexture>) -> DecodedTexture {
    let generated = generate_normal_map(source, false);
    DecodedTexture {
        width: generated.width(),
        height: generated.height(),
        components: 4,
        discard_level: DiscardLevel::FULL,
        pixels: Bytes::from(generated.data.unwrap_or_default()),
        aux: None,
    }
}

// ---------------------------------------------------------------------------
// The tree impostor.
// ---------------------------------------------------------------------------

/// [`SCENES`] `tree-billboard`: the far-distance tree impostor.
///
/// The level of detail below [`TreeLod::COARSEST`]: at range the reference viewer
/// stops drawing a tree's branches at all and draws two crossed alpha quads. The
/// [`tree`] scene never reaches it — `tree_geometry` and `billboard_geometry` are
/// different functions, and the LOD axis this harness sweeps is the *prim*
/// tessellation level, not the tree one. So this is the only thing that renders
/// the geometry every distant tree in a region is actually made of.
fn tree_billboard(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let Some(species) = tree_species(0) else {
        return;
    };
    let mesh = to_bevy_tree_mesh(&tree_billboard_geometry(species));
    spawn_geometry(
        "tree-billboard/impostor",
        mesh,
        matte(Color::srgb(0.35, 0.5, 0.3)),
        Transform::IDENTITY,
        root,
        commands,
        assets,
    );
}

// ---------------------------------------------------------------------------
// The atmosphere: four times of day, each with something to cast a shadow.
// ---------------------------------------------------------------------------

/// The transform that **undoes** the scene root's basis change, for the two viewer
/// modules that build their geometry in **Bevy space** rather than Second Life's.
///
/// Every other fixture here writes plain Second Life metres and lets the scene root
/// convert them once, because that is what the viewer does with everything a region
/// sends it. `crate::sky` and `crate::water` are the exception, and in neither place
/// is it an oversight: the atmosphere is not *in* the region. Both spawn at the
/// **world root** with an identity transform and build directly in Bevy's frame —
/// `build_star_mesh` picks "a random direction on the upper hemisphere (Bevy Y up)",
/// the cloud dome's stacks are "Bevy Y-up: y0 is up", and the ocean is a `Plane3d`
/// (XZ, `+Y` normal) placed at its height on Bevy **y**.
///
/// Under the scene root that geometry is rotated a **second** time, and the first
/// version of these scenes was: the cloud cap ended up on the horizon and the star
/// hemisphere on its side, both invisible from a camera looking up — while the sky
/// dome and the water plane rendered anyway, *because a sphere and a plane are
/// symmetric about the very rotation that was wrong*. Two scenes blank and two
/// silently correct for the wrong reason is precisely the failure this registry
/// exists to stop, so the asymmetry is written down here once rather than papered
/// over in four fixtures.
fn bevy_space() -> Transform {
    Transform::from_rotation(sl_to_bevy_rotation().inverse())
}

/// Where every sky scene's camera stands — **the same pose for all four**, and
/// that is the whole point of them being four.
///
/// The first version gave sunrise and sunset mirrored cameras, on the reasoning
/// that each should look toward its own sun. That is exactly wrong: mirroring the
/// viewpoint along with the sun cancels the thing being compared, so the two scenes
/// rendered *the same picture* — and, because the two poses sit 90° apart around the
/// origin, the shadow appeared to swing by 90° between them rather than the 180° the
/// sun actually moved. A fixed camera makes the four a comparison: the same box, the
/// same ground, and only the sky changing.
const SKY_CAMERA: SceneCamera = SceneCamera {
    position: Vec3::new(-10.0, -11.0, 5.0),
    look_at: Vec3::new(0.0, 0.0, 1.5),
};

/// One of Linden's four canonical WindLight sky presets, ported.
///
/// **These are content, and that is the entire point.** The first version of these
/// scenes moved the sun across one palette — the legacy WindLight default — and
/// produced a midnight nearly as bright as midday, which was filed as a viewer bug
/// ([[viewer-r27]]) and was not one. Second Life's night is dark because the
/// **midnight sky frame's `sunlight_color` is authored dark**: `A-12AM`'s is
/// `(0.35, 0.36, 0.66)` against `A-12PM`'s `(0.73, 0.78, 0.90)`, and the reference's
/// scene light is that colour attenuated by elevation. Nothing computes a night.
///
/// So a sky scene that does not carry a palette per time of day is not a sky scene
/// at all; it is one sky with the sun in the wrong place, which is an environment
/// that cannot exist in-world. The legacy WindLight default is a **single midday
/// frame** (the reference's `LLSettingsSky::defaults()` is too) — it has no night
/// in it to find.
///
/// The values are Linden's own, from the presets Firestorm ships in
/// `app_settings/windlight/skies/`, converted by the reference's own rules
/// (`LLSettingsSky::translateLegacySettings`): scalars are the `[0]` of their legacy
/// array, `star_brightness` is scaled by 250, and the bodies come from `sun_angle` /
/// `east_angle` — see [`sky_settings_from`]. Ported as constants rather than read
/// from disk because a scene that needs an asset is a scene that skips.
#[derive(Clone, Copy)]
struct SkyPreset {
    /// How this time names its scene and its entities.
    label: &'static str,
    /// The legacy `sunlight_color` — the one field that makes a night a night.
    sunlight: [f32; 3],
    /// The legacy `ambient`.
    ambient: [f32; 3],
    /// The legacy `blue_horizon`.
    blue_horizon: [f32; 3],
    /// The legacy `blue_density`.
    blue_density: [f32; 3],
    /// The legacy `cloud_color`.
    cloud_color: [f32; 3],
    /// The legacy `haze_horizon`.
    haze_horizon: f32,
    /// The legacy `haze_density`.
    haze_density: f32,
    /// The legacy `density_multiplier`.
    density_multiplier: f32,
    /// The legacy `distance_multiplier`.
    distance_multiplier: f32,
    /// The legacy `max_y`.
    max_y: f32,
    /// The legacy `gamma`.
    gamma: f32,
    /// The legacy `cloud_shadow`.
    cloud_shadow: f32,
    /// The legacy `cloud_scale`.
    cloud_scale: f32,
    /// The legacy `glow`.
    glow: [f32; 3],
    /// The legacy `star_brightness`, **before** the reference's 250x conversion.
    star_brightness: f32,
    /// The legacy `sun_angle`, in radians — the sun's altitude.
    sun_angle: f32,
    /// The legacy `east_angle`, in radians. Negated to an azimuth.
    east_angle: f32,
}

/// Linden's `A-6AM` preset, ported from `app_settings/windlight/skies/A-6AM.xml`.
const SUNRISE: SkyPreset = SkyPreset {
    label: "sky-sunrise",
    sunlight: [2.37, 2.37, 2.37],
    ambient: [0.81, 0.4629, 0.63],
    blue_horizon: [0.2067, 0.4099, 0.48],
    blue_density: [0.1579, 0.435, 0.87],
    cloud_color: [0.2262, 0.2262, 0.2262],
    haze_horizon: 0.16,
    haze_density: 0.54,
    density_multiplier: 0.000_620,
    distance_multiplier: 2.6999,
    max_y: 563.0,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.001, 0.001, -0.48],
    star_brightness: 0.0,
    sun_angle: 0.0942,
    east_angle: 0.0,
};

/// Linden's `A-12PM` preset, ported from `app_settings/windlight/skies/A-12PM.xml`.
const MIDDAY: SkyPreset = SkyPreset {
    label: "sky-midday",
    sunlight: [0.7342, 0.7816, 0.9],
    ambient: [1.05, 1.05, 1.05],
    blue_horizon: [0.4955, 0.4955, 0.64],
    blue_density: [0.2448, 0.4487, 0.76],
    cloud_color: [0.41, 0.41, 0.41],
    haze_horizon: 0.19,
    haze_density: 0.7,
    density_multiplier: 0.000_180,
    distance_multiplier: 0.8,
    max_y: 1605.0,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.0, 0.001, -0.48],
    star_brightness: 0.0,
    // The preset's literal 1.5708 is pi/2 — the sun at the zenith.
    sun_angle: FRAC_PI_2,
    east_angle: 0.0,
};

/// Linden's `A-6PM` preset, ported from `app_settings/windlight/skies/A-6PM.xml`.
const SUNSET: SkyPreset = SkyPreset {
    label: "sky-sunset",
    sunlight: [2.8386, 2.8386, 2.8386],
    ambient: [1.02, 0.81, 0.81],
    blue_horizon: [0.1077, 0.2135, 0.25],
    blue_density: [0.1452, 0.4, 0.8],
    cloud_color: [0.2262, 0.2262, 0.2262],
    haze_horizon: 0.16,
    haze_density: 0.7,
    density_multiplier: 0.000_460,
    distance_multiplier: 1.0,
    max_y: 562.5,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.0, 0.001, -0.48],
    star_brightness: 0.0,
    sun_angle: 3.0662,
    east_angle: 0.0,
};

/// Linden's `A-12AM` preset, ported from `app_settings/windlight/skies/A-12AM.xml`.
const MIDNIGHT: SkyPreset = SkyPreset {
    label: "sky-midnight",
    sunlight: [0.3488, 0.3557, 0.66],
    ambient: [0.2041, 0.2425, 0.33],
    blue_horizon: [0.24, 0.24, 0.24],
    blue_density: [0.45, 0.45, 0.45],
    cloud_color: [0.2262, 0.2262, 0.2262],
    haze_horizon: 0.0,
    haze_density: 4.0,
    density_multiplier: 0.000_300,
    distance_multiplier: 0.0,
    max_y: 906.2,
    gamma: 1.0,
    cloud_shadow: 0.27,
    cloud_scale: 0.42,
    glow: [5.0, 0.001, -0.48],
    star_brightness: 2.0,
    sun_angle: 4.7124,
    east_angle: 0.0,
};

/// Build a [`SkySettings`] from a ported preset, by the reference's own legacy →
/// EEP conversion (`LLSettingsSky::translateLegacySettings`).
///
/// The two rules worth stating, because both are easy to get subtly wrong:
///
/// - **The bodies come from the angles.** `azimuth = -east_angle` ("get
///   counter-clockwise radian angle from clockwise legacy WL east angle") and
///   `altitude = sun_angle`; the moon is **diametrically opposed**, at
///   `(azimuth + PI, -altitude)`. That is what makes `A-12AM` a night with no
///   special casing: its `sun_angle` of 4.7124 rad (270°) puts the sun straight
///   down, so the moon is straight up and *it* is the light — and since the
///   reference shares one colour between the two bodies, the moon's light is the
///   frame's own dark blue.
/// - **`star_brightness` is scaled by 250.** `A-12AM`'s legacy `2.0` becomes `500`,
///   which the star shader's `star_brightness / 500` turns into a fully visible
///   field; `A-12PM`'s `0.0` hides it. So the stars come and go with the time of day
///   for free, from the data, rather than from a flag in the fixture.
fn sky_settings_from(preset: &SkyPreset) -> SkySettings {
    let azimuth = -preset.east_angle;
    let altitude = preset.sun_angle;
    let [sun_r, sun_g, sun_b] = preset.sunlight;
    let [amb_r, amb_g, amb_b] = preset.ambient;
    let [bh_r, bh_g, bh_b] = preset.blue_horizon;
    let [bd_r, bd_g, bd_b] = preset.blue_density;
    let [cc_r, cc_g, cc_b] = preset.cloud_color;
    let [glow_x, glow_y, glow_z] = preset.glow;
    SkySettings {
        sun_rotation: azimuth_altitude_to_rotation(azimuth, altitude),
        moon_rotation: azimuth_altitude_to_rotation(azimuth + PI, -altitude),
        // The alpha is unused by the shader (`sky_params` reads rgb), and the
        // reference's own EEP defaults carry a zero there.
        sunlight_color: ColorAlpha::new(sun_r, sun_g, sun_b, 0.0),
        ambient: SlColor::new(amb_r, amb_g, amb_b),
        blue_horizon: SlColor::new(bh_r, bh_g, bh_b),
        blue_density: SlColor::new(bd_r, bd_g, bd_b),
        cloud_color: SlColor::new(cc_r, cc_g, cc_b),
        haze_horizon: preset.haze_horizon,
        haze_density: preset.haze_density,
        density_multiplier: preset.density_multiplier,
        distance_multiplier: preset.distance_multiplier,
        max_y: preset.max_y,
        gamma: preset.gamma,
        cloud_shadow: preset.cloud_shadow,
        cloud_scale: preset.cloud_scale,
        glow: Glow::new(glow_x, glow_y, glow_z),
        star_brightness: preset.star_brightness * 250.0,
        ..SkySettings::legacy_windlight_default(preset.label)
    }
}

/// The sky scenes' shared placeholder texture.
///
/// Every material in the atmosphere stack samples a grid texture — the sky's
/// rainbow and halo, the sun and moon discs, the cloud noise, the star bloom — and
/// every one is a UUID this registry has no capability to fetch. So the scenes stand
/// exactly where a real login's sky stands between `setup_sky` and the first
/// `apply_sky_textures`: real geometry, real uniforms, and the placeholder the
/// viewer seeds them with.
fn sky_placeholder(assets: &mut SceneAssets<'_>) -> Handle<Image> {
    assets.images.add(sky_placeholder_image())
}

/// Spawn one complete sky at `time`: the atmosphere, the clouds, the stars, the two
/// discs, the light the sky yields — and a lit box on the ground for that light to
/// throw a shadow of.
///
/// **One scene rather than four modules' worth**, because the sky is the registry's
/// own argument for scenes over objects taken to its limit: a star field is
/// meaningless at midday, a sun disc has nothing to be seen against without the dome
/// behind it, and the whole point of a time of day is what it does to the light. The
/// earlier split (`sky-dome`, `cloud-dome`, `star-field`) rendered each of those
/// alone, where the only judgement available was "is a thing there".
///
/// The ground and the box are the half that makes the light checkable. Everything
/// above the horizon is emissive — it renders whatever the light is doing — so a
/// scene of nothing but sky cannot show that the sun is in the wrong place, that its
/// colour is wrong, or that it casts no shadow at all. A box on a plane shows all
/// three, and shows them differently at each of the four times.
fn spawn_sky_at(
    preset: &SkyPreset,
    cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    let sky = sky_settings_from(preset);
    // The viewer's real derivation, shared with `drive_sky` — see `ResolvedSky`.
    let resolved = resolve_sky(&sky);
    let label = preset.label;
    let placeholder = sky_placeholder(assets);

    // The Bevy-space subtree. See `bevy_space`: everything the atmosphere builds is
    // already in Bevy's frame, so it hangs here rather than under the scene root's
    // Second Life one.
    let space = commands
        .spawn((
            bevy_space(),
            Visibility::default(),
            Name::new(format!("{label}/atmosphere")),
            ChildOf(root),
        ))
        .id();

    // The atmosphere dome.
    let sky_material = assets.sky_materials.add(SkyMaterial {
        params: resolved.params,
        rainbow: placeholder.clone(),
        halo: placeholder.clone(),
    });
    commands.spawn((
        Mesh3d(assets.meshes.add(Mesh::from(Sphere::new(SKY_DOME_RADIUS)))),
        MeshMaterial3d(sky_material),
        Transform::IDENTITY,
        Name::new(format!("{label}/dome")),
        ChildOf(space),
        NotShadowCaster,
        WorldScaleGeometry {
            max_extent: SKY_DOME_RADIUS * 1.01,
            reason: "the atmosphere is drawn as a 3 km dome around the viewpoint, not as an \
                     object in the region",
        },
    ));

    // The cloud layer, on its own far larger dome.
    let cloud_material = assets.cloud_materials.add(CloudMaterial {
        params: cloud_params(
            &sky,
            resolved.lightnorm,
            resolved.sun_up_factor,
            resolved.glow_factor,
            // No scroll: `drive_clouds` accumulates it from the frame clock, and a
            // scene that declared a still sky must not quietly drift.
            Vec2::ZERO,
        ),
        cloud_noise: placeholder.clone(),
        cloud_noise_next: placeholder.clone(),
    });
    commands.spawn((
        Mesh3d(assets.meshes.add(build_cloud_dome_mesh())),
        MeshMaterial3d(cloud_material),
        Transform::IDENTITY,
        Name::new(format!("{label}/clouds")),
        ChildOf(space),
        NotShadowCaster,
        WorldScaleGeometry {
            // The dome's `[0, π/8]` cap, whose rim reaches ~5.7 km horizontally —
            // not the 15 km radius it is struck at, because the cap is shallow and
            // its centre is lowered by the baked camera height.
            max_extent: 6_000.0,
            reason: "the cloud layer is a shallow cap of a 15 km dome around the viewpoint, so \
                     its rim reaches far past any region even though it is only ~600 m overhead",
        },
    ));

    // The stars. Present at every time of day, as they are in the viewer — the
    // shader fades them out by the sky's `star_brightness`, so what makes them a
    // night-only sight is the sky frame and not their absence.
    let star_material = assets.star_materials.add(StarMaterial {
        params: StarParams {
            // The reference's `star_brightness / 500`, clamped: the same value
            // `drive_stars` folds in, so the field fades with the time of day
            // rather than being pinned visible.
            custom_alpha: (sky.star_brightness / 500.0).min(1.0),
            time: 0.0,
            reserved: Vec2::ZERO,
        },
        diffuse: placeholder.clone(),
    });
    commands.spawn((
        Mesh3d(assets.meshes.add(build_star_mesh())),
        MeshMaterial3d(star_material),
        Transform::IDENTITY,
        Name::new(format!("{label}/stars")),
        ChildOf(space),
        NotShadowCaster,
        WorldScaleGeometry {
            max_extent: STAR_DOME_RADIUS * 1.01,
            reason: "the stars are drawn on a 2.9 km dome around the viewpoint, just inside the \
                     sky's",
        },
    ));

    // The two discs, each shown only when its body is above the horizon — the
    // reference's `getIsSunUp` / `getIsMoonUp`, which `drive_sun_moon_discs` applies
    // every frame.
    for (name, direction, up, moon_mode, scale, radius) in [
        (
            "sun-disc",
            resolved.sun_dir,
            resolved.sun_up,
            0.0_f32,
            sky.sun_scale,
            SUN_DISK_RADIUS,
        ),
        (
            "moon-disc",
            resolved.moon_dir,
            resolved.moon_up,
            1.0,
            sky.moon_scale,
            MOON_DISK_RADIUS,
        ),
    ] {
        let material = assets.sun_disc_materials.add(SunDiscMaterial {
            params: SunDiscParams {
                brightness: sky.moon_brightness,
                blend_factor: 0.0,
                moon_mode,
                // The reference fades the moon near the horizon by its up
                // component; the sun ignores it.
                up_component: direction.y,
            },
            diffuse: placeholder.clone(),
            alt_diffuse: placeholder.clone(),
        });
        commands.spawn((
            Mesh3d(assets.meshes.add(Mesh::from(Rectangle::new(1.0, 1.0)))),
            MeshMaterial3d(material),
            // Through the viewer's real billboard placement. It centres the disc on
            // the *camera* every frame; here the camera is a fixed pose a few metres
            // from the origin and the disc is 2 km out, so standing it off the origin
            // instead is a parallax error of a fraction of a percent — and a scene
            // must not move with a camera a human is about to orbit.
            disc_transform(Vec3::ZERO, direction, scale, radius),
            Name::new(format!("{label}/{name}")),
            ChildOf(space),
            NotShadowCaster,
            if up {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        ));
    }

    // The light the sky yields, with the viewer's own cascade configuration — this
    // is what the ground and the box below are here to show.
    commands.spawn((
        DirectionalLight {
            illuminance: SCENE_LIGHT_ILLUMINANCE,
            shadow_maps_enabled: true,
            color: Color::linear_rgb(
                resolved.diffuse[0].clamp(0.0, 1.0),
                resolved.diffuse[1].clamp(0.0, 1.0),
                resolved.diffuse[2].clamp(0.0, 1.0),
            ),
            ..default()
        },
        shadow_cascades(),
        // The light travels *away* from its body, so its forward is the negated
        // light direction — as `drive_sky` aims it. A safe up when the body is near
        // the zenith, where `looking_to`'s up degenerates.
        Transform::default().looking_to(
            Vec3::new(
                -resolved.light_dir.x,
                -resolved.light_dir.y,
                -resolved.light_dir.z,
            ),
            if resolved.light_dir.y.abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            },
        ),
        Name::new(format!("{label}/light")),
        ChildOf(space),
    ));

    // The ground, and something floating over it. Ordinary Second Life prims through
    // the real tessellator, under the scene root's own basis — the sky is the
    // exception here, not the region.
    //
    // The two sizes are set by the geometry of a shadow, and neither is arbitrary.
    //
    // The box **floats** rather than resting on the ground, because a resting box is
    // exactly the case where the shadow cannot be seen: at midday the sun is 80° up,
    // so the shadow lands directly beneath the box — which is where the box is. The
    // same is true of the 65° moon. A metre of air under it puts the shadow on
    // ground the camera can see.
    //
    // The ground is then **60 m** rather than a few, because floating the box is what
    // makes a low sun's shadow long: the displacement is `height / tan(elevation)`,
    // so the 3° sun of sunrise and sunset throws this box's shadow about **19 m** —
    // clean off a 28 m plane, which is what the first version of this had. Sized to
    // hold the longest shadow the four times of day actually cast.
    spawn_prim(
        &format!("{label}/ground"),
        &base_shape(),
        cx.lod,
        Color::srgb(0.62, 0.60, 0.55),
        Transform::from_xyz(0.0, 0.0, -0.15).with_scale(Vec3::new(60.0, 60.0, 0.3)),
        root,
        commands,
        assets,
    );
    spawn_prim(
        &format!("{label}/caster"),
        &base_shape(),
        cx.lod,
        Color::srgb(0.80, 0.78, 0.74),
        // A 2 m box centred 2 m up: its underside sits a metre clear of the ground.
        Transform::from_xyz(0.0, 0.0, 2.0).with_scale(Vec3::splat(2.0)),
        root,
        commands,
        assets,
    );
}

/// [`SCENES`] `sky-sunrise`.
fn sky_sunrise(cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    spawn_sky_at(&SUNRISE, cx, root, commands, assets);
}

/// [`SCENES`] `sky-midday`.
fn sky_midday(cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    spawn_sky_at(&MIDDAY, cx, root, commands, assets);
}

/// [`SCENES`] `sky-sunset`.
fn sky_sunset(cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    spawn_sky_at(&SUNSET, cx, root, commands, assets);
}

/// [`SCENES`] `sky-midnight`.
fn sky_midnight(cx: SceneCx, root: Entity, commands: &mut Commands, assets: &mut SceneAssets<'_>) {
    spawn_sky_at(&MIDNIGHT, cx, root, commands, assets);
}

/// A tiling wave normal map: the wavelets a sea's surface *is*.
///
/// The scene needs this because the alternative is a scene of nothing. The viewer
/// fetches its wave normal map from the grid (`DEFAULT_WATER_NORMAL`) and, until it
/// arrives, wears [`flat_normal_image`](crate::water::flat_normal_image) — a **1×1**
/// perfectly flat normal, whose own doc admits it renders "a fresnel-tinted flat
/// sea". With no grid that placeholder is forever, and a flat normal gives the
/// shader no slope anywhere: no fresnel variation, no specular, no wave. Which is
/// exactly what the first version of this scene showed — a sheet of dark blue fog
/// with no surface on it.
///
/// So the fixture generates one, per this registry's convention that fixtures are
/// procedural. It is a **sum of three sine waves at integer wave numbers**, which is
/// what makes it tile: an integer number of periods across the image means the left
/// edge meets the right exactly, and the wave shader scrolls these texcoords far
/// outside `[0, 1]`, so a seam would be visible over and over.
///
/// The normals are computed **analytically** — the derivative of a sum of sines is a
/// sum of cosines — rather than by finite differences over a rendered height field.
/// A generated map has an exact gradient available and no reason to approximate its
/// own.
fn water_wavelet_texture() -> DecodedTexture {
    const SIZE: u32 = 128;
    /// Each wave as `(u wave number, v wave number, amplitude)`. Integer wave
    /// numbers so the field tiles; three of them at different angles so the surface
    /// reads as water rather than as corduroy.
    const WAVES: [(f32, f32, f32); 3] = [(3.0, 1.0, 0.030), (-2.0, 5.0, 0.016), (7.0, -4.0, 0.008)];

    let extent = f32::from(u16::try_from(SIZE).unwrap_or(1));
    let mut pixels: Vec<u8> = Vec::new();
    for y in 0..SIZE {
        let v = f32::from(u16::try_from(y).unwrap_or(0)) / extent;
        for x in 0..SIZE {
            let u = f32::from(u16::try_from(x).unwrap_or(0)) / extent;
            // The height field's slope, summed per wave:
            //   h(u,v) = Σ a·sin(τ(fu·u + fv·v))
            //   ∂h/∂u  = Σ a·τ·fu·cos(τ(fu·u + fv·v))
            let (mut slope_u, mut slope_v) = (0.0_f32, 0.0_f32);
            for (wave_u, wave_v, amplitude) in WAVES {
                let phase = TAU * (wave_u * u + wave_v * v);
                let derivative = amplitude * TAU * phase.cos();
                slope_u += derivative * wave_u;
                slope_v += derivative * wave_v;
            }
            // The surface `z = h(u, v)` has tangent-space normal `(-∂h/∂u, -∂h/∂v, 1)`.
            let length = (slope_u * slope_u + slope_v * slope_v + 1.0).sqrt();
            for component in [-slope_u / length, -slope_v / length, 1.0 / length] {
                // A tangent-space normal is stored biased into `[0, 1]`.
                pixels.push(float_to_u8(((component * 0.5 + 0.5) * 255.0).round()));
            }
            pixels.push(255);
        }
    }
    DecodedTexture {
        width: SIZE,
        height: SIZE,
        components: 4,
        discard_level: DiscardLevel::FULL,
        pixels: Bytes::from(pixels),
        aux: None,
    }
}

/// Where the [`water_surface`] scene's camera stands, in Second Life metres.
///
/// A constant rather than a literal in two places: the water shader needs the
/// camera position in its uniforms, and the registry needs it as the scene's pose.
/// If the two drifted apart the sea would be lit for a viewpoint nobody is at —
/// which is a wrong picture with no other symptom, since it still renders.
const WATER_CAMERA: Vec3 = Vec3::new(0.0, -40.0, 26.0);

/// [`SCENES`] `water-surface`: the endless ocean and a region's water plane.
///
/// Both, because they are two surfaces the viewer draws at almost the same height
/// and the interesting question is what happens where they meet: the ocean is one
/// 40 km plane kept under the camera, and each region gets its own 256 m plane at
/// *its* water height, biased a hair above so it wins the depth test. A scene with
/// only one of them could not show the bias mattering.
///
/// The camera position is **passed into the uniforms**, and it is not a detail:
/// the water shader derives its fresnel and its wave normals from the view vector,
/// so `default_water_params`' placeholder origin renders the sea as a flat sheet of
/// fog colour — which is exactly what the first version of this scene showed. The
/// viewer re-sets it every frame from the live camera (`drive_water`); a scene has
/// a declared pose instead, so it uses that.
fn water_surface(
    _cx: SceneCx,
    root: Entity,
    commands: &mut Commands,
    assets: &mut SceneAssets<'_>,
) {
    // The real upload the fetched map goes through — linear, and repeating. See
    // `water_wavelet_texture` for why the scene generates a map at all rather than
    // wearing the viewer's flat placeholder.
    let normal = assets
        .images
        .add(water_normal_image(&water_wavelet_texture()));
    // Midday's sun, so the sea is lit from where the sky scenes put it.
    let resolved = resolve_sky(&sky_settings_from(&MIDDAY));
    // The scene's own declared camera pose, in Bevy space — the water's frame.
    let camera = sl_to_bevy_rotation().mul_vec3(WATER_CAMERA);
    let material = assets.water_materials.add(WaterMaterial {
        params: water_params(
            &WaterSettings::legacy_default("Default"),
            resolved.light_dir,
            camera,
            // The sky's own horizon colour would need the atmosphere resolved per
            // pixel; `drive_water` uses a sampled reflection tint, and this is its
            // pre-environment seed.
            Vec3::new(0.5, 0.6, 0.8),
            Vec3::from_array(resolved.diffuse),
            0.0,
        ),
        // Both slots share the map, as `apply_water_textures` does until a day
        // cycle drives a separate next frame and a blend between them.
        normal_map: normal.clone(),
        normal_map_next: normal,
    });
    // See `bevy_space`: `crate::water` builds in Bevy's frame at the world root.
    let space = commands
        .spawn((
            bevy_space(),
            Visibility::default(),
            Name::new("water-surface/sea"),
            ChildOf(root),
        ))
        .id();
    for (name, extent, height) in [
        ("ocean", 20_000.0_f32, DEFAULT_WATER_HEIGHT),
        // The region plane, a hair above the ocean — `crate::water`'s
        // `OCEAN_DEPTH_BIAS`, the thing that stops the two z-fighting.
        ("region-plane", 128.0, DEFAULT_WATER_HEIGHT + 0.02),
    ] {
        let mesh = assets.meshes.add(
            Plane3d::default()
                .mesh()
                .size(2.0 * extent, 2.0 * extent)
                .build(),
        );
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(0.0, height, 0.0),
            Name::new(format!("water-surface/{name}")),
            ChildOf(space),
            // The water never casts shadows, as the viewer has it.
            NotShadowCaster,
            WorldScaleGeometry {
                max_extent: 21_000.0,
                reason: "the endless ocean is one 40 km plane kept centred under the camera, \
                         rather than a surface per region",
            },
        ));
    }
}
