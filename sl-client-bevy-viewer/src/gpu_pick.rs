//! **GPU ID-buffer picking** (`roadmap/context/gpu-avatars.md` §6, Phase 3):
//! the cursor pick is a render, not a ray cast.
//!
//! A tiny offscreen **pick view** — the main camera's frustum cropped by
//! projection to a [`CROP_SIZE`]² logical-pixel square around the cursor —
//! renders every pick-tagged mesh under the cursor into an `Rgba32Uint` ID
//! target (tag, fragment depth bits, submission sequence) over a
//! `Depth32Float` depth test, with **two shader variants**: static meshes by
//! their `GlobalTransform`, skinned meshes through the very
//! `SkinUniforms.current_buffer` palettes the visible pass consumes (GPU-posed
//! avatars are therefore picked **exactly where they are drawn**, morphs and
//! physics included — the fix for the Phase 1b rest-pose pick regression).
//! [`bevy::render::gpu_readback::Readback`] lifts the ID target back 1–2
//! frames later; the centre texel resolves through the [`PickRegistry`] to an
//! avatar / object face / terrain / water, and the depth unprojects through
//! the **submitting frame's** matrices (kept per in-flight pick) to the world
//! hit point.
//!
//! Pick identity rides [`MeshTag`] — `class:4 bits | index:28 bits`
//! ([`encode_pick_tag`]) — assigned at spawn by the `assign_*` systems (avatar
//! submeshes via [`AvatarPickTarget`], prim faces via [`PrimFaceEntity`],
//! terrain patches, water planes) and freed on despawn. Name tags keep their
//! own `MeshTag` meaning (the billboard atlas channel) and are simply never
//! pick-tagged; HUD attachments are excluded from the pick view by their HUD
//! [`RenderLayers`] and keep the [`crate::hud_pick`] orthographic test.
//!
//! Consumers ([`GpuPicker::request`] → [`GpuPickResolved`]): the hover
//! tooltip (dwell-gated, ~[`PICK_HZ`] Hz), the left-click touch, the
//! right-click context-menu resolver, the double-click teleport, the
//! inventory-drag world drop, and the debug pick inspector. Non-cursor ray
//! casts (edit-tool axis rays, camera collision, the arm-reach probe, the
//! crosshair `P` diagnostic) deliberately stay on `MeshRayCast`.
//!
//! The render-world half (pipelines, prepare, the pass) lives in
//! [`render`]; this module is the main-world registry, queue and resolve.
//! [`collect_pick_warm_set`] extracts every currently pickable mesh layout
//! each frame (not only while a pick is active) so
//! [`render::warm_gpu_pick_pipelines`] can start compiling both pipeline
//! variants as avatars/prims rez — the first real pick then finds them
//! ready instead of missing while compilation catches up.

pub(crate) mod render;

use std::collections::{HashMap, HashSet};

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::primitives::Frustum;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::math::Affine3A;
use bevy::mesh::MeshTag;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::TextureUsages;
use bevy::render::texture::GpuImage;

use sl_client_bevy::{AgentKey, PrimFaceId, ScopedObjectId};

use crate::avatars::AvatarPickTarget;
use crate::camera::ViewerCamera;
use crate::hud::on_hud_layer;
use crate::objects::{PrimFaceEntity, SceneObject, WornPickTarget};
use crate::terrain::TerrainSurface;
use crate::water::{WaterOcean, WaterRegionPlane};

/// The internal handle `pick.wgsl` is loaded under.
const PICK_SHADER_HANDLE: Handle<Shader> = uuid_handle!("3f5d1a82-6c47-49b3-8e90-b21f7c04a6de");

/// The pick view's square crop, in logical pixels per side (and texels per
/// side of the ID target). The centre texel is the cursor pixel; the margin
/// exists so the projection crop collapses frustum culling to the handful of
/// entities around the cursor.
pub(crate) const CROP_SIZE: u32 = 9;

/// [`CROP_SIZE`] as the f32 the crop matrix uses.
const CROP_PIXELS: f32 = 9.0;

/// The centre texel index of the crop (both axes).
const CROP_CENTRE: u32 = 4;

/// Bytes per `Rgba32Uint` texel of the ID target.
const ID_TEXEL_BYTES: usize = 16;

/// The repeat-pick cadence continuous consumers (hover, drag, the inspector)
/// request at, in Hz.
pub(crate) const PICK_HZ: f32 = 15.0;

/// Frames after which an unanswered in-flight pick is delivered as a miss and
/// dropped (readbacks normally answer within 2–3 frames; this only fires when
/// the render side never ran, e.g. a still-compiling pipeline at startup).
const PICK_TIMEOUT_FRAMES: u32 = 120;

/// The most in-flight picks kept; the oldest is delivered as a miss beyond
/// this (requests arrive at most once per frame, readbacks answer in 2–3).
const PICK_PENDING_CAP: usize = 8;

/// The most candidate draws one pick submission carries (a safety bound for
/// the extract copy; a 9-px crop never legitimately covers more).
const PICK_ITEM_CAP: usize = 512;

/// Conservative world half-extent (metres) of a skinned avatar part's pick
/// bound. Skinned parts have no CPU `Aabb` (their drawn bounds live in the GPU
/// palettes), so the cursor-crop cull bounds them by a fixed box around the
/// avatar origin — large enough to cover any pose's reach and a worn mesh
/// body, small enough to drop avatars that are not near the cursor. Without
/// it every skinned part in the region was an unconditional candidate, so a
/// crowd filled [`PICK_ITEM_CAP`] with off-cursor parts and the one actually
/// under the cursor was dropped (the pick missed it).
const SKINNED_PICK_BOUND: f32 = 3.0;

/// How many bits of a pick tag hold the slot index.
const PICK_CLASS_SHIFT: u32 = 28;

/// The mask over a pick tag's index bits.
const PICK_INDEX_MASK: u32 = 0x0FFF_FFFF;

/// The pick class of an avatar submesh (index → avatar slot).
const CLASS_AVATAR: u32 = 1;

/// The pick class of an object face (index → object-face slot).
const CLASS_OBJECT_FACE: u32 = 2;

/// The pick class of a terrain patch (no per-entity slot; the depth carries
/// the information).
const CLASS_TERRAIN: u32 = 3;

