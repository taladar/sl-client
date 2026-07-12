//! Reflection probes (Phase 33): fold a prim's `LLReflectionProbeParams`
//! extra-param block into the scene mirror, and drive the scene-wide **default**
//! reflection probe — a real-time captured environment cubemap — the way the
//! reference viewer's `LLReflectionMapManager` provides its fallback probe.
//!
//! **Detect (ingest).** A reflection probe is not a `PrimFlags` bit — a prim is a
//! probe exactly when it carries the `LLReflectionProbeParams` extra-param block
//! (`ExtraParams` type `0x90`), the way `LLViewerObject::getReflectionProbeParams`
//! keys off the block's presence. sl-proto already decodes that block into a
//! [`ReflectionProbe`] on `Object::extra.reflection_probe` (the two packed floats —
//! ambiance and clip distance — plus the flag byte: box-vs-sphere influence
//! volume, dynamic-object capture, and mirror). [`reflection_probe_from_object`]
//! lifts a present block onto an [`ObjectReflectionProbe`] component that
//! [`apply_object`] attaches to (or clears from) each object entity as its updates
//! arrive, exactly the way [`apply_flexi`](crate::flexi) /
//! [`apply_light`](crate::lights) / [`apply_particles`](crate::particles) do — a
//! prim toggled probe on or off in-world flips the block present / absent, so the
//! component is refreshed every update. The component also carries the prim's metre
//! scale, from which a *local* probe's box / sphere influence volume is derived —
//! but placing those per-object local probes is a later roadmap item; this module
//! implements only the ingest plus the default (global) probe.
//!
//! **Default probe capture & render (see [`ReflectionProbePlugin`]).** Bevy 0.19 has
//! the sink side of reflection probes — a [`GeneratedEnvironmentMapLight`] on the
//! view is the "global" probe that lights every PBR surface (the reference viewer's
//! default probe). What Bevy lacks is the *source*: it never renders the scene into
//! a cubemap. This module supplies that missing half — mirroring
//! `LLReflectionMapManager`'s real-time capture — by pointing six 90° cameras at the
//! viewpoint (one per cube face) that render the scene into six `Rgba16Float` colour
//! targets; a render-world blit ([`copy_probe_faces`]) copies those into the six
//! layers of a cube [`Image`], which a [`GeneratedEnvironmentMapLight`] filters
//! (irradiance + roughness-mipped radiance) into the diffuse / specular maps the PBR
//! shader samples. Six separate colour targets plus a copy (rather than rendering
//! straight into the cube's layers) keeps camera sizing on Bevy's ordinary
//! image-target path — a cube-layer render target would need render-world manual
//! texture views that the main-world camera-sizing pass cannot resolve.
//!
//! The capture is amortized ([`CAPTURE_PERIOD_FRAMES`]): the six faces are
//! re-rendered one per frame in a brief burst a few times per second, then the
//! cameras idle, so the costly six-face scene re-render (each with its own shadow
//! pass) is not paid every frame. The environment changes slowly, so the staleness
//! is imperceptible while the frame rate stays near the un-probed baseline.
//!
//! **Consistent image-based lighting.** Bevy applies the view environment map only
//! to `StandardMaterial` (prims, meshes, avatars). The viewer's custom sky / terrain
//! / water materials do not sample it, so — to avoid double-counting a flat ambient
//! on top of the probe's diffuse contribution — [`suppress_global_ambient`] drops the
//! sky-set `GlobalAmbientLight`, and the terrain and water shaders sample the probe
//! themselves (terrain reads its diffuse irradiance for ambient; water reflects the
//! specular cube). Sky stays the source and is not itself lit by the probe.
//!
//! The exact reflection / ambient brightness is left to a later calibration pass
//! (alongside other lighting features such as ambient occlusion) — the intensity
//! ([`PROBE_INTENSITY`] / `SL_VIEWER_PROBE_INTENSITY`) and the residual ambient
//! ([`probe_ambient_scale`] / `SL_VIEWER_PROBE_AMBIENT_SCALE`) are exposed as tuning
//! knobs, and `SL_VIEWER_PROBE_TEST_SPHERE=1` spawns a mirror ball to inspect the
//! captured environment.
//!
//! [`apply_object`]: crate::objects
//! [`GeneratedEnvironmentMapLight`]: bevy::light::GeneratedEnvironmentMapLight

