//! The headless half of the render test harness (`viewer-render-test-harness`):
//! enough of the viewer to build **real geometry, through the real converters,
//! in `cargo test`** — no window, no GPU, no login, no region, no OAR.
//!
//! # Why
//!
//! Because the alternative is what the `R*` list in `roadmap/bugs/` was found
//! by, and more importantly what it was *missed* by. Seeing a prim today means:
//! start OpenSim, log in, provision the object by OAR import (whose UUID is
//! regenerated, so look the real one up in `bin/OpenSim.db` first), fly the
//! camera there, squint. That is minutes per iteration, needs a human, and half
//! of it is grid administration rather than rendering.
//!
//! The `sl-client-r22-avatar-render-wip` memory records the cost exactly: R22
//! was split into seven sub-items, three of them "committed but do NOT visibly
//! fix" — because the only way to tell was another login, another bake, another
//! screenshot. This module ends that loop for everything a machine can decide.
//!
//! # What a machine can actually decide
//!
//! More than it looks like, and none of it needs an eye. Each check is here
//! because the bug it catches has already been paid for once:
//!
//! - [`skin_violations`] — **R1**: Bevy does not renormalize skin weights, so a
//!   wire rig's 0.9 sum drags every vertex a tenth of the way to the mesh
//!   origin. Found by a login, a bake, and a CPU reimplementation of GPU
//!   skinning to compare against. It is a sum and a subtraction.
//! - [`skin_violations`] — **R13**: a vertex weighted onto a joint outside the
//!   render list reads a garbage matrix. Found as an armpit spike, localised
//!   with a bespoke geometry-logging env var. It is a comparison.
//! - [`sampler_violations`] — **R22h**: Bevy's default sampler clamps where
//!   Second Life repeats, smearing an edge texel across a face. Found as a
//!   "white torso" over a bake that was itself correct.
//! - [`LogCapture`] — **R26**: a zero-particle cloud's empty mesh made Bevy's
//!   allocator log every frame. A bug whose *only* symptom was a log line.
//! - [`geometry_violations`] — NaN positions, non-unit normals, out-of-range
//!   indices. Never caught, because they render as *something*.
//!
//! Every one of those is a pure function of data, and needs no grid.
//!
//! # The tiers, and why one rule does not fit
//!
//! The same structure [`crate::ui_test`] arrived at, because the same reasoning
//! applies:
//!
//! | Tier | Question | Who decides |
//! | --- | --- | --- |
//! | **Universal** | is this broken? | [`geometry_violations`], for every scene, no opt-in |
//! | **Declared** | does it match its stated intent? | the scene ([`DeclaredBounds`], [`SymmetricAbout`]) |
//! | **Timeline** | did anything actually happen? | the scene's `Timeline` |
//!
//! The universal tier only catches what is **wrong**. Geometry also has
//! properties that are not wrong at any particular value and must not change by
//! accident — the size a box comes out at, the symmetry a sphere has. Nothing is
//! incorrect if they move; they are simply not allowed to move without somebody
//! saying so. Recording those into a committed baseline is
//! `viewer-ui-baseline-regressions`' tier, and this harness should share that
//! format rather than grow a second one.
//!
//! # What this is not
//!
//! Geometry only. Nothing here rasterises a triangle, so it cannot answer "did
//! the right pixels light up". That needs a GPU, and is kept separate for
//! exactly that reason: the cheap tier must never depend on the expensive one,
//! or a machine without an adapter loses the tier holding most of the value.

use core::fmt::Write as _;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::image::{ImageAddressMode, ImageSampler};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, MeshVertexAttribute, VertexAttributeValues};
use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use tracing::field::{Field, Visit};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry;

use crate::camera::FlyCamera;
use crate::particles::{ParticleSim, drive_particles, setup_particles};
use crate::render_scene::{
    DeclaredBounds, RenderScene, SamplerMayClamp, SceneAssets, SceneCx, SymmetricAbout,
    SymmetryAxis, UvsInUnitSquare, scene_root,
};
use crate::textures::TextureManager;

/// A boxed error, so a test can use `?` rather than the workspace-denied
/// `unwrap` / `expect`.
pub(crate) type TestError = Box<dyn core::error::Error>;

/// The fixed step every [`advance_to`] frame takes.
///
/// Manual, never the wall clock. A harness whose results depend on how fast the
/// machine ran it is a harness that flakes, and a particle count sampled at
/// "whatever 0.5 s of real time got through" is exactly that. 16 ms is a
/// plausible 60 Hz frame, so the subsystems under test see the deltas they would
/// see in the viewer rather than a synthetic giant step they might handle
/// differently.
const TIMESTEP: Duration = Duration::from_millis(16);

/// How far a normal's length may sit from one.
///
/// Normals are `f32` and produced by a normalize, so the error is a few ULPs —
/// but a normal that has been interpolated, morphed and re-softened accumulates
/// more. 1e-3 is far below anything structural: a wrong normal is wrong by tens
/// of percent (an un-normalized sum, a zero-length degenerate), never by a
/// thousandth.
const NORMAL_EPSILON: f32 = 1.0e-3;

/// How far a skin weight sum may sit from one.
///
/// The R1 bug is a sum of ~0.9 — a tenth off, not a thousandth — so this is
/// nowhere near the failure it guards. It is loose only for the `f32` division
/// `to_bevy_rigged_mesh` renormalizes with.
const WEIGHT_EPSILON: f32 = 1.0e-4;

/// The largest coordinate any fixture's geometry may reach, in metres.
///
/// Every scene here is a few metres across and a Second Life region is 256 m, so
/// nothing legitimate comes near this. It is the catch-all for the failure with
/// no other signature: a vertex transformed by a garbage matrix and flung
/// somewhere absurd, which is otherwise a perfectly finite number that no other
/// check objects to.
const MAX_COORDINATE: f32 = 1_000.0;

/// How far an atlas-sampling UV may leave `[0, 1]` before it counts.
///
/// A UV that lands on exactly 1.0 sometimes comes out of the interpolation a
/// hair past it, and samples the same texel. The failure this guards is a UV at
/// 2.0 or -0.5 — a whole tile away, which in an atlas is a different body part.
const UV_EPSILON: f32 = 1.0e-4;

/// A position snapped onto a matching grid, so two vertices that *should* be the
/// same point compare equal despite having been computed by different float
/// paths.
type VertexKey = (i32, i32, i32);

/// The grid [`vertex_key`] snaps onto: fine enough that two genuinely distinct
/// vertices never collide (a millimetre, against scenes metres across), coarse
/// enough that a mirrored or shared vertex computed two ways lands in one cell.
const VERTEX_QUANTUM: f32 = 1.0e-3;

