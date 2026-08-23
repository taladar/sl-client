//! The in-world **tracking beacon** (`viewer-beacons-beam-render`): the tall
//! vertical beam of light the reference viewer draws at a tracked position so you
//! can walk / fly toward it — the destination of a map double-click or teleport, a
//! tracked landmark, or a tracked avatar / friend. This is the reference's
//! `LLTracker` beacon (`lltracker.cpp` `renderBeacon` / `drawBeacon` /
//! `drawHUDArrow`), reimplemented for the Bevy scene.
//!
//! Three parts, all client-side rendering of an already-known position:
//!
//! - a world-space **beam** — two stacked, camera-facing translucent blades (a blue
//!   lower shaft from the ground up to the target, a red upper shaft from the target
//!   up to the sky ceiling; the target sits at the seam), colour-coded by what is
//!   tracked. Depth-tested but not depth-writing (the reference's
//!   `LLGLDepthTest(GL_TRUE, GL_FALSE)`), so the tall upper shaft pokes above nearer
//!   geometry and reads as a waypoint from behind a building. Drawn by
//!   [`BeaconBeamMaterial`] (the [`crate::parcel_borders`] unlit / alpha-blended /
//!   glow-mask-preserving material template);
//! - a **label** — the tracked thing's name and its distance from the agent
//!   (`"%.0f m"`), drawn over the beam so it reads through geometry (the reference's
//!   `LLHUDText` with `setZCompare(false)`). Rendered as a screen-space overlay
//!   pinned to the target's projected position — visually a world-anchored,
//!   constant-on-screen-size label, but always on top and free of the name-tag
//!   fade-distance cutoff (a beacon must stay visible at any range);
//! - a **direction arrow** — a small arrow (shaft + head) sitting on an ellipse
//!   around the beacon's projected position, pointing back out at it (the reference's
//!   `LLTracker::drawMarker`): it points up when the camera is below the beacon
//!   altitude, down when above, sideways when level, and pins to the viewport edge
//!   when the beacon is off-screen so you can turn to face it. Colour-matched to the
//!   beacon, and clickable to dismiss (stop tracking).
//!
//! The tracked position comes from the shared [`MapTracking`] resource
//! ([`crate::minimap`]) — the one beacon the minimap and (later) the world map both
//! drive. A tracked **location** resolves to a global position (this is also how a
//! map double-click / teleport destination and a tracked landmark surface, exactly
//! as the reference's `getTrackedPositionGlobal` returns one global position for all
//! of them); a tracked **avatar** follows its live in-world position. Setting and
//! clearing a beacon from the UI — the click-to-dismiss on the arrow and the
//! track-menu hand-off — is the separate `viewer-beacons-control` task; this module
//! takes the target and draws the beam / label / arrow.
//!
//! Reference (Firestorm, read-only): `lltracker.cpp` (`renderBeacon`, `drawBeacon`,
//! `drawHUDArrow`, `drawMarker`), `MapTrackColor` / `MapTrackColorUnder` in
//! `colors.xml`.

use bevy::app::Propagate;
use bevy::asset::{Asset, RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use sl_client_bevy::{AgentKey, RegionHandle, SlIdentity, Vector};

use crate::avatars::AvatarState;
use crate::camera::ViewerCamera;
use crate::coords::{metres_to_f32, sl_to_bevy_vec};
use crate::minimap::{MapTracking, TrackTarget};
use crate::name_tag_billboard::tag_render_layers;
use crate::terrain::TerrainState;
use crate::ui::UiRoot;
use crate::ui_font::UiFont;
use crate::world_api::FriendsModel;

/// The internal handle the beacon-beam shader (`beacon_beam.wgsl`) is loaded under,
/// so the material can reference it without an on-disk asset path.
const BEACON_BEAM_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("2f7a1c4d-8b30-4e59-9d17-6c04a2e8f513");

/// The half-width, in metres, of a beam blade (the reference's static beacon
/// half-width of one metre — `drawBeacon`'s `a = 2·pulse + 1` with the pulse off).
const BEAM_HALF_WIDTH_M: f32 = 1.0;

/// The top of the upper (red) shaft, in metres above the region ground — the
/// reference's `MAX_HEIGHT` sky ceiling, so the beam is visible poking up from
/// almost anywhere in the region.
const BEAM_TOP_M: f32 = 5020.0;

/// The reference `MapTrackColor` (`colors.xml` → `Red`): the upper shaft, above the
/// target, for a tracked location / landmark / teleport destination.
const MAP_TRACK_ABOVE: [f32; 3] = [0.729, 0.0, 0.121];

/// The reference `MapTrackColorUnder` (`colors.xml` → `Blue`): the lower shaft,
/// below the target.
const MAP_TRACK_BELOW: [f32; 3] = [0.0, 0.0, 1.0];

/// The tracked-avatar beacon colour (a distinct green, so a tracked avatar reads
/// differently from a tracked location's red/blue map-track beam).
const AVATAR_TRACK_COLOR: [f32; 3] = [0.15, 0.8, 0.2];

/// The tracked-friend beacon colour (a distinct gold).
const FRIEND_TRACK_COLOR: [f32; 3] = [1.0, 0.75, 0.1];

/// The base alpha of a beam blade before the shader's per-fragment distance ramp
/// (kept at 1 so the shader's distance-alpha clamp is the whole story, driving the
/// beacon alpha from distance as the reference does — nudged a touch more opaque than
/// the reference's `[0.2, 0.5]` so the beam reads as solid as it does in SL).
const BEAM_BASE_ALPHA: f32 = 1.0;

/// The direction-arrow sprite's node width, logical pixels.
const ARROW_W_PX: f32 = 22.0;

/// The direction-arrow sprite's node height, logical pixels (taller than wide — a
/// shaft plus a triangle head, the reference's `direction_arrow`).
const ARROW_H_PX: f32 = 40.0;

/// The arrow's horizontal offset from the beacon's projected point, toward the
/// screen centre, logical pixels — the reference's `ARROW_ELLIPSE_RADIUS_X`
/// (`2 × HUD_ARROW_SIZE`). The arrow sits this far *inside* the beacon and points
/// back out at it.
const ARROW_ELLIPSE_X_PX: f32 = 56.0;

/// The arrow's vertical offset from the beacon's projected point, toward the screen
/// centre — the reference's `ARROW_ELLIPSE_RADIUS_Y` (`HUD_ARROW_SIZE`).
const ARROW_ELLIPSE_Y_PX: f32 = 32.0;

/// How far, in logical pixels, the arrow is kept from the viewport edge so the whole
/// sprite stays on screen (and clears the top menu bar / bottom toolbar).
const ARROW_MARGIN_PX: f32 = 40.0;

/// The z-order of the beacon overlay (label + arrow): above the top / bottom bars
/// (z 9000) so the arrow / label are not hidden behind a toolbar, the reference's
/// always-on-top HUD marker.
const BEACON_OVERLAY_Z: i32 = 9_500;

// ---------------------------------------------------------------------------
// What is tracked, and its colours.
// ---------------------------------------------------------------------------

/// What a beacon is tracking — the render-time classification that picks the beam
/// colours, the label, and the arrow tint. Location, landmark and teleport
/// destinations share the map-track red/blue beam (as the reference does — they are
/// all just a global position); a tracked avatar / friend gets its own colour so it
/// is distinguishable at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeaconKind {
    /// A fixed world location — a map double-click / teleport destination, or a
    /// tracked landmark (all resolve to a global position).
    Location,
    /// A tracked avatar.
    Avatar,
    /// A tracked friend.
    Friend,
}