/// The pick class of a water plane (occludes, resolves to `Water`).
const CLASS_WATER: u32 = 4;

/// Encode a pick tag from its class and slot index (`class:4 | index:28`).
/// `None` when the index outgrows the 28 index bits.
fn encode_pick_tag(class: u32, index: u32) -> Option<u32> {
    (index <= PICK_INDEX_MASK).then(|| class.wrapping_shl(PICK_CLASS_SHIFT) | index)
}

/// Split a pick tag back into `(class, index)`.
const fn decode_pick_tag(tag: u32) -> (u32, u32) {
    (tag.wrapping_shr(PICK_CLASS_SHIFT), tag & PICK_INDEX_MASK)
}

/// What a resolved pick tag names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickResolution {
    /// An avatar's geometry — the base body, a rigid part, the placeholder
    /// sphere, or (with `worn` set) a worn rigged attachment submesh.
    Avatar {
        /// The avatar the geometry belongs to.
        agent: AgentKey,
        /// The worn object of a rigged attachment submesh, so the hit can
        /// route to the attachment pies; `None` for the body itself.
        worn: Option<ScopedObjectId>,
    },
    /// A prim face of an in-world (or worn rigid) object.
    ObjectFace {
        /// The face's mesh entity (for the surface-refinement ray test).
        entity: Entity,
        /// The object the face belongs to.
        scoped: ScopedObjectId,
        /// The face's Linden face index.
        face: PrimFaceId,
    },
    /// A terrain patch (bare land; the hit point is the depth unprojection).
    Terrain,
    /// A water plane (pickable-opaque so it occludes what is under it, but no
    /// consumer targets it).
    Water,
}

/// One allocated avatar pick slot: the `(agent, worn)` identity shared —
/// reference-counted — by every submesh of that pairing.
#[derive(Debug, Clone, Copy)]
struct AvatarSlot {
    /// The avatar the slot's submeshes belong to.
    agent: AgentKey,
    /// The worn object of a rigged attachment submesh set, `None` for the
    /// body's own parts.
    worn: Option<ScopedObjectId>,
    /// How many live entities carry this slot's tag.
    refs: u32,
}

/// One allocated object-face pick slot (one per face entity).
#[derive(Debug, Clone, Copy)]
struct ObjectFaceSlot {
    /// The face's mesh entity.
    entity: Entity,
    /// The object the face belongs to.
    scoped: ScopedObjectId,
    /// The face's Linden face index.
    face: PrimFaceId,
}

/// The pick-ID registry: dense free-listed slot tables per class, the
/// entity → tag map used to free on despawn, and the tag → identity resolve
/// the readback goes through.
#[derive(Resource, Debug, Default)]
pub(crate) struct PickRegistry {
    /// Avatar slots by index (class 1).
    avatar_slots: Vec<Option<AvatarSlot>>,
    /// Free avatar slot indices.
    avatar_free: Vec<u32>,
    /// The live `(agent, worn)` → avatar slot index map (slots are shared).
    avatar_index: HashMap<(AgentKey, Option<ScopedObjectId>), u32>,
    /// Object-face slots by index (class 2).
    object_slots: Vec<Option<ObjectFaceSlot>>,
    /// Free object-face slot indices.
    object_free: Vec<u32>,
    /// Every tagged entity's tag, for freeing on removal/despawn.
    entity_tags: HashMap<Entity, u32>,
}

impl PickRegistry {
    /// Allocate (or share) the avatar slot for `(agent, worn)` and record
    /// `entity` under its tag. `None` only on 28-bit slot exhaustion.
    fn alloc_avatar(
        &mut self,
        entity: Entity,
        agent: AgentKey,
        worn: Option<ScopedObjectId>,
    ) -> Option<u32> {
        let index = match self.avatar_index.get(&(agent, worn)) {
            Some(index) => {
                let index = *index;
                if let Some(Some(slot)) = self.avatar_slots.get_mut(usize::try_from(index).ok()?) {
                    slot.refs = slot.refs.saturating_add(1);
                }
                index
            }
            None => {
                let slot = AvatarSlot {
                    agent,
                    worn,
                    refs: 1,
                };
                let index = match self.avatar_free.pop() {
                    Some(free) => {
                        if let Some(entry) = self.avatar_slots.get_mut(usize::try_from(free).ok()?)
                        {
                            *entry = Some(slot);
                        }
                        free
                    }
                    None => {
                        let index = u32::try_from(self.avatar_slots.len()).ok()?;
                        self.avatar_slots.push(Some(slot));
                        index
                    }
                };
                self.avatar_index.insert((agent, worn), index);
                index
            }
        };
        let tag = encode_pick_tag(CLASS_AVATAR, index)?;
        self.entity_tags.insert(entity, tag);
        Some(tag)
    }

    /// Allocate the object-face slot for `entity` and record it under its
    /// tag. `None` only on 28-bit slot exhaustion.
    fn alloc_object_face(
        &mut self,
        entity: Entity,
        scoped: ScopedObjectId,
        face: PrimFaceId,
    ) -> Option<u32> {
        let slot = ObjectFaceSlot {
            entity,
            scoped,
            face,
        };
        let index = match self.object_free.pop() {
            Some(free) => {
                if let Some(entry) = self.object_slots.get_mut(usize::try_from(free).ok()?) {
                    *entry = Some(slot);
                }
                free
            }
            None => {
                let index = u32::try_from(self.object_slots.len()).ok()?;
                self.object_slots.push(Some(slot));
                index
            }
        };
        let tag = encode_pick_tag(CLASS_OBJECT_FACE, index)?;
        self.entity_tags.insert(entity, tag);
        Some(tag)
    }

    /// Record `entity` under a fixed-class tag (terrain / water — no slot).
    fn note_fixed(&mut self, entity: Entity, tag: u32) {
        self.entity_tags.insert(entity, tag);
    }