/// Snap a position onto the [`VERTEX_QUANTUM`] grid.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "`geometry_violations` rejects any coordinate beyond MAX_COORDINATE (1e3 m), \
              so the quantized value is bounded by 1e6 — nowhere near the i32 range"
)]
fn vertex_key(position: Vec3) -> VertexKey {
    let snap = |value: f32| -> i32 { (value / VERTEX_QUANTUM).round() as i32 };
    (snap(position.x), snap(position.y), snap(position.z))
}

/// Build the headless app: assets, transforms, manual time, and the viewer's own
/// time-varying systems.
///
/// **No `RenderPlugin`.** Everything checked here is a property of the geometry in
/// `Assets<Mesh>`, built on the CPU by code that has never heard of an adapter.
/// Standing up a renderer to read a vertex buffer would make the cheap tier depend
/// on a GPU for nothing — and this is the tier that has to run everywhere.
///
/// A free function rather than a builder method: the matrix cell a scene is built
/// at ([`SceneCx`]) is consumed by the *fixture*, at spawn, not by the app — so a
/// `RenderTest` type holding one would have been a wrapper whose `build` never
/// read it.
fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        TransformPlugin,
        TimePlugin,
    ));
    app.init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<SkinnedMeshInverseBindposes>();

    // The deterministic clock. See `TIMESTEP`.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TIMESTEP));

    // The viewer's own time-varying systems, so a dynamic scene is driven by
    // the real thing rather than a test double. `TextureManager` is added at
    // its `Default` — no capability URL, so it never fetches, which is the
    // state the real viewer is in before seed caps arrive
    // (`sl-client-viewer-fetch-defer-until-cap`).
    app.init_resource::<ParticleSim>()
        .init_resource::<TextureManager>()
        .add_systems(Startup, setup_particles)
        .add_systems(Update, drive_particles);

    // `drive_particles` billboards each particle at the camera, so a scene
    // with an emitter needs one to exist. Not a *rendering* camera — there is
    // no renderer — only the pose the billboarding reads.
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::IDENTITY));
    app
}

// ---------------------------------------------------------------------------
// Logs. A scene that renders correctly and complains has still failed.
// ---------------------------------------------------------------------------

/// **Universal.** Rendering a scene must not log at `WARN` or `ERROR`.
///
/// The motivating case is **R26**, and it is worth stating exactly, because it is
/// the whole class: a particle cloud with zero live particles built a zero-vertex
/// mesh, and Bevy's mesh allocator — which skips allocating a zero-size vertex
/// buffer but still tries to copy into it — logged `use-after-free: unallocated
/// key` **every frame**. Nothing looked wrong. No geometry invariant was
/// violated: there was no geometry. The picture was fine. The only symptom the
/// bug ever had was a line in a log nobody reads during a screenshot run, and the
/// fix is the `want_visible` guard in [`crate::particles`].
///
/// That is the shape of it: a subsystem *telling* you it is unhappy while the
/// render carries on plausibly. Every one of those is a real defect, and every
/// one is free to detect — so long as something is listening, which in a normal
/// viewer run nothing is.
///
/// Collected through a **thread-local** subscriber, not a global one, because
/// `cargo test` runs tests in parallel threads inside one process: a global
/// subscriber would be installed once, by whichever test got there first, and
/// every other test's events would land in its bucket.
///
/// # What it cannot see here, and where that is fixed
///
/// Only what the **headless** app logs — which is the viewer's own systems, and
/// not the renderer's. That is a real gap and it is not hypothetical in either
/// direction:
///
/// - R26 itself was logged by Bevy's *mesh allocator*, which lives in the render
///   app. This check would not have caught the bug it is named for.
/// - Bevy's `B0004` (a `Mesh3d` whose ancestor lacks `Visibility`, so it drops
///   out of visibility propagation) is likewise a render-side warning. The
///   registry had exactly that bug; it was found by running the gallery and
///   reading the log **by hand**, which is the loop this file exists to end.
///   Adding `VisibilityPlugin` here does not recover it — the validation hangs
///   off an `On<Insert, InheritedVisibility>` observer that bails when the
///   entity has no `ChildOf` *yet*, which is every single-bundle spawn.
///
/// Both want the fuller plugin set, which means a renderer:
/// `viewer-render-readback-tier` is where this check gets its teeth, and wiring
/// `capture_logs` into it is a few lines.
#[derive(Clone, Default)]
pub(crate) struct LogCapture {
    /// Every `WARN` / `ERROR` event seen, formatted for a failure message.
    events: Arc<Mutex<Vec<String>>>,
}

impl LogCapture {
    /// Everything logged at `WARN` or above since this capture was installed.
    ///
    /// A poisoned lock reports as one synthetic entry rather than panicking: a
    /// harness that takes the suite down because its own bookkeeping failed is
    /// worse than one that reports the failure.
    pub(crate) fn events(&self) -> Vec<String> {
        self.events.lock().map_or_else(
            |_poisoned| vec!["the log capture's lock was poisoned".to_owned()],
            |events| events.clone(),
        )
    }
}

impl<S: Subscriber> Layer<S> for LogCapture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        // `tracing`'s Level ordering runs ERROR < WARN < INFO, so "at least as
        // severe as WARN" is `<=`.
        if *metadata.level() > Level::WARN {
            return;
        }
        let mut message = MessageVisitor(String::new());
        event.record(&mut message);
        if let Ok(mut events) = self.events.lock() {
            events.push(format!(
                "{} {}: {}",
                metadata.level(),
                metadata.target(),
                message.0
            ));
        }
    }
}

/// Pulls an event's fields into a string, so a failure quotes what was logged
/// rather than only that something was.
struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        // `write!` into the buffer rather than `push_str(&format!(..))`: the
        // workspace denies the latter (`format_push_string`), and a `fmt::Result`
        // into a String cannot actually fail, so a failed write is dropped.
        let _written = if field.name() == "message" {
            write!(self.0, "{value:?}")
        } else {
            write!(self.0, "{}={value:?}", field.name())
        };
    }
}

/// Install a [`LogCapture`] on this thread for as long as the guard lives.
pub(crate) fn capture_logs() -> (LogCapture, DefaultGuard) {
    let capture = LogCapture::default();
    let guard = tracing::subscriber::set_default(registry().with(capture.clone()));
    (capture, guard)
}

// ---------------------------------------------------------------------------
// Driving a scene.
// ---------------------------------------------------------------------------

/// Spawn one scene from the registry into a fresh app, and settle it.
///
/// The whole of a matrix cell's setup, mirroring
/// [`spawn_element`](crate::ui_test::spawn_element).
pub(crate) fn spawn_scene(cx: SceneCx, scene: &RenderScene) -> App {
    let mut app = headless_app();
    let spawn = scene.spawn;
    // `Startup`, so the scene is built by the same one-shot path a real object
    // spawn takes, before any frame has run.
    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>,
              mut images: ResMut<Assets<Image>>,
              mut inverse_bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
            let root = commands.spawn(scene_root()).id();
            let mut assets = SceneAssets {
                meshes: &mut meshes,
                materials: &mut materials,
                images: &mut images,
                inverse_bindposes: &mut inverse_bindposes,
            };
            spawn(cx, root, &mut commands, &mut assets);
        },
    );
    app.update();
    app
}