impl BeaconKind {
    /// The `(below, above)` shaft colours for this kind: the classic red-above /
    /// blue-below map-track beam for a location, or the single tracked-thing colour
    /// (top and bottom) for an avatar / friend.
    const fn beam_colors(self) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::Location => (MAP_TRACK_BELOW, MAP_TRACK_ABOVE),
            Self::Avatar => (AVATAR_TRACK_COLOR, AVATAR_TRACK_COLOR),
            Self::Friend => (FRIEND_TRACK_COLOR, FRIEND_TRACK_COLOR),
        }
    }

    /// The off-screen arrow / label-accent colour for this kind (the reference uses
    /// the beacon's `MapTrackColor` for the arrow — the upper shaft colour here).
    const fn accent_color(self) -> [f32; 3] {
        self.beam_colors().1
    }
}

// ---------------------------------------------------------------------------
// The beam material (unlit, alpha-blended, glow-mask-preserving).
// ---------------------------------------------------------------------------

/// A tiny unlit, alpha-blended material for one beacon-beam shaft: the shaft's RGB
/// tint and base alpha ride the `color` uniform, the soft edge→centre fade rides the
/// mesh's per-vertex colour alpha, and the fragment shader ramps the alpha with
/// camera distance (the reference's `[0.2, 0.5]` beacon alpha). Modelled on
/// [`crate::parcel_borders::ParcelBorderMaterial`].
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub(crate) struct BeaconBeamMaterial {
    /// The shaft's RGB tint (`rgb`) and base alpha (`a`).
    #[uniform(0)]
    color: Vec4,
}

impl Material for BeaconBeamMaterial {
    /// The bundled beam shader carries the mesh through the standard transform.
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(BEACON_BEAM_SHADER_HANDLE)
    }

    /// The bundled beam shader tints by the uniform and ramps alpha with distance.
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(BEACON_BEAM_SHADER_HANDLE)
    }

    /// Alpha-blended: the beam is a translucent coloured overlay, so it sorts in the
    /// transparent phase (depth-tested against the world, no depth write).
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// No depth / normal prepass: the mesh carries only position + colour, and a
    /// translucent overlay belongs in neither prepass.
    fn enable_prepass() -> bool {
        false
    }

    /// The beam casts no shadows (a translucent overlay, not solid geometry).
    fn enable_shadows() -> bool {
        false
    }

    /// Pin the vertex layout to position + colour and draw both faces (the blade is
    /// billboarded, so either face can front the camera), and keep the beam's
    /// coverage out of the scene alpha (the glow mask) so it does not bloom.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(1),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        sl_client_bevy::preserve_glow_mask_alpha(descriptor);
        Ok(())
    }
}

/// The shared beam mesh and the per-colour material cache. The blade mesh is a unit
/// soft-edged quad pair built once; the child shaft entities scale it in place. One
/// material is created per distinct shaft colour and reused.
#[derive(Resource)]
pub(crate) struct BeaconAssets {
    /// The unit blade mesh (soft-edged, `x ∈ [-1, 1]`, `y ∈ [0, 1]`, normal `+Z`).
    blade: Handle<Mesh>,
    /// One shared material per quantised shaft colour.
    materials: HashMap<[u8; 4], Handle<BeaconBeamMaterial>>,
    /// The off-screen chevron texture (a white upward triangle on transparent,
    /// tinted per beacon by the [`ImageNode`] colour).
    arrow_image: Handle<Image>,
}

impl FromWorld for BeaconAssets {
    /// Build the shared unit blade mesh and the chevron texture once.
    fn from_world(world: &mut World) -> Self {
        let blade = world.resource_mut::<Assets<Mesh>>().add(build_blade_mesh());
        let arrow_image = world
            .resource_mut::<Assets<Image>>()
            .add(build_arrow_image());
        Self {
            blade,
            materials: HashMap::default(),
            arrow_image,
        }
    }
}

/// The direction-arrow texture's width, pixels.
const ARROW_TEX_W: u32 = 22;

/// The direction-arrow texture's height, pixels.
const ARROW_TEX_H: u32 = 40;

