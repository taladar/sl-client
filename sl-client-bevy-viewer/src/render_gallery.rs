//! The **render gallery** (`viewer-render-test-harness`): every registered scene,
//! rendered on its own, with **no login, no grid and no world**.
//!
//! ```console
//! sl-client-bevy-viewer-scenes
//! ```
//!
//! # What it is for, now that the matrix is not its job
//!
//! The gallery answers the one question a machine cannot: **does this look
//! right**. Whether geometry is *valid* — finite, unit-normalled, in-range,
//! correctly weighted, correctly sampled — is machine-checkable, and
//! `crate::render_test` checks it across every scene at every LOD at every
//! sample of its timeline. Walking that grid by eye is exactly the combinatorial
//! explosion the harness exists to end, so the gallery does not try.
//!
//! What is left for a human is real and cannot be automated: is the shape right,
//! is the shading plausible, does the light fall where it should, is the twist
//! going the right way. And the discovery loop — a person notices something wrong
//! here, and the fix is a **check** in `crate::render_test`, which from then on
//! runs against every scene forever. The gallery is where bugs are *found*; the
//! harness is where they stay found.
//!
//! # Why it can exist at all
//!
//! Because of the registry's one rule: geometry is **constructible without a
//! session** (`crate::render_scene`). Every scene here is built from
//! parameters, computed maps and synthesized submeshes, through the viewer's real
//! converters. No OpenSim, no login, no OAR import, no looking the regenerated
//! UUID up in `bin/OpenSim.db` first, no flying a camera across a region. It is
//! the same geometry the viewer builds, minus the grid that usually delivers it.
//!
//! That is the loop this replaces, and it is worth being concrete about the
//! difference: seeing a change to prim tessellation used to be *minutes* and a
//! human, half of it grid administration. It is now a keypress.
//!
//! # Driving it
//!
//! | Key | What it does |
//! | --- | --- |
//! | `N` / `P` | next / previous scene |
//! | `L` | cycle the client-tessellation LOD (the harness's matrix axis) |
//! | `R` | restart the current scene's timeline (re-run a particle burst) |
//! | `Space` | pause / resume the scene's clock (the camera keeps moving) |
//! | arrows | orbit the camera around the scene |
//! | `+` / `-` | dolly in / out |
//! | `Escape` | quit |
//!
//! `--scene <id>` opens straight on one, so a failing check's own words paste
//! into a command instead of being hunted for with `N`.
//!
//! `L` is the matrix, hand-drivable. It is here not to *check* the cells — the
//! harness does that — but so a person can look at the cell a failing check just
//! named. The arrows matter more than they look: every scene's declared opening
//! pose has been wrong at least once (a tree framed at its roots, an avatar
//! edge-on and cut off at the knee), and half of judging geometry is walking
//! around it — a hole is invisible from the front.

use bevy::camera::{Exposure, Hdr};
use bevy::light::DirectionalLightShadowMap;
use bevy::log::LogPlugin;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::window::PresentMode;
use clap::Parser;

use sl_client_bevy::PrimLod;
use tracing::{error, info};

use crate::camera::FlyCamera;
use crate::particles::{ParticleSim, drive_particles, setup_particles};
use crate::probes::ReflectionProbePlugin;
use crate::render_scene::{
    DeclaredBounds, RenderScene, SCENES, SamplerMayClamp, SceneAssets, SceneCx, SceneLighting,
    SymmetricAbout, UvsInUnitSquare, scene_root, scene_root_transform,
};
use crate::textures::TextureManager;