/// Advance the app to `seconds` from its spawn, in [`TIMESTEP`] steps.
///
/// Steps rather than one jump because the subsystems under test *integrate*: a
/// particle simulation handed a single 2-second delta produces a completely
/// different (and wrong) cloud from one stepped 125 times, and it is the stepped
/// one the viewer runs.
pub(crate) fn advance_to(app: &mut App, seconds: f32) {
    let step = TIMESTEP.as_secs_f32();
    let mut remaining = seconds - app.world().resource::<Time>().elapsed_secs();
    while remaining > 0.0 {
        app.update();
        remaining -= step;
    }
}

// ---------------------------------------------------------------------------
// Reading the scene back out of the world.
// ---------------------------------------------------------------------------

/// Everything the checks need about one renderable, lifted out of the world.
///
/// Gathered into a plain struct first, rather than each check re-querying, for
/// two reasons: a check reads `Assets<Mesh>` *and* the entity's components, which
/// is two borrows of the world at once; and a failure has to name the thing,
/// which means resolving [`Name`] once rather than in every check.
#[derive(Debug, Clone)]
pub(crate) struct Geometry {
    /// How a violation names this renderable — its [`Name`], or its entity id.
    pub(crate) name: String,
    /// The **object** this renderable belongs to: its parent's name, or its own
    /// if it has no named parent.
    ///
    /// A prim is an object entity with one child per face (`crate::render_scene`'s
    /// `spawn_prim`), because that is how the viewer builds one. A single face is
    /// a flat quad, so any question about the *solid* — does it enclose a volume
    /// — is meaningful only over the faces grouped back together. This is that
    /// key.
    pub(crate) group: String,
    /// Vertex positions, in the object's local Second Life space.
    pub(crate) positions: Vec<Vec3>,
    /// Per-vertex normals (empty if the mesh carries none).
    pub(crate) normals: Vec<Vec3>,
    /// Per-vertex UV0 (empty if the mesh carries none).
    pub(crate) uvs: Vec<Vec2>,
    /// Triangle-list indices.
    pub(crate) indices: Vec<u32>,
    /// Per-vertex skin joint slots (empty if the mesh is not rigged).
    pub(crate) joint_indices: Vec<[u16; 4]>,
    /// Per-vertex skin weights (empty if the mesh is not rigged).
    pub(crate) joint_weights: Vec<[f32; 4]>,
    /// How many joints the entity's `SkinnedMesh` binds, if it is skinned — the
    /// render list a [`joint_indices`](Self::joint_indices) slot must fall
    /// inside.
    pub(crate) joint_count: Option<usize>,
    /// The size this geometry declared it would be, if it declared one.
    pub(crate) declared_bounds: Option<DeclaredBounds>,
    /// The symmetry this geometry declared, if it declared one.
    pub(crate) symmetry: Option<SymmetricAbout>,
    /// Whether the geometry declared that it samples an atlas, so its UVs must
    /// stay inside the unit square.
    pub(crate) uvs_in_unit_square: bool,
    /// Each texture this renderable samples: the material slot, and whether the
    /// image's sampler wraps on both U and V.
    pub(crate) textures: Vec<TextureSlot>,
    /// Whether the geometry declared its texture may clamp rather than repeat.
    pub(crate) sampler_may_clamp: bool,
}

/// One texture a renderable samples, and the single fact the checks care about.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextureSlot {
    /// Which `StandardMaterial` slot it fills, for a failure message.
    pub(crate) slot: &'static str,
    /// Whether its sampler wraps on **both** U and V.
    pub(crate) repeats: bool,
}

/// Read a `Float32x3` vertex attribute, or an empty list if it is absent or
/// stored as something else.
fn read_vec3(mesh: &Mesh, attribute: MeshVertexAttribute) -> Vec<Vec3> {
    match mesh.attribute(attribute) {
        Some(VertexAttributeValues::Float32x3(values)) => {
            values.iter().copied().map(Vec3::from_array).collect()
        }
        _other => Vec::new(),
    }
}

/// Whether a sampler wraps on **both** U and V.
///
/// `ImageSampler::Default` reports as *not* repeating, and that is the point:
/// Bevy's default address mode is clamp-to-edge, so a texture that never set a
/// descriptor is exactly the bug [`sampler_violations`] exists for.
fn sampler_repeats(sampler: &ImageSampler) -> bool {
    match sampler {
        ImageSampler::Descriptor(descriptor) => {
            descriptor.address_mode_u == ImageAddressMode::Repeat
                && descriptor.address_mode_v == ImageAddressMode::Repeat
        }
        _default => false,
    }
}

/// Every texture a material samples, paired with whether its sampler wraps.
///
/// The slots are named rather than collected anonymously so a failure says
/// *which* map clamps — "some texture on this face" is not an actionable
/// sentence.
fn texture_slots(material: &StandardMaterial, images: &Assets<Image>) -> Vec<TextureSlot> {
    [
        ("base_color_texture", material.base_color_texture.as_ref()),
        ("normal_map_texture", material.normal_map_texture.as_ref()),
        ("emissive_texture", material.emissive_texture.as_ref()),
        (
            "metallic_roughness_texture",
            material.metallic_roughness_texture.as_ref(),
        ),
        ("occlusion_texture", material.occlusion_texture.as_ref()),
    ]
    .into_iter()
    .filter_map(|(slot, handle)| {
        let image = images.get(handle?)?;
        Some(TextureSlot {
            slot,
            repeats: sampler_repeats(&image.sampler),
        })
    })
    .collect()
}

/// The gathered entity-side facts about one renderable, before its mesh handle is
/// resolved. A named struct rather than the ten-wide tuple it started as, which
/// said nothing about what any of it meant.
struct Gathered {
    /// The entity's name.
    name: String,
    /// The object it belongs to. See [`Geometry::group`].
    group: String,
    /// Its mesh.
    mesh: Handle<Mesh>,
    /// Its material, if it has one.
    material: Option<Handle<StandardMaterial>>,
    /// Its declared size.
    declared_bounds: Option<DeclaredBounds>,
    /// Its declared symmetry.
    symmetry: Option<SymmetricAbout>,
    /// Whether it declared it samples an atlas.
    uvs_in_unit_square: bool,
    /// Whether it declared its sampler may clamp.
    sampler_may_clamp: bool,
    /// How many joints it binds, if skinned.
    joint_count: Option<usize>,
}