/// Build the direction-arrow texture: a white upward-pointing arrow — a triangle
/// head over the top half and a straight shaft down the lower half — on a
/// transparent field, anti-aliased with a 3×3 supersample. White so the
/// [`ImageNode`] colour tints it to the beacon accent; the sprite is rotated by
/// `UiTransform` to point at the beacon.
fn build_arrow_image() -> Image {
    let width = ARROW_TEX_W;
    let height = ARROW_TEX_H;
    let wf = f32::from(u16::try_from(width).unwrap_or(u16::MAX));
    let hf = f32::from(u16::try_from(height).unwrap_or(u16::MAX));
    let half_w = wf * 0.5;
    // The top half is the triangle head; the bottom half the shaft.
    let head_h = hf * 0.5;
    let shaft_half = wf * 0.18;
    // Coverage of one pixel by the arrow, via a 3×3 supersample.
    let coverage = |px: u32, py: u32| -> f32 {
        let mut inside = 0_u32;
        let samples = [0.17_f32, 0.5, 0.83];
        for sy in samples {
            for sx in samples {
                let x = f32::from(u16::try_from(px).unwrap_or(u16::MAX)) + sx;
                let y = f32::from(u16::try_from(py).unwrap_or(u16::MAX)) + sy;
                let hit = if y <= head_h {
                    // Head: apex at the top (y = 0, zero width), widening to the full
                    // width at the base of the head.
                    let head_half = (y / head_h) * half_w;
                    (x - half_w).abs() <= head_half
                } else {
                    // Shaft: a straight vertical bar down the centre.
                    (x - half_w).abs() <= shaft_half
                };
                if hit {
                    inside = inside.saturating_add(1);
                }
            }
        }
        f32::from(u16::try_from(inside).unwrap_or(0)) / 9.0
    };
    let mut data: Vec<u8> = Vec::with_capacity(
        usize::try_from(width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height).unwrap_or(0))
            .saturating_mul(4),
    );
    for py in 0..height {
        for px in 0..width {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "coverage is clamped to [0, 1]; ×255 rounded is a whole 0..=255 byte"
            )]
            let alpha = (coverage(px, py).clamp(0.0, 1.0) * 255.0).round() as u8;
            data.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

impl BeaconAssets {
    /// The shared material for a shaft colour, created on first use. The colour is
    /// quantised to bytes for the cache key (a handful of distinct beacon colours).
    fn material_for(
        &mut self,
        color: [f32; 3],
        alpha: f32,
        materials: &mut Assets<BeaconBeamMaterial>,
    ) -> Handle<BeaconBeamMaterial> {
        let [r, g, b] = color;
        let key = [quantise(r), quantise(g), quantise(b), quantise(alpha)];
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let handle = materials.add(BeaconBeamMaterial {
            color: Vec4::new(r, g, b, alpha),
        });
        self.materials.insert(key, handle.clone());
        handle
    }
}

/// Quantise a `0..=1` colour / alpha component to a byte for the material cache key.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a clamped 0..=255 value is whole after rounding; only used as a HashMap key"
)]
fn quantise(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Build the unit blade mesh: two quads sharing a centre spine, in the local XY
/// plane (normal `+Z`), `x ∈ [-1, 1]`, `y ∈ [0, 1]`. The per-vertex colour alpha is
/// `1` on the centre spine and `0` on the two side edges — the reference's bright
/// core / transparent edge, giving a soft-edged glowing blade. The RGB is unused
/// (the tint comes from the material uniform).
fn build_blade_mesh() -> Mesh {
    // Four columns: the two transparent side edges (`x = ±1`) and a solid opaque core
    // band (`x = ±CORE`), so the beam reads as a bright wide shaft with soft edges
    // rather than a single hair-thin spine. Each column has a bottom and top vertex.
    /// The half-width, in blade units, of the fully-opaque core band (the rest fades
    /// to transparent at the `±1` edges).
    const CORE: f32 = 0.45;
    let positions: Vec<[f32; 3]> = vec![
        [-1.0, 0.0, 0.0],  // 0 left edge  bottom
        [-1.0, 1.0, 0.0],  // 1 left edge  top
        [-CORE, 0.0, 0.0], // 2 core left  bottom
        [-CORE, 1.0, 0.0], // 3 core left  top
        [CORE, 0.0, 0.0],  // 4 core right bottom
        [CORE, 1.0, 0.0],  // 5 core right top
        [1.0, 0.0, 0.0],   // 6 right edge bottom
        [1.0, 1.0, 0.0],   // 7 right edge top
    ];
    let edge = [1.0, 1.0, 1.0, 0.0];
    let core = [1.0, 1.0, 1.0, 1.0];
    let colors: Vec<[f32; 4]> = vec![edge, edge, core, core, core, core, edge, edge];
    // Three quads: left fade (0-2), opaque core (2-4), right fade (4-6). Wound so
    // either face draws (the material disables culling anyway).
    let indices = vec![
        0, 2, 3, 0, 3, 1, // left fade
        2, 4, 5, 2, 5, 3, // opaque core
        4, 6, 7, 4, 7, 5, // right fade
    ];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ---------------------------------------------------------------------------
// Resolving the tracked target to a world position + kind + label.
// ---------------------------------------------------------------------------

/// A resolved, renderable beacon: where it is in the Bevy scene, what it is (colour
/// coding), its label, and its distance from the agent (metres). Recomputed each
/// frame from [`MapTracking`] and stored for the overlay system to consume.
#[derive(Debug, Clone)]
struct ActiveBeacon {
    /// The target's Bevy world position (the seam between the two shafts).
    position: Vec3,
    /// What is tracked (beam / arrow colours, label accent).
    kind: BeaconKind,
    /// The label's first line (the tracked thing's name).
    name: String,
    /// The agent→target distance, metres (the reference measures the beacon
    /// distance from the agent, not the camera).
    distance: f32,
}

/// The beacon renderer's persistent state: the spawned beam entities and the
/// currently rendered beacon (for the overlay system and to avoid re-tinting the
/// shafts every frame).
#[derive(Resource, Default)]
pub(crate) struct BeaconState {
    /// The billboarded beam root entity, spawned lazily on the first beacon.
    root: Option<Entity>,
    /// The lower (below-target) shaft child.
    below: Option<Entity>,
    /// The upper (above-target) shaft child.
    above: Option<Entity>,
    /// The kind the shafts are currently tinted for (so the material handles are
    /// only swapped when the kind changes).
    tinted_kind: Option<BeaconKind>,
    /// The beacon rendered this frame, or `None` when nothing is tracked.
    resolved: Option<ActiveBeacon>,
}

/// Narrow a global-metre `f64` to the `f32` the scene works in (global metres stay
/// well inside `f32`'s exact-integer range once the region origin is subtracted).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "f64 → f32 narrowing at the coordinate boundary; the value is a bounded world \
              coordinate and std has no lossless conversion for it"
)]
const fn narrow(metres: f64) -> f32 {
    metres as f32
}