/// The command-line options for the render gallery.
///
/// One option, and it earns its place: the registry is a list a human steps
/// through with `N`, and when a check fails it names **one** scene. Pressing `N`
/// eight times to reach it is exactly the friction this harness exists to
/// remove, so a failure's own words paste straight into a command:
///
/// ```console
/// sl-client-bevy-viewer-scenes --scene avatar-base-part
/// ```
#[derive(Parser, Debug)]
#[clap(
    name = "sl-client-bevy-viewer-scenes",
    // `long_about = None` so `--help` shows this one line rather than the
    // rationale above: clap promotes a struct's whole doc comment otherwise.
    about = "The sl-client render gallery: every registered scene, with no login and no world",
    long_about = None,
    author = clap::crate_authors!(),
    version = clap::crate_version!(),
    disable_version_flag = true,
)]
struct GalleryArgs {
    /// The scene to open on, by id (e.g. `prim-box`). Defaults to the first.
    #[clap(long, value_name = "ID")]
    scene: Option<String>,
}

/// The key that steps to the next scene.
const NEXT_KEY: KeyCode = KeyCode::KeyN;

/// The key that steps to the previous scene.
const PREVIOUS_KEY: KeyCode = KeyCode::KeyP;

/// The key that cycles the tessellation LOD.
const LOD_KEY: KeyCode = KeyCode::KeyL;

/// The key that restarts the current scene.
const RESTART_KEY: KeyCode = KeyCode::KeyR;

/// The key that pauses and resumes time.
const PAUSE_KEY: KeyCode = KeyCode::Space;

/// The gallery's background: a mid grey rather than black.
///
/// Deliberately not black, and not white. Half of what a human is here to judge
/// is the **silhouette**, and an unlit back face against black is invisible while
/// a bright edge against white is. A mid grey loses neither end.
const BACKGROUND: Color = Color::srgb(0.22, 0.24, 0.28);

/// The chrome's font size, in logical pixels.
const CHROME_FONT_SIZE: f32 = 14.0;

/// How fast the arrow keys swing the orbit, in radians per second.
const ORBIT_RATE: f32 = 1.6;

/// How fast `+` / `-` dolly the camera, as a fraction of the distance per second
/// — proportional, so a step feels the same on a 1 m prim and a 36 m tree.
const DOLLY_RATE: f32 = 1.5;

/// The closest the camera may orbit, in metres.
const MIN_ORBIT_DISTANCE: f32 = 0.2;

/// How far the orbit may tilt from the horizontal, in radians. Short of the pole,
/// where the `looking_at` up vector degenerates and the view rolls wildly.
const MAX_PITCH: f32 = 1.5;

/// The stage rig's ambient brightness, in nits — enough that an unlit face is
/// still a shape rather than a silhouette.
const STAGE_AMBIENT: f32 = 200.0;

/// The ambient a [`SceneLighting::Own`] scene gets: nearly none.
///
/// What those scenes are *for* is where their own light falls, and ambient fill is
/// exactly what erases the difference between lit and unlit. Not zero, so an
/// unlit face is still faintly a shape rather than a hole.
const OWN_LIGHTING_AMBIENT: f32 = 12.0;

/// The colour of the scene's id.
const HEADER_COLOR: Color = Color::srgb(0.95, 0.85, 0.45);

/// The colour of the scene's summary and the key hints.
const CHROME_COLOR: Color = Color::srgb(0.70, 0.76, 0.85);

/// Which cell of the matrix the gallery is showing.
///
/// The same axes `crate::render_test` sweeps, exposed as one resource so a
/// person can steer to the cell a failing check named and look at it.
#[derive(Resource, Debug, Clone, Copy)]
struct GalleryCell {
    /// Which registered scene is shown, as an index into [`SCENES`].
    scene: usize,
    /// The tessellation detail the scene is built at.
    lod: PrimLod,
}

impl Default for GalleryCell {
    /// The resting cell is the registry's own resting context rather than a
    /// second opinion about it: the gallery opens on the same thing a test's
    /// baseline cell shows.
    fn default() -> Self {
        Self {
            scene: 0,
            lod: SceneCx::new().lod,
        }
    }
}

impl GalleryCell {
    /// This cell as the context a scene is spawned with.
    const fn cx(self) -> SceneCx {
        SceneCx { lod: self.lod }
    }