/// Every renderable in the app's world, with its geometry and declarations.
pub(crate) fn scene_geometry(app: &mut App) -> Vec<Geometry> {
    // Names first, so a renderable's parent can be resolved to the object it
    // belongs to (see `Geometry::group`).
    let mut names = app.world_mut().query::<(Entity, &Name)>();
    let named: HashMap<Entity, String> = names
        .iter(app.world())
        .map(|(entity, name)| (entity, name.to_string()))
        .collect();

    let mut query = app.world_mut().query::<(
        Entity,
        &Mesh3d,
        Option<&MeshMaterial3d<StandardMaterial>>,
        Option<&ChildOf>,
        Option<&DeclaredBounds>,
        Option<&SymmetricAbout>,
        Option<&UvsInUnitSquare>,
        Option<&SamplerMayClamp>,
        Option<&SkinnedMesh>,
    )>();
    let gathered: Vec<Gathered> = query
        .iter(app.world())
        .map(
            |(entity, mesh, material, parent, bounds, symmetry, atlas, clamp, skin)| {
                let name = named
                    .get(&entity)
                    .cloned()
                    .unwrap_or_else(|| format!("{entity}"));
                // The object a face belongs to: its named parent, or itself when
                // it hangs straight off the scene root.
                let group = parent
                    .and_then(|parent| named.get(&parent.parent()))
                    .filter(|parent| parent.as_str() != "scene-root")
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                Gathered {
                    name,
                    group,
                    mesh: mesh.0.clone(),
                    material: material.map(|material| material.0.clone()),
                    declared_bounds: bounds.copied(),
                    symmetry: symmetry.copied(),
                    uvs_in_unit_square: atlas.is_some(),
                    sampler_may_clamp: clamp.is_some(),
                    joint_count: skin.map(|skin| skin.joints.len()),
                }
            },
        )
        .collect();

    let meshes = app.world().resource::<Assets<Mesh>>();
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    let images = app.world().resource::<Assets<Image>>();
    gathered
        .into_iter()
        .filter_map(|entry| {
            let mesh = meshes.get(&entry.mesh)?;
            let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
                Some(VertexAttributeValues::Float32x2(values)) => {
                    values.iter().copied().map(Vec2::from_array).collect()
                }
                _other => Vec::new(),
            };
            let indices = match mesh.indices() {
                Some(Indices::U32(values)) => values.clone(),
                Some(Indices::U16(values)) => values.iter().copied().map(u32::from).collect(),
                None => Vec::new(),
            };
            let joint_indices = match mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX) {
                Some(VertexAttributeValues::Uint16x4(values)) => values.clone(),
                _other => Vec::new(),
            };
            let joint_weights = match mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT) {
                Some(VertexAttributeValues::Float32x4(values)) => values.clone(),
                _other => Vec::new(),
            };
            let textures = entry
                .material
                .as_ref()
                .and_then(|material| materials.get(material))
                .map_or_else(Vec::new, |material| texture_slots(material, images));
            Some(Geometry {
                name: entry.name,
                group: entry.group,
                positions: read_vec3(mesh, Mesh::ATTRIBUTE_POSITION),
                normals: read_vec3(mesh, Mesh::ATTRIBUTE_NORMAL),
                uvs,
                indices,
                joint_indices,
                joint_weights,
                joint_count: entry.joint_count,
                declared_bounds: entry.declared_bounds,
                symmetry: entry.symmetry,
                uvs_in_unit_square: entry.uvs_in_unit_square,
                textures,
                sampler_may_clamp: entry.sampler_may_clamp,
            })
        })
        .collect()
}

/// The axis-aligned bounding box of a position list, as `(min, max)`.
///
/// Built component-wise in plain `f32` rather than with `glam`'s operators, per
/// the convention the rest of this crate follows: the workspace's
/// `arithmetic_side_effects` lint fires on the overloaded operators but not on
/// plain floating-point arithmetic.
fn bounds_of(positions: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let first = positions.first().copied()?;
    let (mut min, mut max) = (first, first);
    for position in positions {
        min = Vec3::new(
            min.x.min(position.x),
            min.y.min(position.y),
            min.z.min(position.z),
        );
        max = Vec3::new(
            max.x.max(position.x),
            max.y.max(position.y),
            max.z.max(position.z),
        );
    }
    Some((min, max))
}

// ---------------------------------------------------------------------------
// The universal tier.
// ---------------------------------------------------------------------------

/// **Universal.** Every geometry invariant, over every renderable in the scene.
///
/// Returns one message per breach so a caller can assert the whole scene at once
/// and see everything wrong with it rather than the first thing.
///
/// **A new check belongs here.** That is what makes it retroactive: the moment it
/// is in this function it runs against every registered scene, at every LOD, at
/// every sample — including the scenes written before the check existed.
pub(crate) fn geometry_violations(geometry: &[Geometry]) -> Vec<String> {
    let mut violations = Vec::new();
    for object in geometry {
        let name = &object.name;

        // Nothing to check, and that is itself the finding: a renderable with no
        // vertices renders as nothing, which is the failure mode every other
        // check passes vacuously through. It is also R26's trigger — an empty
        // mesh Bevy's allocator then logs about every frame.
        if object.positions.is_empty() {
            violations.push(format!("{name}: has no vertex positions at all"));
            continue;
        }

        // Finite, and within a plausible world. See `MAX_COORDINATE`.
        for (index, position) in object.positions.iter().enumerate() {
            if !position.is_finite() {
                violations.push(format!(
                    "{name}: vertex {index} is not finite ({position:?}) — a NaN position \
                     silently deletes every triangle touching it"
                ));
                break;
            }
            if position.abs().max_element() > MAX_COORDINATE {
                violations.push(format!(
                    "{name}: vertex {index} at {position:?} is beyond {MAX_COORDINATE} m from \
                     the object origin"
                ));
                break;
            }
        }

        // Normals: one per vertex, each unit length.
        if !object.normals.is_empty() {
            if object.normals.len() != object.positions.len() {
                violations.push(format!(
                    "{name}: {} normals for {} vertices — the attributes are not parallel",
                    object.normals.len(),
                    object.positions.len()
                ));
            }
            if let Some((index, length)) = object
                .normals
                .iter()
                .enumerate()
                .map(|(index, normal)| (index, normal.length()))
                .find(|&(_index, length)| {
                    !length.is_finite() || (length - 1.0).abs() > NORMAL_EPSILON
                })
            {
                violations.push(format!(
                    "{name}: normal {index} has length {length} — a non-unit normal shades as \
                     though the surface were lit from a different angle"
                ));
            }
        }

        // UVs: one per vertex, finite.
        if !object.uvs.is_empty() && object.uvs.len() != object.positions.len() {
            violations.push(format!(
                "{name}: {} UVs for {} vertices — the attributes are not parallel",
                object.uvs.len(),
                object.positions.len()
            ));
        }
        if let Some((index, uv)) = object
            .uvs
            .iter()
            .enumerate()
            .find(|(_index, uv)| !uv.is_finite())
        {
            violations.push(format!("{name}: UV {index} is not finite ({uv:?})"));
        }

        // Indices: whole triangles, all in range.
        if object.indices.is_empty() {
            violations.push(format!(
                "{name}: has vertices but no indices — it draws no triangles"
            ));
        } else if object.indices.len() % 3 != 0 {
            violations.push(format!(
                "{name}: {} indices is not a whole number of triangles",
                object.indices.len()
            ));
        }
        let vertex_count = u32::try_from(object.positions.len()).unwrap_or(u32::MAX);
        if let Some(bad) = object.indices.iter().find(|&&index| index >= vertex_count) {
            violations.push(format!(
                "{name}: an index addresses vertex {bad} of {vertex_count} — out of range"
            ));
        }

        // At least one triangle with area. All-degenerate geometry passes every
        // check above and draws nothing.
        if !object.indices.is_empty() && triangle_areas(object).all(|area| area <= f32::EPSILON) {
            violations.push(format!(
                "{name}: every triangle is degenerate (zero area) — it renders as nothing"
            ));
        }

        violations.extend(skin_violations(object));
        violations.extend(unskinned_violations(object));
        violations.extend(sampler_violations(object));
    }
    violations
}