    /// Free whatever `entity` held: drop an avatar slot reference (freeing
    /// the slot at zero), free an object-face slot, or forget a fixed tag.
    fn free_entity(&mut self, entity: Entity) {
        let Some(tag) = self.entity_tags.remove(&entity) else {
            return;
        };
        let (class, index) = decode_pick_tag(tag);
        let Ok(slot_index) = usize::try_from(index) else {
            return;
        };
        match class {
            CLASS_AVATAR => {
                if let Some(entry) = self.avatar_slots.get_mut(slot_index) {
                    let emptied = match entry {
                        Some(slot) => {
                            slot.refs = slot.refs.saturating_sub(1);
                            slot.refs == 0
                        }
                        None => false,
                    };
                    if emptied {
                        if let Some(slot) = entry.take() {
                            self.avatar_index.remove(&(slot.agent, slot.worn));
                        }
                        self.avatar_free.push(index);
                    }
                }
            }
            CLASS_OBJECT_FACE => {
                if let Some(entry) = self.object_slots.get_mut(slot_index)
                    && entry.take().is_some()
                {
                    self.object_free.push(index);
                }
            }
            _fixed => {}
        }
    }

    /// Resolve a read-back tag to what it names (`None` for 0 / a freed or
    /// unknown slot).
    pub(crate) fn resolve(&self, tag: u32) -> Option<PickResolution> {
        if tag == 0 {
            return None;
        }
        let (class, index) = decode_pick_tag(tag);
        match class {
            CLASS_AVATAR => {
                let slot = self
                    .avatar_slots
                    .get(usize::try_from(index).ok()?)?
                    .as_ref()?;
                Some(PickResolution::Avatar {
                    agent: slot.agent,
                    worn: slot.worn,
                })
            }
            CLASS_OBJECT_FACE => {
                let slot = self
                    .object_slots
                    .get(usize::try_from(index).ok()?)?
                    .as_ref()?;
                Some(PickResolution::ObjectFace {
                    entity: slot.entity,
                    scoped: slot.scoped,
                    face: slot.face,
                })
            }
            CLASS_TERRAIN => Some(PickResolution::Terrain),
            CLASS_WATER => Some(PickResolution::Water),
            _unknown => None,
        }
    }
}

/// The encoded pick tag on a pickable mesh entity — the assignment systems'
/// marker (its value also rides the sibling [`MeshTag`]). Entities without it
/// are invisible to the pick view.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PickId(pub(crate) u32);

// ---------------------------------------------------------------------------
// Tag assignment / freeing.
// ---------------------------------------------------------------------------

/// Tag every untagged avatar mesh piece (base body parts, rigid parts, the
/// placeholder sphere, worn rigged submeshes) with its avatar's class-1 tag —
/// worn rigged submeshes get their own `(agent, worn)` slot so a hit on them
/// routes to the attachment pies.
#[expect(
    clippy::type_complexity,
    reason = "a query's term list is its type; splitting it loses the single-query guarantee"
)]
pub(crate) fn assign_avatar_pick_tags(
    mut registry: ResMut<PickRegistry>,
    untagged: Query<
        (Entity, &AvatarPickTarget, Option<&WornPickTarget>),
        (With<Mesh3d>, Without<PickId>),
    >,
    mut commands: Commands,
) {
    for (entity, target, worn) in &untagged {
        let worn = worn.map(|worn| worn.scoped);
        let Some(tag) = registry.alloc_avatar(entity, target.agent(), worn) else {
            warn!("gpu-pick: avatar slot space exhausted; entity stays unpickable");
            continue;
        };
        // `try_insert`, not `insert`: another system may despawn this queried entity
        // (a re-tessellation / LOD swap despawns face entities) before this deferred
        // command applies, which an `insert` would panic on; the replacement is
        // tagged next frame (still `Without<PickId>`).
        commands
            .entity(entity)
            .try_insert((PickId(tag), MeshTag(tag)));
    }
}

/// Tag every untagged prim-face mesh with its object's class-2 tag, resolved
/// by walking up to the owning [`SceneObject`]. Faces whose object is not yet
/// in the hierarchy retry next frame (the `Without<PickId>` filter keeps the
/// retry set empty at steady state). Avatar-tagged submeshes are excluded —
/// the class-1 rule owns them.
#[expect(
    clippy::type_complexity,
    reason = "a query's term list is its type; splitting it loses the single-query guarantee"
)]
pub(crate) fn assign_object_face_pick_tags(
    mut registry: ResMut<PickRegistry>,
    untagged: Query<
        (Entity, &PrimFaceEntity),
        (With<Mesh3d>, Without<PickId>, Without<AvatarPickTarget>),
    >,
    scene: Query<&SceneObject>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    for (entity, face) in &untagged {
        // Walk up the linkset to the entity carrying the scene identity (the
        // same walk the resolvers use).
        let mut current = entity;
        let scoped = loop {
            if let Ok(scene) = scene.get(current) {
                break Some(scene.scoped_id);
            }
            match parents.get(current) {
                Ok(child_of) => current = child_of.parent(),
                Err(_root) => break None,
            }
        };
        let Some(scoped) = scoped else {
            continue;
        };
        let Some(tag) = registry.alloc_object_face(entity, scoped, face.face_id) else {
            warn!("gpu-pick: object-face slot space exhausted; face stays unpickable");
            continue;
        };
        // `try_insert`, not `insert`: another system may despawn this queried entity
        // (a re-tessellation / LOD swap despawns face entities) before this deferred
        // command applies, which an `insert` would panic on; the replacement is
        // tagged next frame (still `Without<PickId>`).
        commands
            .entity(entity)
            .try_insert((PickId(tag), MeshTag(tag)));
    }
}

/// Tag every untagged terrain patch with the shared class-3 tag (the depth
/// unprojection carries the actual ground point).
#[expect(
    clippy::type_complexity,
    reason = "a query's term list is its type; splitting it loses the single-query guarantee"
)]
pub(crate) fn assign_terrain_pick_tags(
    mut registry: ResMut<PickRegistry>,
    untagged: Query<Entity, (With<TerrainSurface>, With<Mesh3d>, Without<PickId>)>,
    mut commands: Commands,
) {
    let Some(tag) = encode_pick_tag(CLASS_TERRAIN, 0) else {
        return;
    };
    for entity in &untagged {
        registry.note_fixed(entity, tag);
        // `try_insert`, not `insert`: another system may despawn this queried entity
        // (a re-tessellation / LOD swap despawns face entities) before this deferred
        // command applies, which an `insert` would panic on; the replacement is
        // tagged next frame (still `Without<PickId>`).
        commands
            .entity(entity)
            .try_insert((PickId(tag), MeshTag(tag)));
    }
}