    /// The scene this cell names, if the index is in range.
    fn scene(self) -> Option<&'static RenderScene> {
        SCENES.get(self.scene)
    }

    /// The next LOD in the cycle, wrapping.
    fn next_lod(self) -> PrimLod {
        let index = PrimLod::ALL
            .iter()
            .position(|lod| *lod == self.lod)
            .map_or(0, |index| index.saturating_add(1));
        PrimLod::ALL
            .get(index)
            .copied()
            .unwrap_or(PrimLod::COARSEST)
    }
}

/// Where the camera is, as an orbit about the scene.
///
/// Every scene declares a camera pose ([`SceneCamera`]), and every one of those
/// poses has so far been *wrong* on first writing: the tree framed at its roots
/// with the canopy off screen, the avatar edge-on and cut off at the knee. That
/// is not carelessness so much as the nature of a fixed pose — a good one depends
/// on the geometry's extent, which the author does not know until they look.
///
/// So the declared pose is the **opening** view rather than the only one, and a
/// human can move. The scene's pose is decomposed into this orbit on spawn, the
/// keys move it, and the camera transform is recomputed from it — which keeps the
/// per-scene framing meaningful while making a bad one recoverable without an
/// edit-and-rebuild cycle.
#[derive(Resource, Debug, Clone, Copy)]
struct Orbit {
    /// What the camera looks at, in Bevy world space.
    target: Vec3,
    /// How far back it sits, in metres.
    distance: f32,
    /// Rotation about the Bevy up (`+Y`) axis, in radians.
    yaw: f32,
    /// Elevation above the horizontal, in radians, clamped away from the poles.
    pitch: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 4.0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

impl Orbit {
    /// Decompose a camera position and target into an orbit about that target.
    ///
    /// Built component-wise in plain `f32` rather than with `glam`'s operators,
    /// per the convention the rest of this crate follows: the workspace's
    /// `arithmetic_side_effects` lint fires on the overloaded operators but not on
    /// plain floating-point arithmetic.
    fn looking_from(position: Vec3, target: Vec3) -> Self {
        let offset = Vec3::new(
            position.x - target.x,
            position.y - target.y,
            position.z - target.z,
        );
        let distance = offset.length().max(MIN_ORBIT_DISTANCE);
        Self {
            target,
            distance,
            yaw: offset.x.atan2(offset.z),
            pitch: (offset.y / distance).asin().clamp(-MAX_PITCH, MAX_PITCH),
        }
    }