/// Convert a tracked **location** (global metres east / north, altitude) into Bevy
/// world space, using the scene origin region's south-west corner (the same anchor
/// the terrain and coarse avatars use). Returns [`None`] until an origin is known.
fn location_to_bevy(east: f64, north: f64, up: f32, origin: Option<RegionHandle>) -> Option<Vec3> {
    let origin = origin?;
    let (corner_east, corner_north) = origin.global_coordinates();
    let local = Vector {
        x: narrow(east) - metres_to_f32(corner_east),
        y: narrow(north) - metres_to_f32(corner_north),
        z: up,
    };
    Some(sl_to_bevy_vec(&local))
}

/// The lengths of the two beam shafts for a target at Bevy height `target_y`: the
/// lower shaft runs from the ground (`y = 0`) up to the target, the upper shaft from
/// the target up to [`BEAM_TOP_M`]. Both clamped non-negative so an underground /
/// sky-high target still yields a valid (possibly zero-length) shaft.
fn shaft_lengths(target_y: f32) -> (f32, f32) {
    let below = target_y.max(0.0);
    let above = (BEAM_TOP_M - target_y).max(0.0);
    (below, above)
}

/// The billboard yaw (about Bevy up) that turns a blade authored in the local XY
/// plane (normal `+Z`) to face the camera horizontally: the blade's normal points at
/// the camera's ground projection. Returns `0` when the camera is directly above the
/// beam (no horizontal direction).
fn billboard_yaw(beam: Vec3, camera: Vec3) -> f32 {
    let dx = camera.x - beam.x;
    let dz = camera.z - beam.z;
    if dx.abs() < f32::EPSILON && dz.abs() < f32::EPSILON {
        0.0
    } else {
        dx.atan2(dz)
    }
}

// ---------------------------------------------------------------------------
// The beam system.
// ---------------------------------------------------------------------------

/// Resolve the tracked target and drive the world-space beam: (re)spawn the beam
/// root and its two shaft children, place them at the target, billboard them toward
/// the camera, tint them by what is tracked, and hide them when nothing is tracked.
/// Also records the resolved beacon in [`BeaconState`] for the overlay system.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tracking \
              source, the identity / avatar / friend / terrain resolvers, the shared assets and \
              material store, this feature's state, the camera and transform queries, and the \
              command buffer to spawn the beam"
)]
fn update_beacon_beam(
    tracking: Res<MapTracking>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    friends: Option<Res<FriendsModel>>,
    terrain: Res<TerrainState>,
    mut assets: ResMut<BeaconAssets>,
    mut materials: ResMut<Assets<BeaconBeamMaterial>>,
    mut state: ResMut<BeaconState>,
    cameras: Query<&GlobalTransform, With<ViewerCamera>>,
    globals: Query<&GlobalTransform>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
    mut commands: Commands,
) {
    let origin = terrain.origin().or(identity.region_handle);
    let resolved = resolve_beacon(
        &tracking,
        &identity,
        &avatars,
        friends.as_deref(),
        &globals,
        origin,
    );

    let Ok(camera_transform) = cameras.single() else {
        state.resolved = resolved;
        return;
    };

    let Some(beacon) = resolved else {
        // Nothing tracked: hide the beam root (if spawned) and forget the tint.
        state.resolved = None;
        if let Some(root) = state.root
            && let Ok((_transform, mut visibility)) = transforms.get_mut(root)
        {
            visibility.set_if_neq(Visibility::Hidden);
        }
        state.tinted_kind = None;
        return;
    };

    // Spawn the beam rig on first need: a billboarded root with two shaft children.
    if state.root.is_none() {
        let root = commands
            .spawn((
                Transform::default(),
                Visibility::Hidden,
                Propagate(tag_render_layers()),
                bevy::camera::visibility::NoFrustumCulling,
                Name::new("beacon-beam"),
            ))
            .id();
        let below = commands
            .spawn((
                Mesh3d(assets.blade.clone()),
                Transform::default(),
                Visibility::Inherited,
                bevy::camera::visibility::NoFrustumCulling,
                ChildOf(root),
            ))
            .id();
        let above = commands
            .spawn((
                Mesh3d(assets.blade.clone()),
                Transform::default(),
                Visibility::Inherited,
                bevy::camera::visibility::NoFrustumCulling,
                ChildOf(root),
            ))
            .id();
        state.root = Some(root);
        state.below = Some(below);
        state.above = Some(above);
    }

    // Re-tint the shafts only when the tracked kind changes (a material swap).
    if state.tinted_kind != Some(beacon.kind) {
        let (below_color, above_color) = beacon.kind.beam_colors();
        let below_material = assets.material_for(below_color, BEAM_BASE_ALPHA, &mut materials);
        let above_material = assets.material_for(above_color, BEAM_BASE_ALPHA, &mut materials);
        if let Some(below) = state.below {
            commands
                .entity(below)
                .insert(MeshMaterial3d(below_material));
        }
        if let Some(above) = state.above {
            commands
                .entity(above)
                .insert(MeshMaterial3d(above_material));
        }
        state.tinted_kind = Some(beacon.kind);
    }

    let (below_len, above_len) = shaft_lengths(beacon.position.y);
    let yaw = billboard_yaw(beacon.position, camera_transform.translation());

    // Place + orient the root at the target, billboarded to face the camera.
    if let Some(root) = state.root
        && let Ok((mut transform, mut visibility)) = transforms.get_mut(root)
    {
        transform.translation = beacon.position;
        transform.rotation = Quat::from_rotation_y(yaw);
        visibility.set_if_neq(Visibility::Visible);
    }
    // Lower shaft: from the target down to the ground.
    if let Some(below) = state.below
        && let Ok((mut transform, _visibility)) = transforms.get_mut(below)
    {
        transform.translation = Vec3::new(0.0, -below_len, 0.0);
        transform.scale = Vec3::new(BEAM_HALF_WIDTH_M, below_len, 1.0);
    }
    // Upper shaft: from the target up to the sky ceiling.
    if let Some(above) = state.above
        && let Ok((mut transform, _visibility)) = transforms.get_mut(above)
    {
        transform.translation = Vec3::ZERO;
        transform.scale = Vec3::new(BEAM_HALF_WIDTH_M, above_len, 1.0);
    }

    // Record the beacon for the overlay system (moved back after use).
    state.resolved = Some(beacon);
}