/// Tag every untagged water plane (endless ocean + per-region planes) with
/// the shared class-4 tag, so water occludes what is under it without being a
/// pick target itself.
#[expect(
    clippy::type_complexity,
    reason = "a query's term list is its type; splitting it loses the single-query guarantee"
)]
pub(crate) fn assign_water_pick_tags(
    mut registry: ResMut<PickRegistry>,
    untagged: Query<
        Entity,
        (
            Or<(With<WaterOcean>, With<WaterRegionPlane>)>,
            With<Mesh3d>,
            Without<PickId>,
        ),
    >,
    mut commands: Commands,
) {
    let Some(tag) = encode_pick_tag(CLASS_WATER, 0) else {
        return;
    };
    for entity in &untagged {
        registry.note_fixed(entity, tag);
        // `try_insert`, not `insert`: another system may despawn this queried entity
        // (a re-tessellation / LOD swap despawns face entities) before this deferred
        // command applies, which an `insert` would panic on; the replacement is
        // tagged next frame (still `Without<PickId>`).
        commands
            .entity(entity)
            .try_insert((PickId(tag), MeshTag(tag)));
    }
}

/// Free the registry slot of every entity whose [`PickId`] went away
/// (despawn, or an explicit removal).
pub(crate) fn free_pick_tags(
    mut registry: ResMut<PickRegistry>,
    mut removed: RemovedComponents<PickId>,
) {
    for entity in removed.read() {
        registry.free_entity(entity);
    }
}

/// Emit each pick-tagged mesh into [`GpuPickWarmSet`] **once, when it rezzes**,
/// so the render world can kick off its pick-pipeline compilation the moment the
/// mesh becomes pickable — during world-rez / login — instead of only on the
/// first pick over it (`render::warm_gpu_pick_pipelines`).
///
/// **Change-driven, not a per-frame rescan.** The query is filtered to entities
/// that just became pickable ([`Added<PickId>`]) or whose mesh handle just
/// changed ([`Changed<Mesh3d>`], e.g. a placeholder swapped for the decoded
/// mesh); in steady state it matches nothing, so this costs ~0. It used to scan
/// **every** `With<PickId>` entity every frame — ~3.6 ms/frame on a dense region
/// (every tessellated prim face is pick-tagged), rebuilding a near-constant set
/// ([[viewer-perf-pick-warm-set-scales-with-crowd]]). The render world keys the
/// actual pipeline by mesh layout and retries a not-yet-uploaded mesh until it
/// lands (see `render::warm_gpu_pick_pipelines`), so warming stays tied to
/// **rez** — a mesh is warm well before any pick reaches it, never lazily on the
/// first pick.
#[expect(
    clippy::type_complexity,
    reason = "an ECS system's arguments are its injected queries; the change-detection filter is inherently a nested tuple"
)]
pub(crate) fn collect_pick_warm_set(
    mut warm: ResMut<GpuPickWarmSet>,
    candidates: Query<
        (&Mesh3d, Has<SkinnedMesh>),
        (With<PickId>, Or<(Added<PickId>, Changed<Mesh3d>)>),
    >,
) {
    // Only this frame's newly-pickable / re-meshed entities. The render world
    // folds these into its persistent retry set, so nothing is dropped by
    // clearing here (a mesh not yet uploaded is retried render-side).
    warm.0.clear();
    let mut seen = HashSet::new();
    for (mesh3d, skinned) in &candidates {
        let mesh_id = mesh3d.0.id();
        if seen.insert((skinned, mesh_id)) {
            warm.0.push((skinned, mesh_id));
        }
    }
}

// ---------------------------------------------------------------------------
// The request / pending / resolve plumbing.
// ---------------------------------------------------------------------------

/// Why a pick was requested — carried through to [`GpuPickResolved`] so each
/// consumer reads only its own answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickPurpose {
    /// The hover tooltip's dwell pick.
    Hover,
    /// The media-face hover (`hover_media_faces`): which media-on-prim face, if
    /// any, is the nearest thing under the cursor — so an avatar / wall in front
    /// of a media screen suppresses its controls, occlusion-correct and without a
    /// whole-scene `MeshRayCast`.
    Media,
    /// The left-click world touch.
    Touch,
    /// The right-click context-menu resolver.
    RightClick,
    /// The double-click teleport.
    DoubleClick,
    /// The inventory drag's world drop target.
    Drag,
    /// The `SL_VIEWER_DEBUG_PICK` cursor inspector.
    Inspector,
}

/// A resolved pick, delivered 1–2 frames after its request.
#[derive(Message, Debug, Clone)]
pub(crate) struct GpuPickResolved {
    /// The purpose the request carried.
    pub(crate) purpose: PickPurpose,
    /// The cursor position the pick was taken at, logical pixels.
    pub(crate) cursor: Vec2,
    /// The world ray through the cursor at request time (for surface
    /// refinement and look-at derivation).
    pub(crate) ray: Ray3d,
    /// What the centre pixel showed, or `None` for a miss (sky, or an
    /// untagged mesh).
    pub(crate) hit: Option<GpuPickHit>,
}

/// The hit half of a [`GpuPickResolved`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct GpuPickHit {
    /// What the pick tag resolved to.
    pub(crate) resolution: PickResolution,
    /// The world-space hit point (the centre pixel's depth unprojected
    /// through the submitting frame's matrices).
    pub(crate) world_point: Vec3,
    /// The distance from the ray origin to the hit point, metres.
    pub(crate) distance: f32,
}

/// One in-flight pick: the submission identity and everything the resolve
/// needs from the submitting frame.
#[derive(Debug, Clone)]
struct PendingPick {
    /// The sequence number rendered into the ID target.
    sequence: u32,
    /// The cursor position at request time.
    cursor: Vec2,
    /// The world ray through the cursor at request time.
    ray: Ray3d,
    /// The inverse of the submitting frame's cropped `clip_from_world`, for
    /// the depth unprojection.
    world_from_clip: Mat4,
    /// The purposes riding this submission.
    purposes: Vec<PickPurpose>,
    /// Frames since submission (for the timeout).
    age: u32,
}