    /// The camera transform this orbit puts the camera at.
    fn transform(self) -> Transform {
        let horizontal = self.distance * self.pitch.cos();
        let position = Vec3::new(
            self.target.x + horizontal * self.yaw.sin(),
            self.target.y + self.distance * self.pitch.sin(),
            self.target.z + horizontal * self.yaw.cos(),
        );
        Transform::from_translation(position).looking_at(self.target, Vec3::Y)
    }
}

/// A marker on the scene root, so a cell change can despawn and rebuild it
/// without touching the camera, the lights or the chrome.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryScene;

/// A marker on the gallery's own view camera.
///
/// **Not** `With<Camera3d>`, and the difference is not pedantry: it cost a
/// working gallery. `crate::probes`' reflection-probe rig spawns **six
/// `Camera3d` capture cameras per rig** (one per cube face), so the moment the
/// probes were added, `Query<&mut Transform, With<Camera3d>>::single_mut()`
/// stopped matching one entity and started returning `Err` — and every `if let
/// Ok(..)` that positioned the camera quietly did nothing. The camera sat at the
/// origin, inside whatever scene was loaded, and most of them "went invisible".
///
/// The lesson generalises past this file: a component another plugin also spawns
/// is not an identity. Query the thing you own.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryCamera;

/// A marker on the gallery's own key / fill lights, so a scene that declares
/// [`SceneLighting::Own`] can stand them down. See there for why: an 8000-lux key
/// light makes a projector's cone invisible, because the wall is already white.
#[derive(Component, Debug, Clone, Copy)]
struct StageLight;

/// A marker on the header line, which reports the live cell.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryHeader;

/// A marker on the line reporting what the shown scene declares about itself.
#[derive(Component, Debug, Clone, Copy)]
struct GalleryDeclarations;

/// What [`report_declarations`] reads off a declaring entity.
type DeclaredData = (
    &'static Name,
    Option<&'static DeclaredBounds>,
    Option<&'static SymmetricAbout>,
    Option<&'static UvsInUnitSquare>,
    Option<&'static SamplerMayClamp>,
);

/// The filter that keeps [`report_declarations`] to entities that declare
/// something at all.
type HasDeclaration = Or<(
    With<DeclaredBounds>,
    With<SymmetricAbout>,
    With<UvsInUnitSquare>,
    With<SamplerMayClamp>,
)>;

/// Run the gallery: a window, the viewer's real converters, and every registered
/// scene rendered on its own.
///
/// Returns `()` rather than a `Result` because there is nothing here to fail at:
/// no credentials to reject, no grid to be unreachable, no world to fail to load.
/// That is the whole point of the gallery, and the signature says so.
pub fn run() {
    crate::init_tracing();
    let args = GalleryArgs::parse();
    let start = match args.scene.as_deref() {
        None => 0,
        Some(wanted) => match SCENES.iter().position(|scene| scene.id == wanted) {
            Some(index) => index,
            None => {
                // Naming a scene that does not exist is a typo, and the useful
                // answer is the list — not a silent fall back to scene 0, which
                // looks like the tool ignoring you.
                let known: Vec<&str> = SCENES.iter().map(|scene| scene.id).collect();
                error!(
                    "no scene named `{wanted}`. Known scenes: {}",
                    known.join(", ")
                );
                return;
            }
        },
    };
    info!(
        scenes = SCENES.len(),
        "starting the render gallery: no login, no world; N/P steps scenes, L cycles LOD, \
         R restarts, Space pauses"
    );
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "sl-client-bevy-viewer — render gallery".to_owned(),
                        name: Some("sl-client-bevy-viewer-scenes".to_owned()),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                // The binary installs its own subscriber (`crate::init_tracing`),
                // as the viewer does; two would clash over the global slot.
                .disable::<LogPlugin>(),
        )
        .insert_resource(GalleryCell {
            scene: start,
            ..GalleryCell::default()
        })
        .insert_resource(ClearColor(BACKGROUND))
        .insert_resource(DirectionalLightShadowMap::default())
        // The viewer's own time-varying systems, exactly as `crate::render_test`
        // installs them. Without these a dynamic scene renders **nothing**: its
        // emitter spawns and no cloud is ever built, which is what the particle
        // fountain showed the first time it was looked at — an empty screen that
        // no check could complain about, because the scene was fine and the
        // gallery was not running it. `TextureManager` sits at its `Default`: no
        // capability URL, so it never fetches.
        .init_resource::<ParticleSim>()
        .init_resource::<TextureManager>()
        // The viewer's real reflection probes (P33): a capture rig that renders
        // the scene into a cubemap and binds it to the view as the default probe,
        // plus the per-object local probes. Entirely session-free — it captures
        // whatever is in front of the camera — so the gallery gets the viewer's
        // actual image-based lighting rather than an approximation of it, and
        // `metallic-sphere-among-prims` reflects its neighbours for real.
        .add_plugins(ReflectionProbePlugin)
        .init_resource::<Orbit>()
        .add_systems(Startup, (setup_stage, setup_chrome, setup_particles))
        .add_systems(
            Update,
            (
                drive_keys,
                // After `drive_keys`, so a scene change's opening orbit is what
                // this frame's arrows move rather than the previous scene's.
                drive_orbit.after(drive_keys),
                hold_stage_ambient,
                drive_particles,
                report_declarations,
                quit_on_escape,
            ),
        )
        .run();
}