use crate::camera::FlyCamera;
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::CUBE_MAP_FACES;
use bevy::camera::{Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, Origin3d, TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use sl_client_bevy::{Object, ReflectionProbe, ReflectionProbeFlags};
use std::f32::consts::FRAC_PI_2;

/// A component marking an object entity as a **reflection probe**, carrying the
/// decoded `LLReflectionProbeParams` parameters (in Second Life semantics) plus
/// the prim's metre scale — the inputs the capture / volume side needs.
///
/// Attached to (and refreshed / cleared on) each object entity by
/// [`apply_object`](crate::objects) as its updates arrive. See
/// [`reflection_probe_from_object`] for the present-vs-absent lift.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ObjectReflectionProbe {
    /// The decoded reflection-probe parameters: the ambiance (irradiance) scale,
    /// the reflection-capture near-clip distance in metres, and the flag set
    /// (box-vs-sphere volume, dynamic capture, mirror).
    pub(crate) data: ReflectionProbe,
    /// The prim's Second Life metre scale, refreshed every update so a **resized**
    /// probe's influence volume (a box of these half-extents, or a sphere of the
    /// bounding radius) stays correct. The reference viewer likewise derives the
    /// probe volume from the prim's dimensions, not from the probe params.
    pub(crate) scale: [f32; 3],
}

/// Lift an object's reflection-probe block onto an [`ObjectReflectionProbe`], or
/// `None` when the object is not (or is no longer) a probe.
///
/// Mirrors the reference viewer's `LLViewerObject::getReflectionProbeParams`: a
/// prim is a probe exactly when it carries a reflection-probe extra-param block, so
/// this is a straight `Option` lift with no sentinel to reject.
pub(crate) fn reflection_probe_from_object(object: &Object) -> Option<ObjectReflectionProbe> {
    object
        .extra
        .reflection_probe
        .map(|data| ObjectReflectionProbe {
            data,
            scale: [object.scale.x, object.scale.y, object.scale.z],
        })
}

/// Reconcile an object entity's [`ObjectReflectionProbe`] component with its
/// current reflection-probe block: insert / refresh it when the prim is a probe,
/// remove it when the prim was changed to non-probe in-world (the block dropped) or
/// never was one. Called on both the spawn and update paths so a prim toggled probe
/// on or off between updates is tracked, the way [`apply_flexi`](crate::flexi) /
/// [`apply_light`](crate::lights) / [`apply_particles`](crate::particles) are.
pub(crate) fn apply_reflection_probe(
    entity: Entity,
    probe: Option<ObjectReflectionProbe>,
    commands: &mut Commands,
) {
    match probe {
        Some(probe) => {
            let data = &probe.data;
            debug!(
                "object reflection probe: ambiance={:.2} clip_distance={:.2}m \
                 box_volume={} dynamic={} mirror={}",
                data.ambiance,
                data.clip_distance,
                data.flags.contains(ReflectionProbeFlags::BOX_VOLUME),
                data.flags.contains(ReflectionProbeFlags::DYNAMIC),
                data.flags.contains(ReflectionProbeFlags::MIRROR),
            );
            commands.entity(entity).insert(probe);
        }
        None => {
            commands.entity(entity).remove::<ObjectReflectionProbe>();
        }
    }
}

/// The per-face cubemap capture resolution, in texels. Must be a power of two
/// (and ≤ 8192) for [`GeneratedEnvironmentMapLight`]'s filter to accept the cube.
/// 128² per face matches the reference viewer's default probe resolution — enough
/// for a convincing reflection once roughness-filtered, cheap enough to re-render
/// six faces on a slow cadence.
const CAPTURE_SIZE: u32 = 128;

/// The number of cube faces (and therefore capture cameras) per probe.
const FACE_COUNT: usize = 6;

/// Frames between environment re-captures. The six faces are re-rendered one per
/// frame over a short burst (the first [`FACE_COUNT`] frames of each period), then
/// the capture cameras idle — so the expensive six-face scene re-render (each with
/// its own shadow pass) is paid only in a brief burst a few times per second rather
/// than every frame. The environment changes slowly, so the resulting staleness is
/// imperceptible while the frame rate stays near the un-probed baseline.
const CAPTURE_PERIOD_FRAMES: usize = 180;