/// Resolve the current [`MapTracking`] target into a renderable [`ActiveBeacon`], or
/// [`None`] when nothing is tracked (or the target cannot be placed yet). A location
/// resolves via the scene origin; an avatar follows its live in-world entity and is
/// classified friend vs. plain avatar. The label distance is measured from the
/// agent's own in-world position (the reference's beacon distance), falling back to
/// the target itself (distance 0) before the agent is placed.
fn resolve_beacon(
    tracking: &MapTracking,
    identity: &SlIdentity,
    avatars: &AvatarState,
    friends: Option<&FriendsModel>,
    globals: &Query<&GlobalTransform>,
    origin: Option<RegionHandle>,
) -> Option<ActiveBeacon> {
    let (position, kind, name) = match tracking.target? {
        TrackTarget::Location { east, north, up } => {
            let position = location_to_bevy(east, north, up, origin)?;
            (position, BeaconKind::Location, location_label(east, north))
        }
        TrackTarget::Avatar(agent) => {
            let entity = avatars.root_entity_of(agent)?;
            let position = globals.get(entity).ok()?.translation();
            let kind = if friends.is_some_and(|model| model.is_friend(agent)) {
                BeaconKind::Friend
            } else {
                BeaconKind::Avatar
            };
            let name = avatars
                .name_of(agent)
                .map_or_else(|| avatar_fallback_label(agent), str::to_owned);
            (position, kind, name)
        }
    };

    // Agent→target distance (the reference measures the beacon distance from the
    // agent). Before the agent is placed, fall back to zero.
    let distance = identity
        .agent_id
        .and_then(|own| avatars.root_entity_of(own))
        .and_then(|entity| globals.get(entity).ok())
        .map_or(0.0, |own| own.translation().distance(position));

    Some(ActiveBeacon {
        position,
        kind,
        name,
        distance,
    })
}

/// The label for a tracked location without a resolved name — its region-relative
/// grid position, mirroring the reference's `Region (x, y)` style.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a within-region metre coordinate (0..256) is whole, non-negative and tiny after \
              the Euclidean modulo"
)]
fn location_label(east: f64, north: f64) -> String {
    let x = east.rem_euclid(256.0) as u32;
    let y = north.rem_euclid(256.0) as u32;
    format!("Location ({x}, {y})")
}

/// The fallback label for a tracked avatar whose name has not resolved yet.
fn avatar_fallback_label(agent: AgentKey) -> String {
    format!("Avatar {}", agent.uuid())
}

// ---------------------------------------------------------------------------
// The screen-space overlay: label + off-screen direction arrow.
// ---------------------------------------------------------------------------

/// The lazily-spawned overlay nodes (parented under the UI root): the label and its
/// text child, and the off-screen direction chevron.
#[derive(Resource, Default)]
pub(crate) struct BeaconOverlay {
    /// The label node (a small backdrop) and its text child.
    label: Option<(Entity, Entity)>,
    /// The off-screen direction chevron node.
    arrow: Option<Entity>,
}

/// A marker on the beacon label node.
#[derive(Component)]
struct BeaconLabelNode;

/// A marker on the beacon arrow node.
#[derive(Component)]
struct BeaconArrowNode;

/// Drive the screen-space overlay from the beacon resolved in
/// [`update_beacon_beam`]: project the target to the viewport and either show the
/// label pinned to it (on-screen) or the direction chevron on the viewport edge
/// pointing toward it (off-screen). Hides both when nothing is tracked.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the beacon state, \
              the lazily-spawned overlay bookkeeping, the UI root to parent under, the camera to \
              project with, and the node / text / colour / visibility / transform queries that \
              place the label and arrow"
)]
fn update_beacon_overlay(
    state: Res<BeaconState>,
    mut overlay: ResMut<BeaconOverlay>,
    assets: Res<BeaconAssets>,
    root: Option<Res<UiRoot>>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    mut arrow_images: Query<&mut ImageNode>,
    mut visibilities: Query<&mut Visibility>,
    mut ui_transforms: Query<&mut UiTransform>,
    mut commands: Commands,
    mut last_log: Local<i8>,
) {
    /// One-shot per-branch diagnostic (`SL_VIEWER_LOG_BEACON`): logs when the
    /// overlay flips between hidden / label / arrow, with the projected screen point.
    fn log_state(last: &mut i8, now: i8, detail: &str) {
        if *last != now && std::env::var_os("SL_VIEWER_LOG_BEACON").is_some() {
            info!("beacon overlay -> {detail}");
        }
        *last = now;
    }

    // Ensure the overlay nodes exist (spawned once under the UI root).
    let Some(root) = root.map(|root| root.0) else {
        return;
    };
    ensure_overlay(&mut overlay, root, &assets.arrow_image, &mut commands);

    let Some(beacon) = state.resolved.as_ref() else {
        log_state(&mut last_log, 0, "hidden (nothing tracked)");
        hide_overlay(&overlay, &mut visibilities);
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        log_state(&mut last_log, 0, "hidden (no single viewer camera)");
        hide_overlay(&overlay, &mut visibilities);
        return;
    };
    let Some(viewport) = camera.logical_viewport_size() else {
        log_state(&mut last_log, 0, "hidden (no viewport size)");
        hide_overlay(&overlay, &mut visibilities);
        return;
    };

    // View-space position: `z < 0` is in front of the camera (Bevy looks down -Z).
    let view = camera_transform.affine().inverse();
    let view_pos = view.transform_point3(beacon.position);
    let in_front = view_pos.z < 0.0;
    let on_screen = in_front
        .then(|| {
            camera
                .world_to_viewport(camera_transform, beacon.position)
                .ok()
        })
        .flatten()
        .filter(|screen| {
            screen.x >= 0.0 && screen.y >= 0.0 && screen.x <= viewport.x && screen.y <= viewport.y
        });

    // The beacon's projected screen point (the red/blue seam at the tracked
    // altitude): its actual projection when on-screen, else the clamped
    // viewport-edge point along the view-space direction (so a behind / off-screen
    // beacon still gives the arrow a sensible anchor to point out from).
    let seam_screen = match on_screen {
        Some(screen) => screen,
        None => edge_point(
            Vec2::new(view_pos.x, -view_pos.y),
            viewport,
            ARROW_MARGIN_PX,
        ),
    };

    // The label follows the seam, but only while it is genuinely on-screen.
    match on_screen {
        Some(screen) => {
            log_state(
                &mut last_log,
                1,
                &format!(
                    "label + arrow, seam ({:.0}, {:.0}) / viewport {viewport:?}",
                    screen.x, screen.y
                ),
            );
            show_label(
                &overlay,
                beacon,
                screen,
                viewport,
                &mut nodes,
                &mut texts,
                &mut visibilities,
            );
        }
        None => {
            log_state(
                &mut last_log,
                2,
                &format!("arrow only (off-screen), in_front={in_front} view_pos={view_pos:?}"),
            );
            set_visibility_pair(overlay.label, Visibility::Hidden, &mut visibilities);
        }
    }

    // The arrow is always shown: it sits on a small ellipse around the seam, toward
    // the screen centre, and points back out at the beacon (the reference's
    // `LLTracker::drawMarker`). So it points up when the camera is below the beacon
    // altitude, down when above, sideways when level.
    show_arrow(
        &overlay,
        beacon.kind,
        seam_screen,
        viewport,
        &mut nodes,
        &mut arrow_images,
        &mut ui_transforms,
        &mut visibilities,
    );
}