/// Spawn the camera and the fixed lighting every scene is judged under.
///
/// **Fixed**, and that is the point: a gallery whose lighting drifted between
/// scenes would make two geometries look different for a reason that has nothing
/// to do with either. One key light, one fill, one ambient — so a difference on
/// screen is a difference in the thing.
fn setup_stage(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        // The viewer's own exposure, so a material reads here the way it reads
        // in-world rather than a stop brighter. `install_global_probe` also reads
        // it, to calibrate the probe's intensity against this view.
        Exposure::default(),
        // As the viewer does: render into a floating-point target. Without `Hdr`,
        // Bevy takes an 8-bit target as the cue to tonemap `StandardMaterial`
        // inside the mesh shader, which is a different transfer from the one the
        // viewer applies — so a material would read differently here than
        // in-world, which is the one thing this gallery must not do.
        Hdr,
        Transform::default(),
        // `drive_particles` billboards each particle at the fly camera and reads
        // its pose to do it; without the marker no cloud is ever built. It is also
        // what `install_global_probe` hangs the default reflection probe on.
        FlyCamera::default(),
        GalleryCamera,
        Name::new("gallery-camera"),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        StageLight,
        Name::new("gallery-key-light"),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-6.0, 2.0, -4.0).looking_at(Vec3::ZERO, Vec3::Y),
        StageLight,
        Name::new("gallery-fill-light"),
    ));
    // The resource, not the per-camera component: `GlobalAmbientLight` is what
    // the viewer's own sky drives (`crate::sky`), so the gallery lights a scene
    // through the same knob.
    commands.insert_resource(GlobalAmbientLight {
        brightness: STAGE_AMBIENT,
        ..default()
    });
}

/// Spawn the chrome: the scene's id, its summary, and the keys.
fn setup_chrome(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            Name::new("gallery-chrome"),
        ))
        .with_children(|chrome| {
            chrome.spawn((
                Text::new(""),
                TextFont::from_font_size(CHROME_FONT_SIZE),
                TextColor(HEADER_COLOR),
                GalleryHeader,
            ));
            chrome.spawn((
                Text::new(""),
                TextFont::from_font_size(CHROME_FONT_SIZE),
                TextColor(CHROME_COLOR),
                GalleryDeclarations,
            ));
            chrome.spawn((
                Text::new(
                    "N/P scene · L lod · R restart · Space pause · arrows orbit · +/- dolly · \
                     Esc quit",
                ),
                TextFont::from_font_size(CHROME_FONT_SIZE),
                TextColor(CHROME_COLOR),
            ));
        });
}

/// Report what the shown scene **declares** about itself.
///
/// Not decoration. The declared tier (`crate::render_test`) is the harness saying
/// "this scene claims to be 1 m across and symmetric about X, and I checked it" —
/// but a claim can be *wrong* in a way no check can catch, because the check only
/// compares the geometry against the claim. If a fixture declares a box is
/// symmetric about X and it is really symmetric about Z, the suite is green and
/// says nothing.
///
/// The only thing that catches a wrong claim is a human reading it next to the
/// picture, which is exactly what the gallery is for. So the claims are on
/// screen, beside the thing they are about.
fn report_declarations(
    scenes: Query<DeclaredData, HasDeclaration>,
    mut line: Query<&mut Text, With<GalleryDeclarations>>,
) {
    let mut claims: Vec<String> = scenes
        .iter()
        .map(|(name, bounds, symmetry, atlas, clamp)| {
            let mut parts: Vec<String> = Vec::new();
            if let Some(bounds) = bounds {
                parts.push(format!(
                    "half-extents {:?} ±{}",
                    bounds.half_extents, bounds.tolerance
                ));
            }
            if let Some(symmetry) = symmetry {
                parts.push(format!(
                    "symmetric about {:?} ({})",
                    symmetry.axes, symmetry.reason
                ));
            }
            if let Some(atlas) = atlas {
                parts.push(format!("UVs in the unit square ({})", atlas.reason));
            }
            if let Some(clamp) = clamp {
                parts.push(format!("sampler may clamp ({})", clamp.reason));
            }
            format!("{name}: {}", parts.join(", "))
        })
        .collect();
    claims.sort();
    claims.dedup();
    if let Ok(mut text) = line.single_mut() {
        *text = Text::new(if claims.is_empty() {
            "declares: nothing — only the universal checks apply".to_owned()
        } else {
            format!("declares · {}", claims.join(" · "))
        });
    }
}