/// The main-world pick queue: consumers [`request`](GpuPicker::request) picks
/// during `Update`; [`submit_gpu_picks`] folds each frame's requests into one
/// submission; the readback observer resolves them.
#[derive(Resource, Debug, Default)]
pub(crate) struct GpuPicker {
    /// This frame's requests (cursor, purpose).
    requests: Vec<(Vec2, PickPurpose)>,
    /// Picks awaiting their readback.
    pending: Vec<PendingPick>,
    /// The last submission sequence number issued.
    sequence: u32,
    /// Whether the [`Readback`] component is currently installed on the
    /// readback entity.
    readback_installed: bool,
}

impl GpuPicker {
    /// Queue a pick at `cursor` (logical pixels) for `purpose`. All of one
    /// frame's requests share a single submission (and its cursor — the last
    /// request wins, which is harmless: every consumer asks at the live
    /// cursor).
    pub(crate) fn request(&mut self, cursor: Vec2, purpose: PickPurpose) {
        self.requests.push((cursor, purpose));
    }
}

/// The pick view's targets and readback driver, created once at startup.
#[derive(Resource, Clone)]
pub(crate) struct GpuPickTargets {
    /// The `Rgba32Uint` ID target (tag, depth bits, sequence, unused).
    pub(crate) id_image: Handle<Image>,
    /// The entity carrying the [`Readback`] component (toggled on while picks
    /// are in flight) and the readback observer.
    readback_entity: Entity,
}

impl ExtractResource for GpuPickTargets {
    type Source = Self;

    /// Clone the handles into the render world.
    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// One draw of a pick submission (built in the main world, drawn in the
/// render world).
#[derive(Debug, Clone)]
pub(crate) struct GpuPickItem {
    /// The main-world entity (skin-palette offsets resolve against it).
    pub(crate) entity: Entity,
    /// The mesh asset to draw.
    pub(crate) mesh: AssetId<Mesh>,
    /// The encoded pick tag the fragment writes.
    pub(crate) tag: u32,
    /// For a static mesh the full `crop_clip_from_world * world_from_local`;
    /// for a skinned mesh just `crop_clip_from_world` (the palette supplies
    /// the world transform).
    pub(crate) clip_from_local: Mat4,
    /// Whether to draw through the skinned pipeline variant.
    pub(crate) skinned: bool,
}

/// The per-frame pick submission the render world draws — empty / inactive on
/// frames without requests.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct GpuPickSubmission {
    /// The sequence rendered into the ID target this frame.
    pub(crate) sequence: u32,
    /// Whether this frame renders the pick view at all.
    pub(crate) active: bool,
    /// The candidate draws under the crop.
    pub(crate) items: Vec<GpuPickItem>,
}

impl ExtractResource for GpuPickSubmission {
    type Source = Self;

    /// Clone the (tiny) submission into the render world.
    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// The distinct `(skinned, mesh)` combinations among this frame's
/// pick-tagged entities, extracted every frame regardless of whether a pick
/// is pending — the pipeline-warming counterpart of [`GpuPickSubmission`]
/// (which only carries candidates while a pick is active). Populated by
/// [`collect_pick_warm_set`]; consumed by
/// [`render::warm_gpu_pick_pipelines`].
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct GpuPickWarmSet(Vec<(bool, AssetId<Mesh>)>);

impl ExtractResource for GpuPickWarmSet {
    type Source = Self;