/// Spawn the overlay label + arrow nodes once, under the UI root.
fn ensure_overlay(
    overlay: &mut BeaconOverlay,
    root: Entity,
    arrow_image: &Handle<Image>,
    commands: &mut Commands,
) {
    if overlay.label.is_none() {
        let node = commands
            .spawn((
                BeaconLabelNode,
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    ..Default::default()
                },
                BackgroundColor(Color::srgba(0.02, 0.02, 0.04, 0.6)),
                GlobalZIndex(BEACON_OVERLAY_Z),
                Pickable::IGNORE,
                Visibility::Hidden,
                Name::new("beacon-label"),
                ChildOf(root),
            ))
            .id();
        let text = commands
            .spawn((
                Text::default(),
                UiFont::Sans.at(13.0),
                TextColor(Color::WHITE),
                Pickable::IGNORE,
                ChildOf(node),
            ))
            .id();
        overlay.label = Some((node, text));
    }
    if overlay.arrow.is_none() {
        // A tinted arrow sprite (shaft + triangle head), rotated by `UiTransform` to
        // point at the beacon (the reference's `direction_arrow`). Built as an image
        // rather than a CSS-border triangle, which bevy_ui does not miter. Clickable
        // to dismiss the beacon (the reference's `handleMouseDown` — the arrow
        // doubles as the stop-tracking button).
        let arrow = commands
            .spawn((
                BeaconArrowNode,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ARROW_W_PX),
                    height: Val::Px(ARROW_H_PX),
                    ..Default::default()
                },
                ImageNode::new(arrow_image.clone()),
                GlobalZIndex(BEACON_OVERLAY_Z),
                Visibility::Hidden,
                Name::new("beacon-arrow"),
                ChildOf(root),
            ))
            .observe(on_beacon_arrow_click)
            .id();
        overlay.arrow = Some(arrow);
    }
}

/// Click-to-dismiss on the direction arrow (the reference's
/// `LLTracker::handleMouseDown`): clicking the arrow stops tracking, clearing the
/// beacon — the arrow doubles as the stop-tracking button.
fn on_beacon_arrow_click(mut click: On<Pointer<Click>>, mut tracking: ResMut<MapTracking>) {
    if click.button == PointerButton::Primary {
        tracking.target = None;
        click.propagate(false);
    }
}

/// Hide every overlay node (nothing tracked, or no camera / viewport).
fn hide_overlay(overlay: &BeaconOverlay, visibilities: &mut Query<&mut Visibility>) {
    set_visibility_pair(overlay.label, Visibility::Hidden, visibilities);
    set_visibility(overlay.arrow, Visibility::Hidden, visibilities);
}

/// Set one optional node's visibility (compare-then-assign).
fn set_visibility(
    node: Option<Entity>,
    value: Visibility,
    visibilities: &mut Query<&mut Visibility>,
) {
    if let Some(node) = node
        && let Ok(mut visibility) = visibilities.get_mut(node)
    {
        visibility.set_if_neq(value);
    }
}

/// Set a label `(node, text)` pair's (node) visibility.
fn set_visibility_pair(
    pair: Option<(Entity, Entity)>,
    value: Visibility,
    visibilities: &mut Query<&mut Visibility>,
) {
    set_visibility(pair.map(|(node, _text)| node), value, visibilities);
}