/// Rebuild the shown scene: despawn the old root, spawn the new one, and re-aim
/// the camera at the pose the scene declares.
#[expect(
    clippy::too_many_arguments,
    reason = "the cell, the four things it rebuilds, and the assets it builds from are each \
              genuinely independent; bundling them would only move the list"
)]
fn rebuild(
    cell: GalleryCell,
    commands: &mut Commands,
    existing: &Query<Entity, With<GalleryScene>>,
    camera: &mut Query<&mut Transform, With<GalleryCamera>>,
    header: &mut Query<&mut Text, With<GalleryHeader>>,
    stage: &mut Query<&mut Visibility, With<StageLight>>,
    orbit: &mut Orbit,
    assets: &mut SceneAssets<'_>,
) {
    for root in existing.iter() {
        commands.entity(root).despawn();
    }
    let Some(scene) = cell.scene() else {
        return;
    };
    let root = commands.spawn((scene_root(), GalleryScene)).id();
    (scene.spawn)(cell.cx(), root, commands, assets);

    // Stand the stage rig down for a scene that lights itself, or its own lights
    // are invisible against it. The ambient goes with them (`hold_stage_ambient`):
    // a projector's cone is a *contrast*, and 200 nits of fill erases it.
    let own = scene.lighting == SceneLighting::Own;
    for mut visibility in stage.iter_mut() {
        *visibility = if own {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }

    // The scene's declared camera pose is in Second Life region-local metres
    // (Z-up), the same frame as the viewer's `--camera-position` CLI, so it is
    // converted here exactly as the viewer converts an object's. It becomes the
    // *opening* orbit rather than a fixed transform — see `Orbit`.
    let basis = scene_root_transform().rotation;
    *orbit = Orbit::looking_from(
        basis.mul_vec3(scene.camera.position),
        basis.mul_vec3(scene.camera.look_at),
    );
    if let Ok(mut transform) = camera.single_mut() {
        *transform = orbit.transform();
    }
    if let Ok(mut text) = header.single_mut() {
        let timeline = if scene.timeline.is_dynamic() {
            format!(" · sampled at {:?}s", scene.timeline.samples)
        } else {
            String::new()
        };
        *text = Text::new(format!(
            "{} [{}/{}] · {:?}{timeline}\n{}",
            scene.id,
            cell.scene.saturating_add(1),
            SCENES.len(),
            cell.lod,
            scene.what,
        ));
    }
}

/// The keys: step scenes, cycle the LOD, restart, pause.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's arguments are its resource/query dependencies"
)]
fn drive_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut cell: ResMut<GalleryCell>,
    mut commands: Commands,
    existing: Query<Entity, With<GalleryScene>>,
    mut camera: Query<&mut Transform, With<GalleryCamera>>,
    mut header: Query<&mut Text, With<GalleryHeader>>,
    mut stage: Query<&mut Visibility, With<StageLight>>,
    mut orbit: ResMut<Orbit>,
    mut time: ResMut<Time<Virtual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inverse_bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut started: Local<bool>,
) {
    if keys.just_pressed(PAUSE_KEY) {
        if time.is_paused() {
            time.unpause();
        } else {
            time.pause();
        }
    }

    let mut changed = !*started;
    *started = true;
    if keys.just_pressed(NEXT_KEY) {
        let next = cell.scene.saturating_add(1);
        cell.scene = if next >= SCENES.len() { 0 } else { next };
        changed = true;
    }
    if keys.just_pressed(PREVIOUS_KEY) {
        cell.scene = cell
            .scene
            .checked_sub(1)
            .unwrap_or_else(|| SCENES.len().saturating_sub(1));
        changed = true;
    }
    if keys.just_pressed(LOD_KEY) {
        cell.lod = cell.next_lod();
        changed = true;
    }
    if keys.just_pressed(RESTART_KEY) {
        changed = true;
    }
    if !changed {
        return;
    }
    let mut assets = SceneAssets {
        meshes: &mut meshes,
        materials: &mut materials,
        images: &mut images,
        inverse_bindposes: &mut inverse_bindposes,
    };
    rebuild(
        *cell,
        &mut commands,
        &existing,
        &mut camera,
        &mut header,
        &mut stage,
        &mut orbit,
        &mut assets,
    );
}