/// Each triangle's area, in square metres.
fn triangle_areas(object: &Geometry) -> impl Iterator<Item = f32> + '_ {
    object.indices.chunks_exact(3).map(|triangle| {
        let corners: Vec<Vec3> = triangle
            .iter()
            .filter_map(|&index| object.positions.get(usize::try_from(index).unwrap_or(0)))
            .copied()
            .collect();
        match corners.as_slice() {
            [a, b, c] => {
                let ab = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
                let ac = Vec3::new(c.x - a.x, c.y - a.y, c.z - a.z);
                ab.cross(ac).length() * 0.5
            }
            _other => 0.0,
        }
    })
}

/// **Universal.** The two skinning invariants that have already shipped bugs.
///
/// Both are pure counting, and both were diagnosed the expensive way first:
///
/// - **Weights sum to one.** Second Life quantizes each influence independently
///   and drops any past the fourth, so a wire rig's weights sum to *less* than
///   one — and Bevy's skinning shader does not renormalize (the reference
///   viewer's `getPerVertexSkinMatrix` does). The shortfall blends in that
///   fraction of the zero matrix, dragging the vertex toward the mesh origin:
///   the downward "streak toward the feet" of R1
///   (`sl-client-rigged-mesh-skinning`).
/// - **Joints inside the render list.** A vertex bound to a joint the render
///   list does not contain reads a garbage matrix — the R13 armpit spike, from
///   base-mesh weights naming Bento / extended ancestors the skeleton skips.
fn skin_violations(object: &Geometry) -> Vec<String> {
    let mut violations = Vec::new();
    let name = &object.name;
    if object.joint_weights.is_empty() {
        return violations;
    }
    if object.joint_weights.len() != object.positions.len() {
        violations.push(format!(
            "{name}: {} skin weights for {} vertices — the attributes are not parallel",
            object.joint_weights.len(),
            object.positions.len()
        ));
    }
    if let Some((index, sum)) = object
        .joint_weights
        .iter()
        .enumerate()
        .map(|(index, weights)| (index, weights.iter().sum::<f32>()))
        .find(|&(_index, sum)| (sum - 1.0).abs() > WEIGHT_EPSILON)
    {
        violations.push(format!(
            "{name}: vertex {index} skin weights sum to {sum}, not 1 — Bevy does not \
             renormalize, so this vertex is dragged {} of the way to the mesh origin (R1)",
            1.0 - sum
        ));
    }
    if let Some(joint_count) = object.joint_count {
        let limit = u16::try_from(joint_count).unwrap_or(u16::MAX);
        let offending = object
            .joint_indices
            .iter()
            .enumerate()
            .find_map(|(index, joints)| {
                let weights = object.joint_weights.get(index);
                joints
                    .iter()
                    .enumerate()
                    .find(|&(slot, &joint)| {
                        // A slot with zero weight is not read by the shader, so
                        // its joint index is free to be anything —
                        // `to_bevy_rigged_mesh` fills unused slots with 0. Only a
                        // *weighted* slot has to name a real joint.
                        let weight = weights.and_then(|weights| weights.get(slot)).copied();
                        weight.is_some_and(|weight| weight > 0.0) && joint >= limit
                    })
                    .map(|(slot, &joint)| (index, slot, joint))
            });
        if let Some((index, slot, joint)) = offending {
            violations.push(format!(
                "{name}: vertex {index} slot {slot} is weighted onto joint {joint}, but the \
                 render list has only {joint_count} joints — this vertex reads a garbage \
                 matrix (R13)"
            ));
        }
    }
    violations
}

/// **Universal.** A mesh's skin attributes and its entity's `SkinnedMesh` must
/// **agree** — both present, or neither.
///
/// This one is not a wrong picture. It is a **hard crash**, in both directions,
/// and both have now happened here.
///
/// Bevy specializes its render pipeline from the mesh's *vertex attributes*,
/// while the bind group comes from the *entity's components*. Nothing makes the
/// two agree, so a mismatch either way is a wgpu validation error and Bevy quits
/// the application:
///
/// - **Attributes, no `SkinnedMesh`** — the skinned pipeline is handed a
///   `model_only_mesh_bind_group`:
///
///   ```text
///   The BindGroupLayout with 'mesh_layout' label of current set BindGroup with
///   'model_only_mesh_bind_group' label ... is not compatible with the
///   corresponding BindGroupLayout with 'skinned_mesh_layout' label of
///   RenderPipeline with 'pbr_opaque_mesh_pipeline' label
///   ```
///
/// - **`SkinnedMesh`, no attributes** — the mismatch inverts, and so does the
///   message: a `skinned_mesh_bind_group` handed to a `mesh_layout` pipeline.
///
/// The registry shipped **both**, in sequence, and neither was found by a check:
/// the first by watching the gallery die on the avatar scene, the second by
/// pointing that scene at the real Linden body, whose eye parts carry no weights
/// where the committed mini fixture does. The second is the more instructive:
/// the check existed by then, and only covered one direction, so it watched the
/// crash happen. Both are decidable from the mesh and the entity alone.
fn unskinned_violations(object: &Geometry) -> Vec<String> {
    let skinned_mesh = object.joint_count.is_some();
    let skin_attributes = !object.joint_weights.is_empty();
    if skinned_mesh == skin_attributes {
        return Vec::new();
    }
    vec![if skin_attributes {
        format!(
            "{}: its mesh carries skin attributes ({} weighted vertices) but the entity has no \
             `SkinnedMesh` — Bevy specializes the skinned pipeline from the attributes and then \
             hands it a model-only bind group, which is a wgpu validation error that kills the \
             process, not a rendering artifact",
            object.name,
            object.joint_weights.len()
        )
    } else {
        format!(
            "{}: the entity has a `SkinnedMesh` but its mesh carries no skin attributes — Bevy \
             specializes the model-only pipeline from the attributes and then hands it a skinned \
             bind group, which is a wgpu validation error that kills the process, not a \
             rendering artifact",
            object.name
        )
    }]
}