/// Show the on-screen label pinned above the target's projected point, clamped so
/// it stays fully within the viewport (a target near the top edge would otherwise
/// push the lifted label off the screen).
fn show_label(
    overlay: &BeaconOverlay,
    beacon: &ActiveBeacon,
    screen: Vec2,
    viewport: Vec2,
    nodes: &mut Query<&mut Node>,
    texts: &mut Query<&mut Text>,
    visibilities: &mut Query<&mut Visibility>,
) {
    let Some((node, text_entity)) = overlay.label else {
        return;
    };
    let content = format!("{}\n{:.0} m", beacon.name, beacon.distance);
    if let Ok(mut text) = texts.get_mut(text_entity)
        && text.0 != content
    {
        text.0 = content;
    }
    if let Ok(mut layout) = nodes.get_mut(node) {
        // Pin the label just above the target's screen point (a small lift so the
        // beam seam is not hidden), offset left so the block roughly centres, then
        // clamp into the viewport so it is never pushed off an edge. The clamp
        // upper bounds leave rough room for the label's own size.
        let left = (screen.x - 24.0).clamp(4.0, (viewport.x - 120.0).max(4.0));
        let top = (screen.y - 34.0).clamp(4.0, (viewport.y - 40.0).max(4.0));
        let left = Val::Px(left);
        let top = Val::Px(top);
        if layout.left != left {
            layout.left = left;
        }
        if layout.top != top {
            layout.top = top;
        }
    }
    set_visibility(Some(node), Visibility::Visible, visibilities);
}

/// Show the direction arrow near the beacon's projected `seam_screen` point,
/// pointing at it (the reference's `LLTracker::drawMarker`).
#[expect(
    clippy::too_many_arguments,
    reason = "the arrow placement needs the overlay handle, the beacon kind (colour), the \
              projected seam and viewport to place it, and the node / image-tint / transform / \
              visibility queries that position, tint and rotate it"
)]
fn show_arrow(
    overlay: &BeaconOverlay,
    kind: BeaconKind,
    seam_screen: Vec2,
    viewport: Vec2,
    nodes: &mut Query<&mut Node>,
    arrow_images: &mut Query<&mut ImageNode>,
    ui_transforms: &mut Query<&mut UiTransform>,
    visibilities: &mut Query<&mut Visibility>,
) {
    let Some(arrow) = overlay.arrow else {
        return;
    };
    let (center, angle) = arrow_marker_placement(seam_screen, viewport);
    let [r, g, b] = kind.accent_color();
    let color = Color::srgb(r, g, b);
    if let Ok(mut image) = arrow_images.get_mut(arrow)
        && image.color != color
    {
        image.color = color;
    }
    if let Ok(mut layout) = nodes.get_mut(arrow) {
        // The node's box centre sits at `center`; offset by the half-extents so the
        // arrow centres on the placement point.
        let left = Val::Px(center.x - ARROW_W_PX * 0.5);
        let top = Val::Px(center.y - ARROW_H_PX * 0.5);
        if layout.left != left {
            layout.left = left;
        }
        if layout.top != top {
            layout.top = top;
        }
    }
    if let Ok(mut transform) = ui_transforms.get_mut(arrow) {
        let rotation = Rot2::radians(angle);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
    }
    set_visibility(Some(arrow), Visibility::Visible, visibilities);
}

/// Place the direction arrow around the beacon's projected `seam_screen` point (the
/// reference's `LLTracker::drawMarker`): on an ellipse of radii
/// ([`ARROW_ELLIPSE_X_PX`], [`ARROW_ELLIPSE_Y_PX`]) *inside* the seam toward the
/// screen centre, rotated to point back out at the seam, and clamped to the
/// viewport. Returns the arrow's centre and its rotation. `seam_screen` and the
/// result are in the window's y-down screen space.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "finite pixel-space viewport geometry; the glam / float operators are the readable form"
)]
fn arrow_marker_placement(seam_screen: Vec2, viewport: Vec2) -> (Vec2, f32) {
    let center = viewport * 0.5;
    let to_seam = seam_screen - center;
    let dist = to_seam.length();
    // The outward unit direction (centre → beacon); a beacon dead-centre defaults to
    // pointing up so the arrow still has a stable orientation.
    let outward = if dist > f32::EPSILON {
        to_seam / dist
    } else {
        Vec2::new(0.0, -1.0)
    };
    // Pull back from the seam toward the centre by the ellipse radii.
    let position = Vec2::new(
        seam_screen.x - ARROW_ELLIPSE_X_PX * outward.x,
        seam_screen.y - ARROW_ELLIPSE_Y_PX * outward.y,
    );
    // Keep the whole sprite on screen (and clear of the toolbars).
    let lo = Vec2::splat(ARROW_MARGIN_PX);
    let hi = (viewport - lo).max(lo);
    let position = position.clamp(lo, hi);
    (position, arrow_angle(outward))
}

/// The point on the viewport rectangle (inset by `margin`) along the ray from the
/// centre in screen direction `dir` — where an off-screen beacon's arrow anchors.
/// `dir` is in the window's y-down screen space.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "finite pixel-space viewport geometry; the glam / float operators are the readable form"
)]
fn edge_point(dir: Vec2, viewport: Vec2, margin: f32) -> Vec2 {
    let center = viewport * 0.5;
    let dir = dir.normalize_or_zero();
    // Distance from centre to the inset rectangle edge along `dir`: the smaller of
    // the two axis intercepts (guarding a near-zero component).
    let half = (center - Vec2::splat(margin)).max(Vec2::splat(0.0));
    let tx = if dir.x.abs() > f32::EPSILON {
        half.x / dir.x.abs()
    } else {
        f32::INFINITY
    };
    let ty = if dir.y.abs() > f32::EPSILON {
        half.y / dir.y.abs()
    } else {
        f32::INFINITY
    };
    let t = tx.min(ty);
    let t = if t.is_finite() { t } else { 0.0 };
    center + dir * t
}

