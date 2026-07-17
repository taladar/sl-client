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

use std::f32::consts::{PI, TAU};

use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::Affine2;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bytes::Bytes;
use sl_client_bevy::{
    BaseMesh, BevySkeleton, ParticleSystem, ReflectionProbe, ReflectionProbeFlags, Skeleton,
    Vector, VertexWeights,
};
use sl_client_bevy::{
    DecodedMesh, DecodedTexture, DiscardLevel, HoleType, MeshLod, MeshSkin, PathCurve, PrimLod,
    PrimShapeFloat, ProfileCurve, Submesh, TreeLod, grass_geometry, grass_species,
    rigged_inverse_bindposes, tessellate, tessellate_sculpt, to_bevy_base_mesh, to_bevy_grass_mesh,
    to_bevy_image, to_bevy_mesh, to_bevy_prim_meshes, to_bevy_rigged_mesh, to_bevy_tree_mesh,
    tree_geometry, tree_species,
};

use std::path::Path;

use crate::avatar_assets::AvatarAssetLibrary;
use crate::coords::sl_to_bevy_rotation;
use crate::particles::{ObjectParticleSystem, float_to_u8};
use crate::probes::ObjectReflectionProbe;

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
/// Bundled rather than passed as four arguments so a fixture's signature stays
/// readable, and so adding a collection later does not touch every scene.
pub(crate) struct SceneAssets<'assets> {
    /// The mesh collection the fixture's geometry is added to.
    pub(crate) meshes: &'assets mut Assets<Mesh>,
    /// The material collection.
    pub(crate) materials: &'assets mut Assets<StandardMaterial>,
    /// The image collection, for a fixture that needs a texture.
    pub(crate) images: &'assets mut Assets<Image>,
    /// The inverse-bindpose collection a rigged fixture's `SkinnedMesh` binds
    /// against.
    pub(crate) inverse_bindposes: &'assets mut Assets<SkinnedMeshInverseBindposes>,
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