/// Re-set the stage ambient every frame.
///
/// Necessary because of how the probes and the ambient interact, and the
/// interaction is not obvious. `crate::probes`'s `suppress_global_ambient` runs
/// in `PostUpdate` and *multiplies* `GlobalAmbientLight` down each frame, so the
/// probe's image-based lighting is not stacked on a second flat ambient term. In
/// the viewer that is safe: the sky system re-sets the ambient every frame in
/// `Update`, so the multiply is a per-frame attenuation of a per-frame-set value.
///
/// The gallery has no sky. Without this the multiply would compound frame after
/// frame and the ambient would decay to nothing — a scene that dims to black over
/// a few seconds for no visible reason. So the gallery plays the sky's part: it
/// sets the value the attenuation is applied to.
fn hold_stage_ambient(cell: Res<GalleryCell>, mut ambient: ResMut<GlobalAmbientLight>) {
    let own = cell
        .scene()
        .is_some_and(|scene| scene.lighting == SceneLighting::Own);
    ambient.brightness = if own {
        OWN_LIGHTING_AMBIENT
    } else {
        STAGE_AMBIENT
    };
}

/// Orbit the camera: arrows swing it around the scene, `+` / `-` dolly.
///
/// A camera a human can move is what makes a *declared* opening pose safe to get
/// wrong — and every one of them was, first time. It also answers the question
/// the gallery exists for that a fixed view cannot: half of judging geometry is
/// walking around it, because a silhouette is right from one angle by luck and a
/// hole is invisible from the front.
fn drive_orbit(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut orbit: ResMut<Orbit>,
    mut camera: Query<&mut Transform, With<GalleryCamera>>,
) {
    // Real time, not the virtual clock: `Space` pauses the *scene*, and a camera
    // that froze with it would stop a human inspecting the very frame they paused
    // to look at.
    let step = time.delta_secs();
    let mut moved = false;
    for (key, yaw) in [(KeyCode::ArrowLeft, 1.0_f32), (KeyCode::ArrowRight, -1.0)] {
        if keys.pressed(key) {
            orbit.yaw += yaw * ORBIT_RATE * step;
            moved = true;
        }
    }
    for (key, pitch) in [(KeyCode::ArrowUp, 1.0_f32), (KeyCode::ArrowDown, -1.0)] {
        if keys.pressed(key) {
            orbit.pitch = (orbit.pitch + pitch * ORBIT_RATE * step).clamp(-MAX_PITCH, MAX_PITCH);
            moved = true;
        }
    }
    // Proportional dolly: a fixed metres-per-second step would crawl across a
    // 36 m tree and fly straight through a 1 m prim.
    for (keys_for, direction) in [
        ([KeyCode::Equal, KeyCode::NumpadAdd], -1.0_f32),
        ([KeyCode::Minus, KeyCode::NumpadSubtract], 1.0),
    ] {
        if keys_for.iter().any(|key| keys.pressed(*key)) {
            let scale = 1.0 + direction * DOLLY_RATE * step;
            orbit.distance = (orbit.distance * scale).max(MIN_ORBIT_DISTANCE);
            moved = true;
        }
    }
    if !moved {
        return;
    }
    if let Ok(mut transform) = camera.single_mut() {
        *transform = orbit.transform();
    }
}

/// `Escape` quits, as it does in the UI gallery.
fn quit_on_escape(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