    /// Clone the (tiny) warm set into the render world.
    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

/// Build the cropped `clip_from_world`: the main camera's projection and view
/// with a clip-space crop that maps the [`CROP_PIXELS`]² square around
/// `cursor` (logical pixels, y down) onto the full NDC range — the centre
/// texel of the pick target lands exactly on the cursor pixel.
fn crop_clip_from_world(
    clip_from_view: Mat4,
    view_from_world: Mat4,
    cursor: Vec2,
    viewport: Vec2,
) -> Mat4 {
    // The cursor in NDC, and the crop's half-extent in NDC units.
    let cx = 2.0 * cursor.x / viewport.x - 1.0;
    let cy = 1.0 - 2.0 * cursor.y / viewport.y;
    let hx = CROP_PIXELS / viewport.x;
    let hy = CROP_PIXELS / viewport.y;
    // In homogeneous clip space, `ndc' = (ndc - c) / h` is linear in (x, w):
    // x' = x/h - (c/h)·w.
    let crop = Mat4::from_cols(
        Vec4::new(1.0 / hx, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0 / hy, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(-cx / hx, -cy / hy, 0.0, 1.0),
    );
    crop.mul_mat4(&clip_from_view.mul_mat4(&view_from_world))
}

/// Unproject the centre pixel's depth through the submitting frame's inverse
/// cropped `clip_from_world` (the centre texel sits at NDC (0, 0)).
fn unproject_centre(world_from_clip: &Mat4, depth: f32) -> Option<Vec3> {
    let clip = world_from_clip.mul_vec4(Vec4::new(0.0, 0.0, depth, 1.0));
    (clip.w.abs() > f32::EPSILON)
        .then(|| Vec3::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w))
}

/// Parse the centre texel of a read-back ID target: `(tag, depth, sequence)`.
/// The readback buffer is row-padded to the copy alignment, so the row stride
/// is derived from the buffer length.
fn parse_centre_pixel(data: &[u8]) -> Option<(u32, f32, u32)> {
    let rows = usize::try_from(CROP_SIZE).ok()?;
    let stride = data.len().checked_div(rows)?;
    let x_offset = usize::try_from(CROP_CENTRE)
        .ok()?
        .checked_mul(ID_TEXEL_BYTES)?;
    let offset = usize::try_from(CROP_CENTRE)
        .ok()?
        .checked_mul(stride)?
        .checked_add(x_offset)?;
    let word = |at: usize| -> Option<u32> {
        let bytes = data.get(at..at.checked_add(4)?)?;
        Some(u32::from_ne_bytes(bytes.try_into().ok()?))
    };
    let tag = word(offset)?;
    let depth_bits = word(offset.checked_add(4)?)?;
    let sequence = word(offset.checked_add(8)?)?;
    Some((tag, f32::from_bits(depth_bits), sequence))
}

/// Create the ID target image and the readback entity (with its observer).
fn setup_gpu_pick_targets(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: CROP_SIZE,
            height: CROP_SIZE,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &[0_u8; ID_TEXEL_BYTES],
        bevy::render::render_resource::TextureFormat::Rgba32Uint,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage = TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    let id_image = images.add(image);
    let readback_entity = commands
        .spawn(Name::new("gpu-pick-readback"))
        .observe(on_pick_readback)
        .id();
    commands.insert_resource(GpuPickTargets {
        id_image,
        readback_entity,
    });
}

/// Fold this frame's pick requests into one submission: compute the cropped
/// projection, walk the pick-tagged world candidates intersecting the crop
/// frustum, and stash the draw list for the render world; age and time out
/// unanswered picks; keep the [`Readback`] driver installed exactly while
/// picks are in flight.
#[expect(
    clippy::type_complexity,
    reason = "a query's term list is its type; splitting it loses the single-query guarantee"
)]
pub(crate) fn submit_gpu_picks(
    mut picker: ResMut<GpuPicker>,
    mut submission: ResMut<GpuPickSubmission>,
    targets: Option<Res<GpuPickTargets>>,
    camera: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    candidates: Query<(
        Entity,
        &PickId,
        &Mesh3d,
        &ViewVisibility,
        &GlobalTransform,
        Option<&Aabb>,
        Has<SkinnedMesh>,
        Option<&RenderLayers>,
    )>,
    mut resolved: MessageWriter<GpuPickResolved>,
    mut commands: Commands,
) {
    // Age the in-flight picks and time out the never-answered ones.
    for pending in &mut picker.pending {
        pending.age = pending.age.saturating_add(1);
    }
    let mut timed_out = Vec::new();
    picker.pending.retain(|pending| {
        if pending.age > PICK_TIMEOUT_FRAMES {
            timed_out.push(pending.clone());
            false
        } else {
            true
        }
    });
    for pending in timed_out {
        for purpose in pending.purposes {
            resolved.write(GpuPickResolved {
                purpose,
                cursor: pending.cursor,
                ray: pending.ray,
                hit: None,
            });
        }
    }

    submission.active = false;
    submission.items.clear();

    let requests: Vec<(Vec2, PickPurpose)> = std::mem::take(&mut picker.requests);
    if !requests.is_empty()
        && let Ok((camera, camera_transform)) = camera.single()
        && let Some(viewport) = camera.logical_viewport_size()
        && let Some((cursor, _last_purpose)) = requests.last().copied()
        && let Ok(ray) = camera.viewport_to_world(camera_transform, cursor)
    {
        let clip_from_view = camera.clip_from_view();
        let view_from_world = Mat4::from(camera_transform.affine()).inverse();
        let clip_from_world =
            crop_clip_from_world(clip_from_view, view_from_world, cursor, viewport);
        let frustum = Frustum(ViewFrustum::from_clip_from_world(&clip_from_world));

        for (entity, pick_id, mesh3d, visibility, global, aabb, skinned, layers) in &candidates {
            if !visibility.get() || on_hud_layer(layers) {
                continue;
            }
            // Cursor-crop cull. Static parts use their real Aabb; skinned parts
            // have none (their drawn bounds live in the GPU palettes), so bound
            // them conservatively by a fixed box around the avatar origin (the
            // part's `GlobalTransform` — every part is `ChildOf(root)` at the
            // avatar's world position). Culling skinned parts too is what keeps
            // a crowd from filling `PICK_ITEM_CAP` with off-cursor parts and
            // dropping the one under the cursor.
            let off_crop = if skinned {
                let bound = Aabb {
                    center: global.translation().into(),
                    half_extents: Vec3A::splat(SKINNED_PICK_BOUND),
                };
                !frustum.intersects_obb(&bound, &Affine3A::IDENTITY, true, false)
            } else if let Some(aabb) = aabb {
                !frustum.intersects_obb(aabb, &global.affine(), true, false)
            } else {
                false
            };
            if off_crop {
                continue;
            }
            if submission.items.len() >= PICK_ITEM_CAP {
                warn!("gpu-pick: candidate cap {PICK_ITEM_CAP} reached; pick may be incomplete");
                break;
            }
            let clip_from_local = if skinned {
                clip_from_world
            } else {
                clip_from_world.mul_mat4(&Mat4::from(global.affine()))
            };
            submission.items.push(GpuPickItem {
                entity,
                mesh: mesh3d.0.id(),
                tag: pick_id.0,
                clip_from_local,
                skinned,
            });
        }

        picker.sequence = picker.sequence.wrapping_add(1).max(1);
        submission.sequence = picker.sequence;
        submission.active = true;
        if picker.pending.len() >= PICK_PENDING_CAP {
            let dropped = picker.pending.remove(0);
            for purpose in dropped.purposes {
                resolved.write(GpuPickResolved {
                    purpose,
                    cursor: dropped.cursor,
                    ray: dropped.ray,
                    hit: None,
                });
            }
        }
        let sequence = picker.sequence;
        picker.pending.push(PendingPick {
            sequence,
            cursor,
            ray,
            world_from_clip: clip_from_world.inverse(),
            purposes: requests.into_iter().map(|(_at, purpose)| purpose).collect(),
            age: 0,
        });
    }

    // Keep the readback driver installed exactly while picks are in flight.
    let Some(targets) = targets else {
        return;
    };
    let want_readback = !picker.pending.is_empty();
    if want_readback && !picker.readback_installed {
        commands
            .entity(targets.readback_entity)
            .insert(Readback::texture(targets.id_image.clone()));
        picker.readback_installed = true;
    } else if !want_readback && picker.readback_installed {
        commands
            .entity(targets.readback_entity)
            .remove::<Readback>();
        picker.readback_installed = false;
    }
}

/// The readback observer: parse the centre texel, match its sequence to the
/// in-flight pick, resolve the tag through the registry, unproject the depth,
/// and deliver a [`GpuPickResolved`] per purpose.
fn on_pick_readback(
    readback: On<ReadbackComplete>,
    mut picker: ResMut<GpuPicker>,
    registry: Res<PickRegistry>,
    mut resolved: MessageWriter<GpuPickResolved>,
) {
    let Some((tag, depth, sequence)) = parse_centre_pixel(&readback.data) else {
        return;
    };
    let Some(position) = picker
        .pending
        .iter()
        .position(|pending| pending.sequence == sequence)
    else {
        // A stale duplicate (the driver copies every frame while installed) or
        // a frame before the first submission: nothing to deliver.
        return;
    };
    let pending = picker.pending.remove(position);
    let hit = registry.resolve(tag).and_then(|resolution| {
        let world_point = unproject_centre(&pending.world_from_clip, depth)?;
        Some(GpuPickHit {
            resolution,
            world_point,
            distance: pending.ray.origin.distance(world_point),
        })
    });
    for purpose in pending.purposes {
        resolved.write(GpuPickResolved {
            purpose,
            cursor: pending.cursor,
            ray: pending.ray,
            hit,
        });
    }
}