/// The intensity (cd/m²) the captured environment contributes as image-based
/// lighting. The capture cameras render **linear scene radiance** (HDR, no
/// tonemapping), i.e. the same physical units the main view sees before its own
/// tonemap, so a unit scale keeps a probe's reflections consistent with the direct
/// view rather than over- or under-bright.
const PROBE_INTENSITY: f32 = 1200.0;

/// How much of the sky-driven [`GlobalAmbientLight`] to keep once the reflection
/// probe is providing image-based ambient to both PBR objects and the terrain.
/// Default `0.0` drops it entirely (the probe is the single ambient source, so it
/// is not double-counted); overridable by `SL_VIEWER_PROBE_AMBIENT_SCALE`.
fn probe_ambient_scale() -> f32 {
    std::env::var("SL_VIEWER_PROBE_AMBIENT_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// Attenuate the sky-set [`GlobalAmbientLight`] each frame (after the sky system
/// re-sets it) so the reflection probe's image-based lighting is not stacked on top
/// of a second flat ambient term — the double-count that otherwise over-brightens
/// probe-lit PBR surfaces.
fn suppress_global_ambient(mut ambient: ResMut<GlobalAmbientLight>) {
    ambient.brightness *= probe_ambient_scale();
}

/// The environment intensity to use, overridable at runtime by
/// `SL_VIEWER_PROBE_INTENSITY` (a diagnostic knob while the reflection strength is
/// being calibrated). Falls back to [`PROBE_INTENSITY`].
fn probe_intensity() -> f32 {
    std::env::var("SL_VIEWER_PROBE_INTENSITY")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(PROBE_INTENSITY)
}

/// A component on each capture camera marking it as one face of the reflection
/// probe's cubemap (the face / cube-array-layer index it renders), so the driver
/// can pose and toggle the six cameras.
#[derive(Component, Debug, Clone, Copy)]
struct ProbeCaptureCamera {
    /// The cube face (array layer, `0..6`) this camera renders — indexed the same
    /// as [`CUBE_MAP_FACES`], so the camera's look direction and the cube layer the
    /// copy writes agree.
    face: usize,
}

/// The scene-wide **default reflection probe** (the reference viewer's fallback
/// probe): a single environment cubemap captured around the viewpoint and bound to
/// the main view as a global [`EnvironmentMapLight`],
/// lighting every PBR surface that is not inside a nearer local probe's volume.
///
/// Holds the cube [`Image`] the capture is assembled into (and the filter reads),
/// the six per-face colour targets the capture cameras render into, and those six
/// camera entities. Created once by [`setup_global_probe`].
#[derive(Resource)]
struct GlobalProbe {
    /// The cube [`Image`] (six `Rgba16Float` layers) the six face targets are
    /// copied into and that the view's [`GeneratedEnvironmentMapLight`] filters.
    cube: Handle<Image>,
    /// The six capture-camera entities, indexed as [`CUBE_MAP_FACES`]. Each holds
    /// its face colour target alive through its `RenderTarget::Image` handle.
    cameras: [Entity; FACE_COUNT],
    /// Whether the global [`GeneratedEnvironmentMapLight`] has been installed on the
    /// main view yet (it is deferred until the fly-camera entity exists).
    installed: bool,
}

/// One probe's face-target → cube-layer copy mapping, snapshotted for the render
/// world: the cube image and its six per-face source images, by asset id.
#[derive(Clone)]
struct ProbeCubeCopy {
    /// The destination cube image (its six array layers receive the faces).
    cube: AssetId<Image>,
    /// The six source face images, in cube-layer order.
    faces: [AssetId<Image>; FACE_COUNT],
}

/// The render-world work-list of probe cubes to reassemble each frame, extracted
/// from the main world. [`copy_probe_faces`] walks it and blits each probe's six
/// captured face textures into its cube's six array layers.
#[derive(Resource, Clone, Default, ExtractResource)]
struct ProbeCubeCopies {
    /// One entry per probe (currently just the global default probe).
    copies: Vec<ProbeCubeCopy>,
}

/// The reflection-probe plugin (Phase 33): captures a scene environment cubemap and
/// drives it as image-based lighting, supplying the scene-render half of reflection
/// probes that Bevy's [`GeneratedEnvironmentMapLight`] filter and
/// [`EnvironmentMapLight`] consumer expect but never
/// produce themselves.
///
/// This slice installs the **default** probe: one cubemap captured around the
/// viewpoint, bound globally to the main view (the reference viewer's fallback probe
/// used wherever no nearer local probe applies). The per-object local probes
/// ([`ObjectReflectionProbe`]) reuse the same capture machinery (a cube plus six
/// face cameras) once their volume placement lands.
pub(crate) struct ReflectionProbePlugin;

impl Plugin for ReflectionProbePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProbeCubeCopies>()
            .init_resource::<ProbeTestSphere>()
            .add_plugins(ExtractResourcePlugin::<ProbeCubeCopies>::default())
            .add_systems(Startup, setup_global_probe)
            .add_systems(
                Update,
                (
                    install_global_probe,
                    drive_global_probe,
                    spawn_probe_test_sphere,
                )
                    .chain(),
            )
            // Runs after the sky system (Update) re-sets the ambient each frame.
            .add_systems(PostUpdate, suppress_global_ambient);

        // The face → cube-layer blit runs in the render world after the capture
        // cameras have drawn this frame's faces; the view's env-map filter reads the
        // reassembled cube on the following frame (a one-frame lag that is
        // imperceptible for a slowly re-captured environment).
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(Render, copy_probe_faces.after(RenderSystems::Render));
    }
}