/// The `UiTransform` rotation (radians) that turns the chevron — authored pointing
/// up (screen `-y`) — to point along screen direction `dir`. Rotating the up vector
/// `(0, -1)` by this angle yields `dir`.
fn arrow_angle(dir: Vec2) -> f32 {
    if dir.length_squared() < f32::EPSILON {
        0.0
    } else {
        dir.to_angle() + core::f32::consts::FRAC_PI_2
    }
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The tracking-beacon plugin: loads the beam shader, registers the beam material,
/// and runs the beam + overlay systems (the overlay after the beam, so it reads the
/// beacon the beam resolved this frame).
#[derive(Debug, Default)]
pub(crate) struct BeaconPlugin;

impl Plugin for BeaconPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            BEACON_BEAM_SHADER_HANDLE,
            "beacon_beam.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<BeaconBeamMaterial>::default())
            .init_resource::<BeaconAssets>()
            .init_resource::<BeaconState>()
            .init_resource::<BeaconOverlay>()
            .add_systems(Update, (update_beacon_beam, update_beacon_overlay).chain());
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{
        BEAM_TOP_M, BeaconKind, arrow_angle, billboard_yaw, edge_point, location_to_bevy,
        shaft_lengths,
    };
    use bevy::math::Vec2;
    use bevy::prelude::Vec3;

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// Whether two `[f32; 3]` colours match component-wise within a hair (the
    /// workspace forbids `==` / `!=` on float arrays).
    fn colors_equal(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter()
            .zip(b)
            .all(|(left, right)| (left - right).abs() < 1.0e-6)
    }

    /// The two shafts split at the target: the lower shaft is the target height, the
    /// upper reaches the sky ceiling, and both are clamped non-negative.
    #[test]
    fn shafts_split_at_the_target() {
        let (below, above) = shaft_lengths(30.0);
        near(below, 30.0, 1e-4);
        near(above, BEAM_TOP_M - 30.0, 1e-3);
        // An underground target has no lower shaft.
        let (below, above) = shaft_lengths(-5.0);
        near(below, 0.0, 1e-4);
        near(above, BEAM_TOP_M + 5.0, 1e-3);
    }

    /// A location resolves to Bevy world space relative to the origin corner; an
    /// unknown origin yields nothing.
    #[test]
    fn location_resolves_against_the_origin() {
        assert!(location_to_bevy(1_000_100.0, 2_000_050.0, 25.0, None).is_none());
        // Origin region south-west corner at (1_000_000, 2_000_000) global metres.
        let origin = sl_client_bevy::RegionHandle::new((1_000_000_u64 << 32) | 2_000_000_u64);
        let placed = location_to_bevy(1_000_100.0, 2_000_050.0, 25.0, Some(origin));
        // 100 m east, 50 m north, 25 m up → Bevy (100, 25, -50).
        let placed = placed.expect("a known origin places the location");
        near(placed.x, 100.0, 1e-2);
        near(placed.y, 25.0, 1e-2);
        near(placed.z, -50.0, 1e-2);
    }

    /// The billboard yaw turns the blade normal (`+Z`) toward the camera's ground
    /// projection: a camera due Bevy `+Z` of the beam yaws to `0`, one due `+X` to a
    /// quarter turn.
    #[test]
    fn billboard_faces_the_camera() {
        near(
            billboard_yaw(Vec3::ZERO, Vec3::new(0.0, 3.0, 10.0)),
            0.0,
            1e-4,
        );
        near(
            billboard_yaw(Vec3::ZERO, Vec3::new(10.0, 3.0, 0.0)),
            core::f32::consts::FRAC_PI_2,
            1e-4,
        );
        // Camera straight above → no horizontal direction → yaw 0.
        near(
            billboard_yaw(Vec3::ZERO, Vec3::new(0.0, 50.0, 0.0)),
            0.0,
            1e-4,
        );
    }

    /// The chevron rotation carries the authored up-vector `(0, -1)` onto the target
    /// direction: pointing right is a quarter turn, pointing up is no turn.
    #[test]
    fn arrow_points_along_the_direction() {
        // Rotating (0, -1) by `arrow_angle(right)` should yield (1, 0).
        let angle = arrow_angle(Vec2::new(1.0, 0.0));
        let (sin, cos) = angle.sin_cos();
        let rotated = Vec2::new(sin, -cos);
        near(rotated.x, 1.0, 1e-4);
        near(rotated.y, 0.0, 1e-4);
        // Straight up needs no rotation.
        near(arrow_angle(Vec2::new(0.0, -1.0)), 0.0, 1e-4);
    }

    /// The off-screen edge point lands on the inset rectangle boundary: a target
    /// straight right pins the anchor to the right inset edge at mid-height.
    #[test]
    fn edge_point_hits_the_inset_edge() {
        let viewport = Vec2::new(800.0, 600.0);
        let position = edge_point(Vec2::new(1.0, 0.0), viewport, 24.0);
        near(position.x, 800.0 - 24.0, 1e-3);
        near(position.y, 300.0, 1e-3);
    }

    /// The `drawMarker` arrow sits inside the beacon toward the screen centre and
    /// points back out at it: a beacon straight above the centre puts the arrow
    /// below the beacon (pulled toward centre by the vertical ellipse radius) and
    /// pointing up. A beacon below the centre points the arrow down.
    #[test]
    fn marker_points_at_the_beacon() {
        use super::{ARROW_ELLIPSE_Y_PX, arrow_marker_placement};
        let viewport = Vec2::new(800.0, 600.0);
        // Beacon 200 px above the centre (y-down: smaller y is higher).
        let seam = Vec2::new(400.0, 100.0);
        let (position, angle) = arrow_marker_placement(seam, viewport);
        // Pulled down toward centre by the vertical ellipse radius.
        near(position.x, 400.0, 1e-3);
        near(position.y, 100.0 + ARROW_ELLIPSE_Y_PX, 1e-3);
        // Rotating the authored up-vector (0,-1) by `angle` should keep it pointing
        // up (toward the beacon above).
        let (sin, cos) = angle.sin_cos();
        let rotated = Vec2::new(sin, -cos);
        near(rotated.x, 0.0, 1e-3);
        near(rotated.y, -1.0, 1e-3);
    }

    /// The colour coding differs by tracked kind: a location gets the red/blue
    /// map-track beam, an avatar and a friend each a distinct single colour.
    #[test]
    fn kinds_have_distinct_colours() {
        let (below, above) = BeaconKind::Location.beam_colors();
        assert!(!colors_equal(below, above), "a location's beam is two-tone");
        let (av_below, av_above) = BeaconKind::Avatar.beam_colors();
        assert!(
            colors_equal(av_below, av_above),
            "an avatar's beam is one colour"
        );
        assert!(
            !colors_equal(
                BeaconKind::Avatar.accent_color(),
                BeaconKind::Friend.accent_color()
            ),
            "avatar and friend accents differ"
        );
    }
}