/// **Universal, with a declared exception.** Every texture must sample with a
/// repeating address mode.
///
/// Second Life faces tile: the reference viewer samples with `GL_REPEAT` and sets
/// clamp only for the rare texture-entry clamp flag. **Bevy's default is the
/// opposite** — clamp-to-edge — so a texture path that never sets a sampler
/// descriptor is silently wrong, and wrong in a way that looks like a *texture*
/// bug rather than a sampler one: the face renders its edge texel smeared flat
/// across itself. That is R22h (the "white torso" over a correct bake) and the
/// face streaks `sl-client-prim-texture-debugging` records; both cost real time
/// precisely because the geometry, the UVs and the decode were all correct.
///
/// Four separate paths set the mode today, and this check exists for the fifth.
/// See [`SamplerMayClamp`] for the exception.
fn sampler_violations(object: &Geometry) -> Vec<String> {
    if object.sampler_may_clamp {
        return Vec::new();
    }
    object
        .textures
        .iter()
        .filter(|texture| !texture.repeats)
        .map(|texture| {
            format!(
                "{}: the `{}` texture does not sample with a repeating address mode — Bevy's \
                 default clamps, so every UV outside the unit square smears this texture's edge \
                 texel across the face (R22h) rather than tiling",
                object.name, texture.slot
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The declared tier.
// ---------------------------------------------------------------------------

/// **Declared.** Every scene-declared intent, held to.
///
/// See [`DeclaredBounds`] / [`SymmetricAbout`] for why this tier exists at all:
/// nothing in a vertex buffer says how big an object was *meant* to be, or that
/// it was *meant* to be symmetric, so the scene says and this holds it to the
/// declaration in every cell.
pub(crate) fn declared_violations(geometry: &[Geometry]) -> Vec<String> {
    let mut violations = Vec::new();

    // Declared bounds sit on the *object*, whose extent is the union of its faces
    // — a box's declared size is not any one face's size.
    let mut groups: Vec<&str> = geometry
        .iter()
        .map(|object| object.group.as_str())
        .collect();
    groups.sort_unstable();
    groups.dedup();
    for group in groups {
        let faces: Vec<&Geometry> = geometry
            .iter()
            .filter(|object| object.group == group)
            .collect();
        let Some(declared) = faces.iter().find_map(|face| face.declared_bounds) else {
            continue;
        };
        let positions: Vec<Vec3> = faces
            .iter()
            .flat_map(|face| face.positions.iter().copied())
            .collect();
        let Some((min, max)) = bounds_of(&positions) else {
            continue;
        };
        for (axis, actual, expected) in [
            ("x", (max.x - min.x) * 0.5, declared.half_extents.x),
            ("y", (max.y - min.y) * 0.5, declared.half_extents.y),
            ("z", (max.z - min.z) * 0.5, declared.half_extents.z),
        ] {
            if (actual - expected).abs() > declared.tolerance {
                violations.push(format!(
                    "{group}: declared half-extent {expected} on {axis}, measured {actual} \
                     (tolerance {})",
                    declared.tolerance
                ));
            }
        }
    }

    for object in geometry {
        if let Some(symmetry) = object.symmetry {
            violations.extend(symmetry_violations(object, symmetry));
        }
        if object.uvs_in_unit_square
            && let Some((index, uv)) = object.uvs.iter().enumerate().find(|(_index, uv)| {
                uv.x < -UV_EPSILON
                    || uv.y < -UV_EPSILON
                    || uv.x > 1.0 + UV_EPSILON
                    || uv.y > 1.0 + UV_EPSILON
            })
        {
            violations.push(format!(
                "{}: UV {index} is {uv:?}, outside the unit square, but this geometry \
                     declared it samples an atlas — so this vertex samples a different region \
                     of the atlas rather than tiling",
                object.name
            ));
        }
    }
    violations
}

/// `position` with `axis` negated — its mirror image.
///
/// Lives here rather than on [`SymmetryAxis`] because it is a *check* helper: the
/// registry declares which planes a geometry mirrors about, and mirroring a point
/// is this tier's business.
const fn mirror(axis: SymmetryAxis, position: Vec3) -> Vec3 {
    match axis {
        SymmetryAxis::X => Vec3::new(-position.x, position.y, position.z),
        SymmetryAxis::Y => Vec3::new(position.x, -position.y, position.z),
        SymmetryAxis::Z => Vec3::new(position.x, position.y, -position.z),
    }
}

/// Check one geometry against its declared symmetry, on every axis it declared.
///
/// Every vertex must have a mirrored counterpart. Matched by **position**, not by
/// index, and quantized: a tessellator has no obligation to emit mirrored
/// vertices in a mirrored order, and comparing indices would fail on geometry
/// that is perfectly symmetric.
fn symmetry_violations(object: &Geometry, symmetry: SymmetricAbout) -> Vec<String> {
    let present: HashSet<VertexKey> = object.positions.iter().copied().map(vertex_key).collect();
    symmetry
        .axes
        .iter()
        .filter_map(|&axis| {
            let missing = object
                .positions
                .iter()
                .copied()
                .find(|&position| !present.contains(&vertex_key(mirror(axis, position))))?;
            Some(format!(
                "{}: vertex {missing:?} has no counterpart mirrored about {axis:?}, but the \
                 scene declared it symmetric — {}",
                object.name, symmetry.reason
            ))
        })
        .collect()
}

/// Every check, over the whole scene, as one list.
///
/// The shape every matrix cell uses: assert the result is empty and print it on
/// failure, so one run reports everything wrong with the scene rather than the
/// first thing.
pub(crate) fn scene_violations(geometry: &[Geometry]) -> Vec<String> {
    let mut violations = geometry_violations(geometry);
    violations.extend(declared_violations(geometry));
    violations
}

#[cfg(test)]
mod tests {
    use super::{
        Geometry, TestError, advance_to, capture_logs, geometry_violations, scene_geometry,
        scene_violations, spawn_scene,
    };
    use crate::render_scene::{SCENES, SceneCx, rigged_strip};
    use bevy::prelude::*;
    use pretty_assertions::assert_ne;
    use sl_client_bevy::PrimLod;

    // -----------------------------------------------------------------------
    // The harness has to have teeth. These tests are about the *checks*, not
    // about the geometry: a suite whose checks cannot fail is a suite that
    // reports success because it looked at nothing.
    // -----------------------------------------------------------------------

    /// One valid triangle, then broken one way, run through the universal tier.
    fn broken(mutate: impl FnOnce(&mut Geometry)) -> Vec<String> {
        let mut object = Geometry {
            name: "fixture".to_owned(),
            group: "fixture".to_owned(),
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            normals: vec![Vec3::Z, Vec3::Z, Vec3::Z],
            uvs: vec![Vec2::ZERO, Vec2::X, Vec2::Y],
            indices: vec![0, 1, 2],
            joint_indices: Vec::new(),
            joint_weights: Vec::new(),
            joint_count: None,
            declared_bounds: None,
            symmetry: None,
            uvs_in_unit_square: false,
            textures: Vec::new(),
            sampler_may_clamp: false,
        };
        mutate(&mut object);
        geometry_violations(&[object])
    }

    /// The control: the unbroken fixture must be clean.
    ///
    /// The half that makes every test below meaningful. A check that fires on
    /// broken geometry proves nothing on its own — it has to also *not* fire on
    /// the good case, or it is simply a check that always fires.
    #[test]
    fn a_valid_triangle_is_clean() {
        assert!(
            broken(|_unchanged| {}).is_empty(),
            "the unbroken fixture must pass every universal check"
        );
    }

    /// A NaN position is reported.
    #[test]
    fn a_nan_position_is_reported() {
        assert!(
            !broken(|object| {
                object.positions = vec![Vec3::new(f32::NAN, 0.0, 0.0), Vec3::X, Vec3::Y];
            })
            .is_empty(),
            "a NaN position silently deletes every triangle touching it and must be reported"
        );
    }

    /// A non-unit normal is reported.
    #[test]
    fn a_non_unit_normal_is_reported() {
        assert!(
            !broken(|object| {
                object.normals = vec![Vec3::new(0.0, 0.0, 0.5), Vec3::Z, Vec3::Z];
            })
            .is_empty(),
            "a half-length normal shades as though lit from elsewhere and must be reported"
        );
    }

    /// An out-of-range index is reported.
    #[test]
    fn an_out_of_range_index_is_reported() {
        assert!(
            !broken(|object| {
                object.indices = vec![0, 1, 7];
            })
            .is_empty(),
            "an index past the vertex array must be reported"
        );
    }

    /// All-degenerate geometry is reported.
    #[test]
    fn all_degenerate_triangles_are_reported() {
        assert!(
            !broken(|object| {
                object.positions = vec![Vec3::ZERO, Vec3::ZERO, Vec3::ZERO];
            })
            .is_empty(),
            "geometry whose every triangle has zero area renders as nothing and must be reported"
        );
    }

    /// An empty mesh is reported — the R26 trigger.
    #[test]
    fn an_empty_mesh_is_reported() {
        assert!(
            !broken(|object| {
                object.positions = Vec::new();
            })
            .is_empty(),
            "a zero-vertex mesh is what made Bevy's allocator log every frame (R26) and must be \
             reported"
        );
    }

    /// **The R1 check has teeth.** Skin weights that do not sum to one are
    /// reported.
    #[test]
    fn unrenormalized_skin_weights_are_reported() {
        assert!(
            !broken(|object| {
                object.joint_weights = vec![[0.6, 0.3, 0.0, 0.0]; 3];
                object.joint_indices = vec![[0, 1, 0, 0]; 3];
            })
            .is_empty(),
            "weights summing to 0.9 are the R1 distortion and must be reported"
        );
    }

    /// **The R13 check has teeth.** A vertex weighted onto a joint outside the
    /// render list is reported.
    #[test]
    fn a_weight_onto_a_joint_outside_the_render_list_is_reported() {
        assert!(
            !broken(|object| {
                object.joint_weights = vec![[1.0, 0.0, 0.0, 0.0]; 3];
                object.joint_indices = vec![[9, 0, 0, 0]; 3];
                object.joint_count = Some(2);
            })
            .is_empty(),
            "a weighted slot naming joint 9 of a 2-joint render list reads a garbage matrix \
             (R13) and must be reported"
        );
    }

    /// **The crash check has teeth.** A skinned mesh with no `SkinnedMesh` is
    /// reported.
    ///
    /// Not a wrong picture — wgpu rejects the draw and Bevy quits. The registry
    /// shipped this bug (the avatar scene, spawned without a skeleton) and it was
    /// found by watching the gallery die.
    #[test]
    fn a_skinned_mesh_without_a_skeleton_is_reported() {
        assert!(
            !broken(|object| {
                object.joint_weights = vec![[1.0, 0.0, 0.0, 0.0]; 3];
                object.joint_indices = vec![[0, 0, 0, 0]; 3];
                // The bug: skin attributes, no `SkinnedMesh`.
                object.joint_count = None;
            })
            .is_empty(),
            "a mesh with skin attributes and no `SkinnedMesh` is a wgpu validation error that \
             kills the process and must be reported"
        );
    }

    /// **The other direction has teeth too.** A `SkinnedMesh` on an unskinned mesh
    /// is reported.
    ///
    /// The direction the check missed when it was first written — so the real
    /// Linden body (whose eye parts carry no weights) crashed the gallery while a
    /// green suite watched.
    #[test]
    fn a_skeleton_on_an_unskinned_mesh_is_reported() {
        assert!(
            !broken(|object| {
                // The bug: a `SkinnedMesh`, no skin attributes.
                object.joint_count = Some(2);
            })
            .is_empty(),
            "a `SkinnedMesh` on a mesh with no skin attributes hands the model-only pipeline a \
             skinned bind group — a wgpu validation error that kills the process — and must be \
             reported"
        );
    }

    /// The same mesh, on a skeleton, is clean — the other half of the pair.
    #[test]
    fn a_skinned_mesh_with_a_skeleton_is_clean() {
        assert!(
            broken(|object| {
                object.joint_weights = vec![[1.0, 0.0, 0.0, 0.0]; 3];
                object.joint_indices = vec![[0, 0, 0, 0]; 3];
                object.joint_count = Some(2);
            })
            .is_empty(),
            "a properly skinned mesh must not be reported, or the check is just noise"
        );
    }

    /// **The R22h check has teeth.** A clamping texture is reported.
    #[test]
    fn a_clamping_texture_is_reported() {
        assert!(
            !broken(|object| {
                object.textures = vec![super::TextureSlot {
                    slot: "base_color_texture",
                    repeats: false,
                }];
            })
            .is_empty(),
            "Bevy's default sampler clamps where Second Life repeats (R22h) and must be reported"
        );
    }

    /// The rigged fixture really is malformed the way the wire is.
    ///
    /// The other half of the R1 story, and what keeps the matrix honest.
    /// `rigged-mesh` passes the suite only because `to_bevy_rigged_mesh`
    /// renormalizes — so this asserts the fixture *arrives* un-normalized. If
    /// somebody "tidies" it to sum to one, the scene keeps passing while testing
    /// nothing, and this is what notices.
    #[test]
    fn the_rigged_fixture_arrives_unnormalized_like_the_wire() {
        let (submesh, _skin) = rigged_strip();
        let sums: Vec<f32> = submesh
            .weights
            .unwrap_or_default()
            .iter()
            .map(|vertex| vertex.influences.iter().map(|&(_joint, w)| w).sum())
            .collect();
        assert!(
            !sums.is_empty() && sums.iter().all(|&sum| (sum - 1.0).abs() > 0.05),
            "the rigged fixture must arrive with weights that do NOT sum to one, or the matrix \
             is not exercising the renormalization at all — got {sums:?}"
        );
    }

    // -----------------------------------------------------------------------
    // The matrix. Every registered scene, at every LOD.
    // -----------------------------------------------------------------------

    /// Every scene must build geometry at all.
    ///
    /// The guard against the quiet failure this whole file is exposed to: if a
    /// fixture never spawned, every check passes by looking at nothing.
    #[test]
    fn every_scene_actually_renders() {
        for scene in SCENES {
            let mut app = spawn_scene(SceneCx::new(), scene);
            // A dynamic scene's renderable may not exist until time has passed —
            // a particle cloud does not exist at t=0 — so run its timeline out.
            if let Some(&last) = scene.timeline.samples.last() {
                advance_to(&mut app, last);
            }
            let vertices: usize = scene_geometry(&mut app)
                .iter()
                .map(|object| object.positions.len())
                .sum();
            assert!(
                vertices > 0,
                "scene `{}` built no geometry with any vertices — the fixture did not spawn, and \
                 every other check is passing vacuously",
                scene.id
            );
        }
    }

    /// **Every scene × every LOD.** The sweep no human walks.
    ///
    /// A prim is tessellated on the client at a detail chosen by on-screen size,
    /// so every prim has four geometries and the coarse ones are the ones nobody
    /// looks at. A new scene inherits this by being registered; a new check by
    /// being in `scene_violations`.
    #[test]
    fn every_scene_survives_every_lod() {
        let mut failures = Vec::new();
        for scene in SCENES {
            for lod in PrimLod::ALL {
                let mut app = spawn_scene(SceneCx { lod }, scene);
                if let Some(&last) = scene.timeline.samples.last() {
                    advance_to(&mut app, last);
                }
                let violations = scene_violations(&scene_geometry(&mut app));
                if !violations.is_empty() {
                    failures.push(format!("scene `{}` at {lod:?}: {violations:#?}", scene.id));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every scene, at every sample of its timeline.**
    ///
    /// Not only at rest: a simulation's geometry is rebuilt every frame, so a
    /// particle cloud that is valid at 0.5 s and has a NaN at 2 s is a bug the
    /// resting check never sees.
    #[test]
    fn every_scene_survives_every_sample_of_its_timeline() {
        let mut failures = Vec::new();
        for scene in SCENES {
            let mut app = spawn_scene(SceneCx::new(), scene);
            for &sample in scene.timeline.samples {
                advance_to(&mut app, sample);
                let violations = scene_violations(&scene_geometry(&mut app));
                if !violations.is_empty() {
                    failures.push(format!(
                        "scene `{}` at t={sample}s: {violations:#?}",
                        scene.id
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **No scene may log a warning or an error.**
    ///
    /// See [`LogCapture`](super::LogCapture): R26's only symptom was a log line,
    /// on a scene whose picture was fine.
    #[test]
    fn no_scene_logs_a_warning_or_an_error() {
        let mut failures = Vec::new();
        for scene in SCENES {
            let (logs, _guard) = capture_logs();
            let mut app = spawn_scene(SceneCx::new(), scene);
            for &sample in scene.timeline.samples {
                advance_to(&mut app, sample);
            }
            let events = logs.events();
            if !events.is_empty() {
                failures.push(format!("scene `{}` logged: {events:#?}", scene.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **A scene that declares a timeline must actually change.**
    ///
    /// The check the time axis exists for. A multi-sample scene is *declaring*
    /// that something happens over time; if its geometry is identical at the
    /// first and last sample, the thing it exists to exercise did not run — a
    /// dead emitter, a simulation never stepped, an animation that never played.
    /// That failure is invisible to every other check in this file, all of which
    /// would report a perfectly valid empty scene.
    #[test]
    fn every_dynamic_scene_actually_changes_over_its_timeline() -> Result<(), TestError> {
        for scene in SCENES.iter().filter(|scene| scene.timeline.is_dynamic()) {
            let mut app = spawn_scene(SceneCx::new(), scene);
            let first = *scene
                .timeline
                .samples
                .first()
                .ok_or("a dynamic timeline with no samples")?;
            let last = *scene
                .timeline
                .samples
                .last()
                .ok_or("a dynamic timeline with no samples")?;
            advance_to(&mut app, first);
            let before = digest(&scene_geometry(&mut app));
            advance_to(&mut app, last);
            let after = digest(&scene_geometry(&mut app));
            assert_ne!(
                before, after,
                "scene `{}` declares a timeline from {first}s to {last}s but its geometry is \
                 identical at both — nothing it exists to exercise actually ran",
                scene.id
            );
        }
        Ok(())
    }

    /// A coarse summary of a scene's geometry, for "did anything change".
    ///
    /// Deliberately not a hash of the vertex data: the question is whether the
    /// scene *moved*, and a summary a reader can eyeball in a failure message
    /// answers it better than a hex digest nobody can interpret.
    fn digest(geometry: &[Geometry]) -> Vec<String> {
        let mut lines: Vec<String> = geometry
            .iter()
            .map(|object| {
                let centroid = object.positions.iter().fold(Vec3::ZERO, |sum, position| {
                    Vec3::new(sum.x + position.x, sum.y + position.y, sum.z + position.z)
                });
                format!(
                    "{}: {} verts, {} tris, centroid {centroid:.3?}",
                    object.name,
                    object.positions.len(),
                    object.indices.len() / 3,
                )
            })
            .collect();
        lines.sort();
        lines
    }

    /// The registry is actually being swept.
    ///
    /// Cheap insurance against the way a matrix rots: someone adds a scene,
    /// nothing references it, and the suite goes on being green about a smaller
    /// world than it claims.
    #[test]
    fn the_matrix_covers_the_whole_registry() {
        assert!(!SCENES.is_empty(), "no scenes to sweep");
        assert!(
            SCENES.iter().any(|scene| scene.timeline.is_dynamic()),
            "no dynamic scene is registered, so the time axis is untested"
        );
    }
}