/// The GPU-picking plugin: the registry + tag assignment, the queue and
/// submission, the readback resolve, and the render-world pass.
#[derive(Debug, Default)]
pub(crate) struct GpuPickPlugin;

impl Plugin for GpuPickPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            PICK_SHADER_HANDLE,
            "gpu_pick/pick.wgsl",
            Shader::from_wgsl
        );
        app.init_resource::<PickRegistry>()
            .init_resource::<GpuPicker>()
            .init_resource::<GpuPickSubmission>()
            .init_resource::<GpuPickWarmSet>()
            .add_message::<GpuPickResolved>()
            .add_plugins((
                ExtractResourcePlugin::<GpuPickSubmission>::default(),
                ExtractResourcePlugin::<GpuPickWarmSet>::default(),
                ExtractResourcePlugin::<GpuPickTargets>::default(),
            ))
            .add_systems(Startup, setup_gpu_pick_targets)
            .add_systems(
                Update,
                (
                    assign_avatar_pick_tags,
                    assign_object_face_pick_tags,
                    assign_terrain_pick_tags,
                    assign_water_pick_tags,
                    free_pick_tags,
                    collect_pick_warm_set,
                ),
            )
            .add_systems(
                PostUpdate,
                // After visibility, so `ViewVisibility` is this frame's; the
                // extract then carries this frame's submission.
                submit_gpu_picks.after(VisibilitySystems::CheckVisibility),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render::build_render_app(render_app);
    }
}