/// Build a single-face colour target: a square `Rgba16Float` render texture the
/// capture camera draws HDR scene radiance into, also readable as a copy source so
/// the render-world blit can lift it into the cube's matching layer.
fn create_face_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut image =
        Image::new_target_texture(CAPTURE_SIZE, CAPTURE_SIZE, TextureFormat::Rgba16Float, None);
    // `new_target_texture` sets TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT; the
    // blit additionally reads the face as a copy source.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    images.add(image)
}

/// Build the destination cube [`Image`]: six `Rgba16Float` array layers viewed as a
/// cubemap, a copy destination for the per-face blit and a storage / sampled source
/// for [`GeneratedEnvironmentMapLight`]'s realtime filter.
fn create_cube_image(images: &mut Assets<Image>) -> Handle<Image> {
    // A single `Rgba16Float` texel (four 16-bit floats = eight bytes) as the fill
    // pattern; `new_fill` replicates it across all six layers.
    let mut image = Image::new_fill(
        Extent3d {
            width: CAPTURE_SIZE,
            height: CAPTURE_SIZE,
            depth_or_array_layers: u32::try_from(FACE_COUNT).unwrap_or(6),
        },
        TextureDimension::D2,
        &[0u8; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::all(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::STORAGE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    images.add(image)
}

/// Spawn one cube-face capture camera: a 90°-FOV square HDR camera rendering the
/// world into `face_image`, initially inactive (the driver toggles it on its slow
/// capture cadence).
fn spawn_capture_camera(commands: &mut Commands, face: usize, face_image: Handle<Image>) -> Entity {
    commands
        .spawn((
            Camera3d::default(),
            Camera {
                // Render before the main view (order 0). The env-map filter reads the
                // cube a frame later, so ordering among the capture cameras is
                // irrelevant; a single negative order keeps them all ahead of the view.
                order: -1,
                // Toggled on by `drive_global_probe` only when a face is due for
                // re-capture, so the six-face scene re-render is amortized.
                is_active: false,
                ..default()
            },
            // A 2D colour target (not a window), so camera sizing resolves from the
            // image and no manual texture-view plumbing is needed.
            RenderTarget::Image(face_image.into()),
            Projection::Perspective(PerspectiveProjection {
                fov: FRAC_PI_2,
                aspect_ratio: 1.0,
                near: 0.05,
                far: 4096.0,
                ..default()
            }),
            Transform::default(),
            // The face target is `Rgba16Float`, so the camera must render HDR and
            // single-sampled (the target is not multisampled), and must not tonemap —
            // the cube holds linear scene radiance for image-based lighting.
            Hdr,
            Msaa::Off,
            Tonemapping::None,
            ProbeCaptureCamera { face },
        ))
        .id()
}

/// Startup: create the default probe's cube and six face targets, spawn its six
/// capture cameras, and register the cube for the render-world blit.
fn setup_global_probe(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut copies: ResMut<ProbeCubeCopies>,
) {
    let cube = create_cube_image(&mut images);
    let faces: [Handle<Image>; FACE_COUNT] =
        core::array::from_fn(|_| create_face_image(&mut images));
    // `.get(face)` (rather than `faces[face]`) to stay clear of the workspace
    // `indexing_slicing` lint; the `from_fn` index is always in range.
    let cameras: [Entity; FACE_COUNT] = core::array::from_fn(|face| {
        let handle = faces.get(face).cloned().unwrap_or_default();
        spawn_capture_camera(&mut commands, face, handle)
    });

    copies.copies.push(ProbeCubeCopy {
        cube: cube.id(),
        faces: core::array::from_fn(|face| faces.get(face).map(Handle::id).unwrap_or_default()),
    });

    commands.insert_resource(GlobalProbe {
        cube,
        cameras,
        installed: false,
    });
    debug!("reflection probes: default (global) probe capture set up at {CAPTURE_SIZE}² per face");
}

/// Install the default probe's [`GeneratedEnvironmentMapLight`] on the main view
/// once the fly-camera exists, so every PBR surface receives the captured
/// environment as image-based lighting. Runs each frame until it succeeds, then
/// idles (the flag guards against re-inserting).
fn install_global_probe(
    mut commands: Commands,
    mut probe: ResMut<GlobalProbe>,
    camera: Query<Entity, With<FlyCamera>>,
) {
    if probe.installed {
        return;
    }
    let Ok(view) = camera.single() else {
        return;
    };
    commands.entity(view).insert(GeneratedEnvironmentMapLight {
        environment_map: probe.cube.clone(),
        intensity: probe_intensity(),
        // The cube is captured directly in Bevy world space, so it samples with no
        // extra reorientation.
        rotation: Quat::IDENTITY,
        affects_lightmapped_mesh_diffuse: true,
    });
    probe.installed = true;
    debug!("reflection probes: installed default environment map on the main view");
}

/// Drive the amortized environment capture: during the first [`FACE_COUNT`] frames
/// of each [`CAPTURE_PERIOD_FRAMES`] period, re-centre the capture cameras on the
/// viewpoint and activate one face per frame (re-rendering the whole cube over the
/// burst); the rest of the period the cameras idle, so the costly six-face scene
/// re-render is paid only briefly a few times per second rather than every frame.
fn drive_global_probe(
    probe: Res<GlobalProbe>,
    camera: Query<&GlobalTransform, With<FlyCamera>>,
    mut cameras: Query<(&ProbeCaptureCamera, &mut Transform, &mut Camera)>,
    mut phase: Local<usize>,
) {
    let Ok(view) = camera.single() else {
        return;
    };
    let eye = view.translation();
    // The phase within the capture period; the first `FACE_COUNT` frames capture one
    // face each, the remainder idle. Kept in range by an explicit wrap rather than
    // `%` (the workspace `arithmetic_side_effects` lint).
    let this_phase = *phase;
    *phase = phase.wrapping_add(1);
    if *phase >= CAPTURE_PERIOD_FRAMES {
        *phase = 0;
    }
    let capturing = this_phase < FACE_COUNT;

    for &entity in &probe.cameras {
        let Ok((capture, mut transform, mut cam)) = cameras.get_mut(entity) else {
            continue;
        };
        // Pose the camera only while the burst may render it (idle frames leave the
        // transform untouched — the camera is inactive anyway).
        if capturing && let Some(face) = CUBE_MAP_FACES.get(capture.face) {
            *transform = Transform::from_translation(eye).looking_to(face.target, face.up);
        }
        cam.is_active = capturing && capture.face == this_phase;
    }
}

/// Whether the reflection-probe diagnostic mirror ball is enabled
/// (`SL_VIEWER_PROBE_TEST_SPHERE=1`). Off by default; a debug affordance to *see* the
/// captured environment, since ordinary Second Life / OpenSim content rarely carries
/// the metallic PBR materials a probe visibly reflects.
fn probe_test_sphere_enabled() -> bool {
    std::env::var("SL_VIEWER_PROBE_TEST_SPHERE")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Tracks whether the diagnostic mirror ball has been spawned yet (it is deferred
/// until the fly-camera entity exists, then spawned once).
#[derive(Resource, Default)]
struct ProbeTestSphere {
    /// Whether the ball has already been spawned.
    spawned: bool,
}

/// Spawn a perfectly-mirrored sphere parented to the main view (a "mirror ball") so
/// the captured environment cubemap is directly visible as its reflection — a
/// diagnostic for the whole capture → copy → filter → image-based-lighting chain,
/// enabled only by [`probe_test_sphere_enabled`]. A metallic, near-zero-roughness
/// `StandardMaterial` renders black without an environment map, so a lit ball
/// confirms the probe works (and its content confirms the orientation).
fn spawn_probe_test_sphere(
    mut commands: Commands,
    mut state: ResMut<ProbeTestSphere>,
    camera: Query<Entity, With<FlyCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if state.spawned || !probe_test_sphere_enabled() {
        return;
    }
    let Ok(view) = camera.single() else {
        return;
    };
    let mesh = meshes.add(Sphere::new(0.35));
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        metallic: 1.0,
        perceptual_roughness: 0.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        // A little right of, below, and ahead of the camera, so it stays framed as
        // the view moves (Bevy camera space: +X right, +Y up, −Z forward).
        Transform::from_xyz(0.55, -0.25, -1.6),
        ChildOf(view),
    ));
    state.spawned = true;
    debug!("reflection probes: spawned diagnostic mirror ball on the main view");
}

/// Render world: blit each probe's six captured face textures into its cube's six
/// array layers, so the view's [`GeneratedEnvironmentMapLight`] filter reads a
/// complete environment cubemap. Runs after the capture cameras have drawn, issuing
/// its own command buffer (it does not run beneath the render graph, so it cannot use
/// `RenderContext`).
fn copy_probe_faces(
    copies: Res<ProbeCubeCopies>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if copies.copies.is_empty() {
        return;
    }
    let mut encoder = device.create_command_encoder(&default());
    let mut recorded = false;
    for copy in &copies.copies {
        let Some(cube) = images.get(copy.cube) else {
            continue;
        };
        for (index, face_id) in copy.faces.iter().enumerate() {
            let Some(face) = images.get(*face_id) else {
                continue;
            };
            let layer = u32::try_from(index).unwrap_or(0);
            encoder.copy_texture_to_texture(
                TexelCopyTextureInfo {
                    texture: &face.texture,
                    mip_level: 0,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                TexelCopyTextureInfo {
                    texture: &cube.texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: TextureAspect::All,
                },
                Extent3d {
                    width: CAPTURE_SIZE,
                    height: CAPTURE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
            recorded = true;
        }
    }
    if recorded {
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectReflectionProbe, reflection_probe_from_object};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Object, ReflectionProbe, ReflectionProbeFlags, Vector};

    /// A minimal plain prim object with no extra params — the fixture the probe
    /// tests decorate.
    fn bare_object() -> Object {
        use sl_client_bevy::{
            CircuitId, ObjectMotion, RegionHandle, RegionLocalObjectId, Rotation, Uuid,
        };
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
                x: 2.0,
                y: 4.0,
                z: 6.0,
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

    /// An object with no reflection-probe block is not a probe.
    #[test]
    fn no_probe_block_is_none() {
        assert_eq!(reflection_probe_from_object(&bare_object()), None);
    }

    /// A prim carrying a reflection-probe block lifts into a component holding it
    /// and the prim's scale (for the influence volume).
    #[test]
    fn probe_block_becomes_a_component() {
        let mut object = bare_object();
        let data = ReflectionProbe {
            ambiance: 0.5,
            clip_distance: 3.0,
            flags: ReflectionProbeFlags::BOX_VOLUME,
        };
        object.extra.reflection_probe = Some(data);
        assert_eq!(
            reflection_probe_from_object(&object),
            Some(ObjectReflectionProbe {
                data,
                scale: [2.0, 4.0, 6.0],
            })
        );
    }
}