/// Look up the extracted GPU image of the ID target (used by the render
/// half; re-exported here so the render module needs no asset imports).
pub(crate) fn id_target_view<'a>(
    targets: &GpuPickTargets,
    gpu_images: &'a bevy::render::render_asset::RenderAssets<GpuImage>,
) -> Option<&'a bevy::render::render_resource::TextureView> {
    gpu_images
        .get(&targets.id_image)
        .map(|image| &image.texture_view)
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, CircuitId, PrimFaceId, RegionLocalObjectId, ScopedObjectId, Uuid,
    };

    use super::{
        CLASS_AVATAR, CLASS_OBJECT_FACE, CLASS_TERRAIN, CLASS_WATER, CROP_PIXELS, GpuPickWarmSet,
        PICK_INDEX_MASK, PickId, PickRegistry, PickResolution, collect_pick_warm_set,
        crop_clip_from_world, decode_pick_tag, encode_pick_tag, parse_centre_pixel,
        unproject_centre,
    };

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Tags round-trip through encode/decode across every class, and an index
    /// beyond the 28 bits is rejected.
    #[test]
    fn tag_encode_decode_round_trips() -> Result<(), TestError> {
        for class in [CLASS_AVATAR, CLASS_OBJECT_FACE, CLASS_TERRAIN, CLASS_WATER] {
            for index in [0_u32, 1, 4095, PICK_INDEX_MASK] {
                let tag = encode_pick_tag(class, index).ok_or("encode in range")?;
                assert_eq!(decode_pick_tag(tag), (class, index));
            }
        }
        assert_eq!(
            encode_pick_tag(CLASS_AVATAR, PICK_INDEX_MASK.wrapping_add(1)),
            None,
            "an index beyond 28 bits must be rejected"
        );
        Ok(())
    }

    /// The pipeline pre-warm collection is **change-driven**: a mesh is emitted
    /// the frame it becomes pickable (or its mesh handle changes), and an
    /// unchanged pickable set collects nothing — it never re-scans every
    /// pick-tagged entity per frame ([[viewer-perf-pick-warm-set-scales-with-crowd]]).
    #[test]
    fn warm_set_is_change_driven_not_a_full_rescan() {
        let mut app = App::new();
        app.init_resource::<GpuPickWarmSet>();
        app.add_systems(Update, collect_pick_warm_set);

        // A freshly pick-tagged mesh is emitted on the frame it rezzes.
        let entity = app
            .world_mut()
            .spawn((Mesh3d(Handle::default()), PickId(1)))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<GpuPickWarmSet>().0.len(),
            1,
            "a newly pick-tagged mesh must be warmed at rez"
        );

        // Steady state: nothing changed, so nothing is collected — the previous
        // implementation rescanned every `With<PickId>` entity here.
        app.update();
        assert_eq!(
            app.world().resource::<GpuPickWarmSet>().0.len(),
            0,
            "an unchanged pickable set must not be re-collected every frame"
        );

        // A mesh-handle swap on an already-pickable entity (placeholder →
        // decoded mesh) re-emits it, so its new layout still warms at rez.
        if let Some(mut mesh) = app.world_mut().get_mut::<Mesh3d>(entity) {
            mesh.0 = Handle::default();
        }
        app.update();
        assert_eq!(
            app.world().resource::<GpuPickWarmSet>().0.len(),
            1,
            "a changed mesh handle must re-warm at rez, not defer to first pick"
        );
    }

    /// A convenience scoped id for the registry tests.
    fn scoped(id: u32) -> ScopedObjectId {
        ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId::new(id))
    }

    /// Distinct fresh entities for registry tests (real ids, no live world
    /// needed afterwards).
    fn test_entities<const N: usize>() -> [Entity; N] {
        let mut world = World::new();
        core::array::from_fn(|_index| world.spawn_empty().id())
    }

    /// Avatar slots are shared per `(agent, worn)`, reference-counted, and
    /// freed (and reusable) once the last entity is gone.
    #[test]
    fn registry_avatar_slots_share_refcount_and_free() -> Result<(), TestError> {
        let mut registry = PickRegistry::default();
        let agent = AgentKey::from(Uuid::from_u128(0xA));
        let [body_a, body_b, worn] = test_entities::<3>();
        let tag_a = registry
            .alloc_avatar(body_a, agent, None)
            .ok_or("alloc a")?;
        let tag_b = registry
            .alloc_avatar(body_b, agent, None)
            .ok_or("alloc b")?;
        assert_eq!(tag_a, tag_b, "same (agent, worn) shares one slot");
        let tag_worn = registry
            .alloc_avatar(worn, agent, Some(scoped(42)))
            .ok_or("alloc worn")?;
        assert!(tag_worn != tag_a, "a worn submesh gets its own slot");
        assert_eq!(
            registry.resolve(tag_worn),
            Some(PickResolution::Avatar {
                agent,
                worn: Some(scoped(42))
            })
        );
        // Dropping one body entity keeps the shared slot alive.
        registry.free_entity(body_a);
        assert_eq!(
            registry.resolve(tag_a),
            Some(PickResolution::Avatar { agent, worn: None })
        );
        // Dropping the last frees it.
        registry.free_entity(body_b);
        assert_eq!(registry.resolve(tag_a), None, "freed slot resolves to none");
        // The freed index is reused.
        let again = registry
            .alloc_avatar(body_a, agent, None)
            .ok_or("realloc")?;
        assert_eq!(again, tag_a, "the freed slot index is reused");
        Ok(())
    }

    /// Object-face slots are per entity, resolve to their identity, and free
    /// back into the free list on removal.
    #[test]
    fn registry_object_faces_alloc_resolve_free() -> Result<(), TestError> {
        let mut registry = PickRegistry::default();
        let [face_a, face_b] = test_entities::<2>();
        let tag_a = registry
            .alloc_object_face(face_a, scoped(7), PrimFaceId::new(0))
            .ok_or("alloc a")?;
        let tag_b = registry
            .alloc_object_face(face_b, scoped(7), PrimFaceId::new(3))
            .ok_or("alloc b")?;
        assert!(tag_a != tag_b, "every face entity has its own slot");
        assert_eq!(
            registry.resolve(tag_b),
            Some(PickResolution::ObjectFace {
                entity: face_b,
                scoped: scoped(7),
                face: PrimFaceId::new(3)
            })
        );
        registry.free_entity(face_a);
        assert_eq!(registry.resolve(tag_a), None);
        let reused = registry
            .alloc_object_face(face_a, scoped(9), PrimFaceId::new(1))
            .ok_or("realloc")?;
        assert_eq!(reused, tag_a, "the freed slot index is reused");
        // Tag 0 (the cleared background) never resolves.
        assert_eq!(registry.resolve(0), None);
        Ok(())
    }

    /// A world point projected through the cropped clip lands on NDC (0, 0)
    /// when the cursor is its screen pixel, and the unprojection returns it.
    #[test]
    fn crop_projects_the_cursor_to_centre_and_unprojects_back() -> Result<(), TestError> {
        let viewport = Vec2::new(1280.0, 720.0);
        let clip_from_view = Mat4::perspective_infinite_reverse_rh(
            60_f32.to_radians(),
            viewport.x / viewport.y,
            0.1,
        );
        // A camera at +10 Z looking at the origin (the default -Z forward).
        let view_from_world =
            Mat4::from(GlobalTransform::from_xyz(0.0, 0.0, 10.0).affine()).inverse();
        let world_point = Vec3::new(1.5, -0.75, 2.0);
        // Where the plain projection puts the point on screen.
        let clip = clip_from_view
            .mul_mat4(&view_from_world)
            .mul_vec4(world_point.extend(1.0));
        let ndc = Vec3::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
        let cursor = Vec2::new(
            f32::midpoint(ndc.x, 1.0) * viewport.x,
            (1.0 - ndc.y) * 0.5 * viewport.y,
        );
        let cropped = crop_clip_from_world(clip_from_view, view_from_world, cursor, viewport);
        let crop_clip = cropped.mul_vec4(world_point.extend(1.0));
        let crop_ndc = Vec2::new(crop_clip.x / crop_clip.w, crop_clip.y / crop_clip.w);
        assert!(
            crop_ndc.length() < 1.0e-4,
            "the cursor's point must land at the crop centre, got {crop_ndc:?}"
        );
        // A point half a crop to the right lands at NDC x = +1.
        let cursor_off = Vec2::new(cursor.x - CROP_PIXELS * 0.5, cursor.y);
        let cropped_off =
            crop_clip_from_world(clip_from_view, view_from_world, cursor_off, viewport);
        let off_clip = cropped_off.mul_vec4(world_point.extend(1.0));
        assert!(
            (off_clip.x / off_clip.w - 1.0).abs() < 1.0e-3,
            "half a crop off-centre must land at the crop edge"
        );
        // The unprojection inverts the projection at the centre.
        let depth = crop_clip.z / crop_clip.w;
        let back = unproject_centre(&cropped.inverse(), depth).ok_or("unproject")?;
        assert!(
            back.distance(world_point) < 1.0e-3,
            "unproject must invert the crop projection, got {back:?} for {world_point:?}"
        );
        Ok(())
    }

    /// The centre-pixel parse reads the tag / depth / sequence words from a
    /// row-padded readback buffer.
    #[test]
    fn centre_pixel_parses_from_padded_rows() -> Result<(), TestError> {
        // 9 rows at a 256-byte padded stride (the wgpu copy alignment).
        let stride = 256_usize;
        let mut data = vec![0_u8; stride.checked_mul(9).ok_or("size")?];
        let offset = stride
            .checked_mul(4)
            .and_then(|row| row.checked_add(4_usize.checked_mul(16)?))
            .ok_or("offset")?;
        let tag = 0x2000_002A_u32;
        let depth = 0.625_f32;
        let sequence = 77_u32;
        let mut write = |at: usize, value: u32| -> Result<(), TestError> {
            let bytes = value.to_ne_bytes();
            data.get_mut(at..at.checked_add(4).ok_or("range")?)
                .ok_or("in bounds")?
                .copy_from_slice(&bytes);
            Ok(())
        };
        write(offset, tag)?;
        write(offset.checked_add(4).ok_or("o4")?, depth.to_bits())?;
        write(offset.checked_add(8).ok_or("o8")?, sequence)?;
        let (got_tag, got_depth, got_sequence) = parse_centre_pixel(&data).ok_or("parse")?;
        assert_eq!(got_tag, tag);
        assert_eq!(got_sequence, sequence);
        assert!((got_depth - depth).abs() < f32::EPSILON);
        Ok(())
    }
}
