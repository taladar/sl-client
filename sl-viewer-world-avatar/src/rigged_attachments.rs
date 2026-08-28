//! Worn rigged attachments and animesh control avatars: the object-layer
//! geometry that only means anything against an avatar's skeleton.
//!
//! An attachment in Second Life *is* an object — it arrives in the same object
//! stream, holds the same `TrackedObject` record and the same deferred
//! [`PendingGeometry`] — but where it goes and how it is skinned are avatar
//! facts, so binding it needs the wearer's spawned skeleton ([`AvatarBody`]),
//! the bake-on-mesh face materials (`BomFace`) and the GPU skinning slot
//! (`gpu_avatars::GpuSkinBinding`). That is the whole
//! reason this module is separate from [`crate::objects`]: the object layer
//! builds prims without ever naming an avatar, and everything that could not
//! hold to that rule lives here instead.
//!
//! Three things happen here, in the order the wire makes them possible:
//!
//! - [`adopt_pending_attachments`] parents a tracked attachment to its wearer's
//!   attachment-point node (or routes a HUD attachment onto the screen-space HUD
//!   layer), once that avatar's skeleton exists.
//! - [`apply_rigged_attachments`] binds a worn *rigged* mesh — one whose vertices
//!   are weighted to joint names rather than seated at a point — to the wearer's
//!   skeleton instance, spawning its skinned submeshes.
//! - `spawn_animesh_control_avatars` / `prune_control_avatars` give a rigged
//!   *object* (animesh) its own control avatar to be posed against, and reap it
//!   when the object goes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::app::Propagate;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, DecodedMesh, JointOverrides, MeshKey, MeshSkin, ObjectKey, PrimFaceId,
    ScopedObjectId, SlIdentity, TextureFace, TextureKey, Uuid, avatar_texture,
    decode_texture_entry, rigged_inverse_bindposes, texture_face_uv_transform, to_bevy_rigged_mesh,
};

use crate::animesh::{ControlAvatarState, animesh_root};
use crate::asset_budget::MeshUploadBudget;
use crate::avatars::{AvatarBody, BomFace, bom_face_material, log_avatar_faces_enabled};
use crate::face_material::FaceMaterial;
use crate::geometry_cache::GeometryCache;
use crate::meshes::MeshManager;
use crate::objects::{PendingBuilds, PendingGeometry, PrimFaceEntity, WornPickTarget};
use crate::textures::{PrimTextures, TextureAlpha, TextureManager, face_material};
use crate::world_api::{
    AVATAR_BOOST_PRIORITY, AvatarPickTarget, AvatarState, DecodedTextures, HUD_RENDER_LAYER,
    HudState, ObjectState, is_hud_point,
};

/// Parent every tracked attachment that is not yet parented to its avatar's
/// attachment-point node (P16.1/P16.2), so it follows the posed skeleton at the
/// stored local offset rather than sitting at a fixed world offset — or, for one
/// worn on a HUD point, route it out of the world scene onto the screen-space HUD
/// layer (P35.1).
///
/// Attachments arrive in the same object stream as everything else but hang off a
/// **pcode-47 avatar** (not a prim linkset), so `apply_object` holds them
/// parentless and this system — running after the avatars (and their skeleton
/// instances) are spawned — resolves each one's target from the avatar's rigged
/// body: its raw attachment-point id maps to that avatar's attachment-point node
/// entity (`AvatarState::attachment_point_entity`), a child of the skeleton
/// joint carrying the fixed `avatar_lad.xml` offset (P16.2), onto which the
/// object's own local transform composes. An attachment whose avatar / point node
/// is not present yet (a sphere-only avatar, or the avatar simply not spawned yet)
/// stays pending and is retried on a later frame.
///
/// When no `--viewer-assets` avatar body is loaded the avatars are placeholder
/// spheres with no skeleton, so an attachment instead falls back to the avatar's
/// own object entity (its previous, position-only parent) so it at least tracks
/// the avatar's location.
///
/// A **HUD** attachment (`is_hud_point`) takes neither path: its point hangs off
/// the reference viewer's `mScreen` pseudo-joint, not the skeleton, so it is
/// parented to the [`HudState`] node for its point — the screen-space subtree
/// (P35.1) — and only when the wearer is the agent itself. Another avatar's HUD
/// attachment is hidden instead: `LLVOAvatar::initAttachmentPoints` creates the
/// HUD joints for `isSelf()` alone, so there such an object never attaches and
/// never renders, and it must not become world geometry here either. Both are
/// terminal: the object is marked parented (routed) and not retried.
pub fn adopt_pending_attachments(
    mut state: ResMut<ObjectState>,
    avatars: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    hud: Res<HudState>,
    identity: Res<SlIdentity>,
    mut commands: Commands,
) {
    // Snapshot the pending attachments first so the target lookup can read
    // `state.objects` immutably (for the sphere-mode fallback) before the
    // `parented` flag is set.
    let pending: Vec<(ScopedObjectId, Entity, u8, ScopedObjectId)> = state
        .objects
        .iter()
        .filter_map(|(&scoped, tracked)| {
            let point_id = tracked.attachment_point?;
            (!tracked.parented).then_some((scoped, tracked.entity, point_id, tracked.parent))
        })
        .collect();
    for (scoped, entity, point_id, avatar) in pending {
        if is_hud_point(point_id) {
            // The wearer's agent id, needed to tell our own HUD from someone else's.
            // An attachment can arrive before the avatar object it hangs off, so an
            // unresolved wearer is retried next frame rather than taken for a
            // stranger's (which would hide our own HUD for the whole session).
            let Some(agent) = avatars.agent_of(avatar) else {
                continue;
            };
            let own = identity.agent_id == Some(agent);
            route_hud_attachment(
                &mut state,
                scoped,
                entity,
                point_id,
                own,
                &hud,
                &mut commands,
            );
            continue;
        }
        let target = match body.as_deref() {
            // Rigged body: parent to the avatar's attachment-point node, which sits
            // at the stored `avatar_lad.xml` offset from its skeleton joint, so the
            // attachment's own local transform seats it correctly (P16.1/P16.2).
            Some(_body) => avatars.attachment_point_entity(avatar, point_id),
            // Sphere-only avatars (no assets): fall back to the avatar's object
            // entity so the attachment at least follows its position.
            None => state.objects.get(&avatar).map(|tracked| tracked.entity),
        };
        if let Some(target) = target {
            commands.entity(entity).insert(ChildOf(target));
            if let Some(tracked) = state.objects.get_mut(&scoped) {
                tracked.parented = true;
            }
            debug!("parented attachment {scoped} (point {point_id}) to avatar {avatar} joint");
        }
    }
}

/// Route one attachment worn on a HUD point (P35.1): parent the agent's **own**
/// HUD to the screen node for its point, and hide anyone else's (or any HUD at
/// all when the run has no avatar assets, so no HUD screen was spawned).
///
/// Either way the object leaves the world scene — a HUD's local transform is
/// relative to the screen, so left in the world it would sit as loose geometry at
/// the region origin, which is exactly what it did before this phase. Both
/// outcomes are terminal, so the caller marks it routed and stops retrying.
fn route_hud_attachment(
    state: &mut ObjectState,
    scoped: ScopedObjectId,
    entity: Entity,
    point_id: u8,
    own: bool,
    hud: &HudState,
    commands: &mut Commands,
) {
    match hud.point_entity(point_id).filter(|_node| own) {
        Some(node) => {
            // Override the world-geometry probe layers this object got at spawn
            // with the HUD layer, so the HUD subtree renders on the HUD camera
            // (not the world / probe cameras) — a child's own `Propagate` wins
            // over the HUD screen's propagation, so it must be set explicitly.
            commands.entity(entity).insert((
                ChildOf(node),
                Propagate(RenderLayers::layer(HUD_RENDER_LAYER)),
            ));
            debug!("routed own HUD attachment {scoped} to HUD point {point_id}");
        }
        None => {
            commands.entity(entity).insert(Visibility::Hidden);
            debug!(
                "hid HUD attachment {scoped} (point {point_id}): {}",
                if own {
                    "no HUD screen (no avatar assets)"
                } else {
                    "not the agent's own"
                }
            );
        }
    }
    if let Some(tracked) = state.objects.get_mut(&scoped) {
        tracked.parented = true;
    }
}
/// Whether worn rigged meshes' joint position overrides (R1) are applied to the
/// avatar skeleton. On by default; `SL_VIEWER_JOINT_OVERRIDES=0` disables it, so
/// the pre-override skeleton behaviour can be compared side by side in one
/// session.
#[must_use]
pub fn joint_overrides_enabled() -> bool {
    std::env::var("SL_VIEWER_JOINT_OVERRIDES").as_deref() != Ok("0")
}

/// Whether the worn-attachment bind trace is enabled
/// (`SL_VIEWER_LOG_ATTACHMENT_BIND=1`): logs, once per reason-change, why each
/// worn rigged attachment is not yet bound, so an attachment that never binds
/// (boots / hair missing while the rest of the avatar draws — roadmap
/// viewer-mesh-hair-not-rendering) reveals *which* stall it is stuck on — the
/// mesh not decoding, an in-flight LOD upgrade, an unresolved wearer, or the
/// wearer's body not spawned — instead of retrying silently every frame.
pub(crate) fn log_attachment_bind_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_ATTACHMENT_BIND").as_deref() == Ok("1")
}

/// Diagnostic state for `log_attachment_bind_enabled`: the last-logged
/// not-yet-bound reason per worn rigged attachment, so [`apply_rigged_attachments`]
/// logs a stuck attachment's reason once per change rather than every frame, and
/// re-logs when the reason advances (progress) or clears when it finally binds.
#[derive(Debug, Resource, Default)]
pub struct RiggedBindSkipLog(HashMap<ScopedObjectId, &'static str>);

impl RiggedBindSkipLog {
    /// Record `reason` as `scoped`'s current not-yet-bound reason, returning
    /// `true` when it changed since last time — so a caller that wants to log
    /// something richer than [`note`](Self::note) still fires exactly once per
    /// reason-change.
    fn changed(&mut self, scoped: ScopedObjectId, reason: &'static str) -> bool {
        self.0.insert(scoped, reason) != Some(reason)
    }

    /// Note that `scoped` is still unbound for `reason`, logging it only when the
    /// reason changed since last time (the caller has already checked the trace is
    /// enabled).
    fn note(&mut self, scoped: ScopedObjectId, reason: &'static str) {
        if self.changed(scoped, reason) {
            info!("rigged attachment {scoped} not yet bound: {reason}");
        }
    }

    /// Forget `scoped`'s stall reason — it bound (or is gone), so a later
    /// re-attach traces afresh.
    fn bound(&mut self, scoped: ScopedObjectId) {
        let _prev = self.0.remove(&scoped);
    }
}

/// One-word description of a tracked object's geometry-pending state, for the
/// wearer-walk terminus diagnostic (`log_attachment_bind_enabled`).
const fn pending_kind(pending: Option<&PendingGeometry>) -> &'static str {
    match pending {
        None => "built",
        Some(PendingGeometry::Mesh(_)) => "Mesh-pending",
        Some(PendingGeometry::Sculpt(_)) => "Sculpt-pending",
        Some(PendingGeometry::RiggedMesh(_)) => "RiggedMesh-pending",
    }
}

/// Bind every worn rigged mesh attachment whose skeleton instance is now
/// available (P17.2): for each object holding a `PendingGeometry::RiggedMesh`,
/// resolve the wearer avatar's skeleton-instance joint entities and spawn the
/// mesh's skinned submeshes bound to them, so the mesh deforms with the avatar
/// rather than sitting rigidly at an attachment point.
///
/// A rigged mesh's build is deferred here (rather than in
/// [`apply_object_meshes`](crate::objects::apply_object_meshes))
/// because it needs the wearer's spawned skeleton — which can arrive before or
/// after the mesh decodes. The pending build is retried each frame until the
/// avatar's rigged body (`AvatarState::is_rigged`) is present; an avatar
/// with no rigged body (a sphere-only, no-`--viewer-assets` run) never resolves,
/// so the mesh simply stays unbuilt there. Each rig joint name is mapped to the
/// avatar's matching skeleton joint entity (`AvatarBody::joint_index`), falling
/// back to the pelvis for a name the skeleton lacks (the reference viewer's
/// unknown-joint fallback). The object is marked parented so
/// [`adopt_pending_attachments`] does not also pin it to a rigid
/// attachment-point node.
///
/// Budgeted: skinned builds spend from the shared [`MeshUploadBudget`], so
/// a crowd's rigged bodies bind over several frames; the not-yet-built rest
/// stays pending and is re-collected next frame (the cheap not-ready retries
/// — skeleton or finest LOD still loading — are free, as before).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system joining the object, avatar, and mesh state with the ECS resources the skinned build needs"
)]
pub fn apply_rigged_attachments(
    mut state: ResMut<ObjectState>,
    mut builds: PendingBuilds,
    mut avatars: ResMut<AvatarState>,
    mut control: ResMut<ControlAvatarState>,
    body: Option<Res<AvatarBody>>,
    mesh_manager: Res<MeshManager>,
    mut budget: ResMut<MeshUploadBudget>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut manager: ResMut<TextureManager>,
    store: Res<DecodedTextures>,
    mut prim_textures: ResMut<PrimTextures>,
    mut cache: ResMut<GeometryCache>,
    mut skip_log: ResMut<RiggedBindSkipLog>,
) {
    // The worn-attachment bind trace (off unless SL_VIEWER_LOG_ATTACHMENT_BIND=1),
    // read once so the per-object skips below cost nothing in the normal case.
    let trace = log_attachment_bind_enabled();
    // Without loaded avatar assets there are no rigged bodies to bind to.
    let Some(body) = body else {
        return;
    };
    // Snapshot the objects whose rigged build is pending, so the per-object reads
    // below can borrow `builds` immutably before the final update.
    let pending =
        builds.scoped_pending_on(|pending| matches!(pending, PendingGeometry::RiggedMesh(_)));
    for (scoped, entity) in pending {
        // A skinned build is among the heaviest per-object costs (submesh
        // meshes + inverse bindposes + skeleton binding); spend from the
        // shared decode-apply budget so a crowd's worth of rigged bodies
        // binds over several frames. The unbuilt rest stays pending and is
        // re-collected next frame.
        if budget.remaining == 0 {
            break;
        }
        if !state.objects.contains_key(&scoped) {
            continue;
        }
        let Some(PendingGeometry::RiggedMesh(build)) = builds.pending(entity) else {
            continue;
        };
        let key = build.key;
        // A rigged mesh that started on the managed coarse-LOD path (an animesh, or
        // an attachment whose worn status resolved late) is being upgraded to the
        // finest block asynchronously (`upgrade_to_finest`). Building now would bind
        // the coarse geometry, and a rigged mesh is not on the LOD-swap rebuild path
        // (`apply_object_meshes`), so it would stay frozen at that block — an animesh
        // rendered from its few-vertex coarsest LOD (P29). Wait for the finest decode.
        if mesh_manager.lod_change_inflight(key) {
            if trace {
                skip_log.note(scoped, "finest-LOD upgrade in flight");
            }
            continue;
        }
        // Resolve the skeleton this rigged mesh binds to: an animated object
        // (animesh) drives its OWN control-avatar skeleton (P29), spawned on demand
        // as a child of the linkset root; every other rigged mesh binds to the
        // wearer avatar it hangs off. The `bind_agent` (the wearer, keying its baked
        // textures for a bake-on-mesh face) is `None` for an animesh — an animated
        // object has no wearer bake, so its faces texture from ordinary fetches.
        let animesh = animesh_root(&state, scoped);
        let (root, joints, bind_agent, slot) = if let Some((object, object_entity)) = animesh {
            // The animesh control avatar's root (a child of the linkset root, so
            // the skeleton tracks the object). Phase 4/§5: no per-object joint
            // entities — the submeshes bind every palette slot to the shared
            // dummy and are GPU-posed in place on the `Animesh` pose slot.
            let root = control.ensure_spawned(object, object_entity, &mut commands);
            let joints = vec![body.dummy_joint(); body.skeleton_joint_count()];
            (
                root,
                joints,
                None,
                crate::world_api::PoseSlotKey::Animesh(object),
            )
        } else {
            // The wearer avatar, found by chasing this mesh's parent links up to the
            // avatar root — a mesh body is worn as a multi-prim linkset whose parts
            // parent to the linkset root prim, not the avatar directly, so its direct
            // `parent` is not the avatar (P17.2 fix; verified live on a real mesh body).
            let Some(agent) = avatars.wearer_of(scoped) else {
                if trace && skip_log.changed(scoped, "wearer avatar not resolved (parent chain)") {
                    // Classify the failure by walking the parent chain to where it
                    // stopped: a *tracked in-world* terminus means the object is
                    // genuinely not worn (an in-world rigged mesh that should never
                    // be in the attachment bind); an *untracked* terminus means the
                    // wearer / linkset-root object never arrived (a parenting gap).
                    match avatars.avatar_root_walk(scoped) {
                        Ok(_resolved) => {}
                        Err((terminus, hops)) => {
                            let kind = match state.objects.get(&terminus) {
                                Some(tracked) => format!(
                                    "tracked in-world object (is_root={}, attach_point={:?}, {})",
                                    tracked.is_root,
                                    tracked.attachment_point,
                                    pending_kind(builds.pending(tracked.entity)),
                                ),
                                None => {
                                    "UNTRACKED — its parent/root object never arrived".to_owned()
                                }
                            };
                            // Attribute the stuck attachment to the avatar that wears
                            // it: the update's `owner_id` is the wearer, so resolving
                            // it to a spawned avatar's name (when present) names which
                            // avatar is rendering wrong — e.g. a mesh head whose root
                            // is one of the UNTRACKED termini.
                            let (attach, owner) = state.objects.get(&scoped).map_or_else(
                                || ("?".to_owned(), "?".to_owned()),
                                |worn| {
                                    let owner = avatars.name_of(worn.owner_id).map_or_else(
                                        || worn.owner_id.to_string(),
                                        |name| format!("{name} [{}]", worn.owner_id),
                                    );
                                    (format!("{:?}", worn.attachment_point), owner)
                                },
                            );
                            info!(
                                "rigged attachment {scoped} (attach_point={attach}, \
                                 owner={owner}): wearer unresolved after {hops} hop(s); \
                                 chain terminus {terminus} is {kind}"
                            );
                        }
                    }
                }
                continue;
            };
            // The wearer's rigged body; retry next frame if the avatar (or its
            // body) is not spawned yet. Phase 4 removed the per-avatar joint
            // entities: a worn rig's `SkinnedMesh` binds every palette slot to
            // the single shared dummy joint (its wearer's canonical joint indices
            // ride the `GpuSkinBinding` below), so a full-skeleton-length vec of
            // dummies is all the skin mapping needs — `joints.get(index)` still
            // resolves any valid skeleton index to a (dummy) entity.
            let Some(root) = avatars
                .body_root_of(agent)
                .filter(|_| avatars.is_rigged(agent))
            else {
                if trace {
                    skip_log.note(scoped, "wearer body / skeleton not spawned yet");
                }
                continue;
            };
            let joints = vec![body.dummy_joint(); body.skeleton_joint_count()];
            (
                root,
                joints,
                Some(agent),
                crate::world_api::PoseSlotKey::Avatar(agent),
            )
        };
        let Some(fallback) = joints.first().copied() else {
            if trace {
                skip_log.note(scoped, "wearer skeleton has no joints");
            }
            continue;
        };
        // The decoded geometry + skin, cloned out so the immutable `mesh_manager`
        // borrow ends before the build borrows the other resources mutably.
        let (Some(decoded), Some(skin)) = (
            mesh_manager.decoded(key).map(Arc::clone),
            mesh_manager.skin(key).map(Arc::clone),
        ) else {
            if trace {
                skip_log.note(scoped, "attachment mesh / skin not decoded yet");
            }
            continue;
        };
        // Resolve the rig's own joint-name table against the avatar's skeleton
        // instance (unknown names fall back to the pelvis joint).
        // Map each rig joint name to the avatar's skeleton-instance joint entity;
        // an unresolved name (a bone or collision volume the skeleton lacks) falls
        // back to the pelvis, which would misplace those vertices, so it is logged.
        let mut unresolved: Vec<&str> = Vec::new();
        // The canonical skeleton joint index of each palette slot, built in
        // lockstep with the joint entities for the GPU-avatar real-skin
        // resolver (Phase 4 groundwork, `crate::gpu_avatars::GpuSkinBinding`).
        // An unresolved name binds to the pelvis `fallback` entity (joint 0),
        // so its canonical entry is 0 too — the canonical index always matches
        // the entity that palette slot actually skins to.
        let mut canonical: Vec<u32> = Vec::with_capacity(skin.joint_names.len());
        let joint_entities: Vec<Entity> = skin
            .joint_names
            .iter()
            .map(|name| {
                match body
                    .joint_index(name)
                    .and_then(|index| joints.get(index).copied().map(|entity| (index, entity)))
                {
                    Some((index, entity)) => {
                        canonical.push(u32::try_from(index).unwrap_or(0));
                        entity
                    }
                    None => {
                        unresolved.push(name.as_str());
                        canonical.push(0);
                        fallback
                    }
                }
            })
            .collect();
        if !unresolved.is_empty() {
            warn!(
                "rigged mesh {key}: {}/{} joint(s) unresolved, bound to pelvis: {:?}",
                unresolved.len(),
                skin.joint_names.len(),
                unresolved
            );
        }
        let texture_entry = build.texture_entry.clone();
        // The wearer's agent id (when known) keys its baked textures (P17.3): a
        // bake-on-mesh face is textured from the wearer's own bake, not a fetch. An
        // animesh has no wearer bake (`bind_agent` is `None`), so its faces texture
        // from ordinary fetches.
        let face_entities = build_rigged_submeshes(
            &decoded,
            &skin,
            &joint_entities,
            &canonical,
            &texture_entry,
            root,
            slot,
            bind_agent,
            // A worn mesh's submeshes carry their worn-object identity for the
            // attachment pies; an animesh (`bind_agent` `None`) is not worn.
            bind_agent.is_some().then_some(scoped),
            key,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut bindposes,
            &mut manager,
            &store,
            &mut prim_textures,
            &mut cache,
        );
        budget.remaining = budget.remaining.saturating_sub(1);
        if trace {
            // Bound (or built as empty — the `rendered no geometry` warn covers
            // that): stop tracing this attachment so a later re-attach starts fresh.
            skip_log.bound(scoped);
        }
        // The rigged build is done with (a re-tessellation would establish a fresh
        // one); dropping it also drops the object's whole queue entry, since a
        // rigged mesh carries no LOD-rebuild inputs.
        let _built = builds.take_pending(entity);
        builds.drop_if_resolved(entity, &mut commands);
        if let Some(tracked) = state.objects.get_mut(&scoped) {
            tracked.face_entities = face_entities;
            // The skinned mesh follows the skeleton joints directly, so the object
            // must not also be pinned to a rigid attachment-point node.
            tracked.parented = true;
        }
        // Fold the rig's joint position overrides into the skeleton it binds to (R1):
        // a fitted mesh body/head repositions the joints its inverse-bind matrices
        // were baked against, so without these the mesh distorts at the extremities.
        // For an animesh they go onto its own control avatar; for a worn mesh onto
        // the wearer (flagging it for a skeleton re-deform, the reference viewer's
        // `addAttachmentOverridesForObject`). Suppressible via
        // `SL_VIEWER_JOINT_OVERRIDES=0` (A/B against the pre-override behaviour).
        let overrides = if joint_overrides_enabled() {
            body.joint_overrides(&skin)
        } else {
            JointOverrides::default()
        };
        if !overrides.is_empty() {
            debug!(
                "rigged mesh {key}: {} joint position override(s), lock_scale={}",
                overrides.len(),
                overrides.lock_scale()
            );
        }
        // Whether this rig is *fitted* (P34.3): how many of the joints it binds are
        // collision volumes, and how many of those its own joint positions override
        // — an overridden volume is pinned by the rig and ignores the shape's volume
        // morphs (the reference does the same: `LLJoint::setPosition` yields to an
        // active attachment override). A rig that binds no volume at all cannot
        // follow the chest / belly / butt sliders however faithfully they resolve.
        let volumes_bound = skin
            .joint_names
            .iter()
            .filter(|name| body.is_collision_volume(name))
            .count();
        if volumes_bound > 0 {
            let volumes_pinned = skin
                .joint_names
                .iter()
                .filter(|name| {
                    body.is_collision_volume(name)
                        && body
                            .joint_index(name)
                            .is_some_and(|joint| overrides.position(joint).is_some())
                })
                .count();
            // Binding a volume means nothing if the rig puts no *weight* on it, so
            // report the share of the skin's total weight mass that rides the
            // collision volumes: that — not the joint list — is what decides whether
            // the shape's volume morphs move this mesh at all.
            let is_volume_slot: Vec<bool> = skin
                .joint_names
                .iter()
                .map(|name| body.is_collision_volume(name))
                .collect();
            let mut volume_mass = 0.0_f32;
            let mut total_mass = 0.0_f32;
            for submesh in &decoded.submeshes {
                let Some(weights) = submesh.weights.as_ref() else {
                    continue;
                };
                for vertex in weights {
                    for &(slot, weight) in &vertex.influences {
                        total_mass += weight;
                        if is_volume_slot.get(usize::from(slot)).copied() == Some(true) {
                            volume_mass += weight;
                        }
                    }
                }
            }
            let share = if total_mass > 0.0 {
                100.0 * volume_mass / total_mass
            } else {
                0.0
            };
            debug!(
                "rigged mesh {key}: binds {volumes_bound} collision volume(s), \
                 {volumes_pinned} pinned by its own joint overrides, \
                 {share:.1}% of its skin weight rides them"
            );
        } else {
            debug!("rigged mesh {key}: binds no collision volume (not a fitted rig)");
        }
        match (animesh, bind_agent) {
            (Some((object, _entity)), _) => control.record_overrides(object, key.uuid(), overrides),
            (None, Some(agent)) => {
                avatars.record_joint_overrides(agent, key.uuid(), overrides);
                // Record the worn rigged mesh for the avatar-state dump
                // (viewer-avatar-state-dump-replay).
                avatars.record_worn_rigged_mesh(agent, key.uuid());
            }
            (None, None) => {}
        }
        debug!("bound rigged mesh {key} to its skeleton");
    }
}

/// Spawn a control avatar (P29) for every tracked animesh root any part of whose
/// linkset has an animation playing, as soon as the animation arrives — rather
/// than waiting for its rigged mesh to bind (`apply_rigged_attachments`), which
/// can be many seconds later once the mesh's finest LOD decodes. The reference
/// viewer creates an `LLControlAvatar` when the object is detected as animated,
/// not when its geometry loads; spawning early means an `ObjectAnimation` that
/// arrives before the mesh decode is captured and posed the moment the mesh
/// binds, instead of being lost in the gap. The skeleton is invisible until a
/// mesh binds to it, so this only materialises for animesh we are actually
/// animating.
///
/// Each signalled **part** resolves to its flagged root through
/// [`animesh_root`] (P29.2): the sim keys `ObjectAnimation` by the prim holding
/// the animations, which is often a plain (un-flagged) linkset child — keying
/// the spawn on the root itself being signalled left every such animesh
/// permanently un-posed.
pub(crate) fn spawn_animesh_control_avatars(
    state: Res<ObjectState>,
    mut control: ResMut<ControlAvatarState>,
    body: Option<Res<AvatarBody>>,
    mut commands: Commands,
) {
    // Only spawn control avatars when the avatar asset library (the shared
    // skeleton the GPU rest solve needs) is present.
    if body.is_none() {
        return;
    }
    let parts = control.signalled_parts();
    let scoped_by_full = state.scoped_by_full_keys(&parts);
    let roots: HashSet<(ObjectKey, Entity)> = scoped_by_full
        .values()
        .filter_map(|&scoped| animesh_root(&state, scoped))
        .collect();
    for (key, entity) in roots {
        let _spawned = control.ensure_spawned(key, entity, &mut commands);
    }
}

/// Drop the control avatar of every animesh (P29) whose root object is no longer
/// tracked — removed, or its region left. The skeleton entities parent under the
/// object entity, so Bevy's recursive despawn already took them with the object;
/// this only clears the stale [`ControlAvatarState`] bookkeeping, so a re-rez
/// rebuilds a fresh control avatar.
///
/// The **signalled-animation sets are deliberately not pruned here** (P29.2):
/// an `ObjectAnimation` routinely arrives before its part's first
/// `ObjectUpdate` (and for a part we may never track at all), and pruning the
/// set by tracked-object liveness destroyed that early-arrival buffer the same
/// frame it was folded — the event cursor had advanced, so the animation was
/// gone for good. The reference keeps its signalled map for the session and
/// re-reads it whenever a control avatar is (re)built; only a safety cap
/// bounds ours (`ControlAvatarState::bound_signalled`).
pub(crate) fn prune_control_avatars(
    state: Res<ObjectState>,
    mut control: ResMut<ControlAvatarState>,
) {
    let live: HashSet<ObjectKey> = state
        .objects
        .values()
        .map(|tracked| tracked.full_key)
        .collect();
    control.retain(|object| live.contains(&object));
    control.bound_signalled(|part| live.contains(&part));
}

/// Spawn one skinned child entity per non-empty submesh of a decoded rigged mesh
/// under the wearer avatar's body `root` (P17.2), each a Bevy `SkinnedMesh` bound
/// to the shared `joint_entities` (the avatar's skeleton-instance joints, in the
/// skin's `joint_names` order) and the mesh's own inverse bindposes, textured per
/// submesh via the Phase-6 pipeline exactly as the static mesh path is. Returns
/// the spawned entities so a detach (or the avatar leaving) can despawn them.
///
/// All submeshes share the mesh's single skin, so the inverse bindposes are built
/// once. The skinned vertices are computed in world space from the joint entities'
/// global transforms, so the entities are parented under the avatar body root only
/// for lifecycle and visibility — their own `Transform` does not place them.
///
/// The converted submesh [`Mesh`]es and the [`SkinnedMeshInverseBindposes`] are
/// shared across wearers through the [`GeometryCache`] rigged slots — both are
/// pure functions of the decoded asset, and sharing the *asset handles* is what
/// lets Bevy batch N wearers of the same body into one instanced draw per
/// submesh (batching keys on the mesh asset). The per-wearer state stays
/// per-entity: `SkinnedMesh::joints` is this wearer's own skeleton-instance
/// joint entities, and the per-face materials / bake textures are built per
/// spawn as before.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the several ECS resources the skinned build needs"
)]
fn build_rigged_submeshes(
    decoded: &DecodedMesh,
    skin: &MeshSkin,
    joint_entities: &[Entity],
    canonical: &[u32],
    texture_entry: &[u8],
    root: Entity,
    slot: crate::world_api::PoseSlotKey,
    agent: Option<AgentKey>,
    worn: Option<ScopedObjectId>,
    mesh_key: MeshKey,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FaceMaterial>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    manager: &mut TextureManager,
    store: &DecodedTextures,
    prim_textures: &mut PrimTextures,
    cache: &mut GeometryCache,
) -> Vec<Entity> {
    let entry = decode_texture_entry(texture_entry, decoded.submeshes.len());
    // The slot every face falls back to when the object carries no texture entry.
    let default_face = TextureFace::new(TextureKey::from(Uuid::nil()));
    // Shared across wearers of the same mesh asset at this decoded level: the
    // second wearer revives the first wearer's asset instead of minting its own.
    let rigged_key = (mesh_key, decoded.lod);
    let inverse_bindposes = cache
        .revive_rigged_bindposes(rigged_key, bindposes)
        .unwrap_or_else(|| {
            let built = bindposes.add(SkinnedMeshInverseBindposes::from(rigged_inverse_bindposes(
                skin,
            )));
            cache.record_rigged_bindposes(rigged_key, built.id());
            built
        });
    let log_faces = agent.is_some() && log_avatar_faces_enabled();
    // The GPU-avatar real-skin binding's canonical joint map (Phase 4
    // groundwork), shared by every submesh of this rig (they all skin to the
    // one `joint_names` list). Interned into an `Arc` once so each spawned
    // submesh clones a handle rather than the slice.
    let canonical_binding: Arc<[u32]> = Arc::from(canonical);
    let mut face_entities = Vec::new();
    for (index, submesh) in decoded.submeshes.iter().enumerate() {
        // Skip a submesh with no renderable geometry — the explicit `NoGeometry`
        // marker *or* a `no_geometry == false` submesh that still decoded to zero
        // vertices. Converting the latter yields a zero-vertex mesh the GPU mesh
        // allocator floods the log over (viewer-r26) and which renders nothing.
        if !submesh.has_geometry() {
            continue;
        }
        // Revive the shared converted submesh, or convert once and record it
        // for the next wearer. The handle — not a per-wearer copy — is what
        // Bevy's draw batching keys on.
        let mesh = cache
            .revive_rigged_submesh(rigged_key, index, meshes)
            .unwrap_or_else(|| {
                let converted = meshes.add(to_bevy_rigged_mesh(submesh, skin));
                cache.record_rigged_submesh(rigged_key, index, converted.id());
                converted
            });
        let texture_face = entry.face(index).unwrap_or(&default_face);
        // A bake-on-mesh face (P17.3): its texture id is an `IMG_USE_BAKED_*`
        // sentinel meaning "show the wearer's own baked skin here". Tag it [`BomFace`]
        // (carrying the face's TE tint + UV) so `apply_bom_face_materials` textures it
        // from the wearer's own bake with the reference viewer's per-face tint / hide /
        // blend (R22) — never fetch the sentinel, which is not a real texture (the
        // P17.2 invisible-shell finding). Only when the wearer's agent is known;
        // otherwise fall through to a plain fetch.
        let bom = agent.and_then(|agent| {
            avatar_texture::use_baked_slot(texture_face.texture_id).map(|slot| {
                BomFace::new(
                    agent,
                    slot,
                    texture_face.color,
                    texture_face_uv_transform(texture_face),
                )
            })
        });
        if log_faces {
            log_rigged_face(mesh_key, index, texture_face, bom.as_ref());
        }
        let material = match &bom {
            // A BoM face owns its material so `apply_bom_face_materials` can give it
            // the reference per-face tint / blend / hide on the sampled bake; until
            // then it shows the neutral fallback (not the reddish skin placeholder).
            Some(bom) => materials.add(bom_face_material(bom.tint(), bom.uv())),
            // A rigged mesh is always a worn attachment, so its face textures are
            // boosted (P20.2) — its skinned entity transform does not reflect its
            // on-screen size, so the pixel-area pass cannot rank it.
            None => face_material(
                texture_face,
                materials,
                manager,
                store,
                prim_textures,
                AVATAR_BOOST_PRIORITY,
                // A rigged face cannot alpha-mask (reference: `canRenderAsMask` is
                // false for rigged), so one with a genuinely transparent texture
                // alpha-*blends* — hair / eyelashes render soft, not as a solid card.
                // A texture with no real transparency stays opaque. (A rigged face
                // sampling a 5-channel bake is the separate BoM path in avatars.rs.)
                TextureAlpha::Blend,
            ),
        };
        // The submesh index is the Linden face index; a mesh has few faces, so the
        // widening never saturates in practice (a clamp keeps it lint-clean).
        let face_id = PrimFaceId::new(u16::try_from(index).unwrap_or(u16::MAX));
        let mut spawned = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            SkinnedMesh {
                inverse_bindposes: inverse_bindposes.clone(),
                joints: joint_entities.to_vec(),
            },
            // Frustum culling is driven by the GPU-computed posed AABB
            // (`crate::gpu_avatars::stage::apply_gpu_avatar_bounds`) via this
            // submesh's `GpuSkinBinding` slot: its drawn vertices live wherever
            // the GPU palettes put them, not at this entity's transform, so the
            // real posed bound is read back and set on its `Aabb` rather than
            // opting culling out (Phase 5, retiring `NoFrustumCulling`).
            Transform::default(),
            Visibility::default(),
            PrimFaceEntity { face_id },
            ChildOf(root),
        ));
        if let Some(bom) = bom {
            spawned.insert(bom);
        }
        // A worn rigged submesh is part of its wearer's drawn silhouette — on a
        // modern mesh-body avatar the *base* body is hidden, so without this tag
        // the wearer would have no pickable geometry in the GPU pick view
        // (`sl_viewer_world_view::gpu_pick`). An animesh (no wearer, `agent` `None`) is not an
        // avatar and stays untagged, matching the reference viewer's
        // control-avatar exclusion from avatar picking.
        if let Some(agent) = agent {
            spawned.insert(AvatarPickTarget::new(agent));
        }
        // The GPU-avatar real-skin binding (§1.1): the pose slot + per-slot
        // canonical joint indices, so pass D resolves this submesh's palette in
        // place. Both a worn avatar rig (`PoseSlotKey::Avatar`) and an animesh
        // control-avatar submesh (`PoseSlotKey::Animesh`) bind it — only the
        // avatar pick target above distinguishes them.
        spawned.insert(crate::gpu_avatars::GpuSkinBinding {
            slot,
            canonical: Arc::clone(&canonical_binding),
        });
        // …and the worn-object identity beside it, so that same pick can route a
        // hit on this submesh to the attachment pies (`crate::attachment_menu`)
        // instead of the wearer's plain avatar pie.
        if let Some(scoped) = worn {
            spawned.insert(WornPickTarget { scoped });
        }
        face_entities.push(spawned.id());
    }
    // Every submesh was dropped as empty above, so this worn rigged attachment
    // renders NOTHING — the "boots / hair missing while the rest of the avatar
    // draws" symptom (roadmap viewer-mesh-hair-not-rendering). Surface it, with the
    // decoded LOD and per-submesh vertex counts, so an intermittent and otherwise
    // silent drop is observable: it distinguishes a genuinely-empty chosen LOD
    // block (the author put no geometry there — the fix is a coarser-LOD fallback)
    // from an `sl-mesh` decode gap yielding zero vertices for a block that has data
    // (a decoder bug to fix, not mask). Rigged meshes are forced to the finest
    // block (`upgrade_to_finest`), so this is the finest available LOD.
    if face_entities.is_empty() && !decoded.submeshes.is_empty() {
        let vertex_counts: Vec<usize> = decoded
            .submeshes
            .iter()
            .map(|submesh| submesh.positions.len())
            .collect();
        warn!(
            "rigged attachment {mesh_key} rendered no geometry: all {} submesh(es) \
             empty at LOD {:?} (vertex counts {vertex_counts:?}); agent={agent:?} worn={worn:?}",
            decoded.submeshes.len(),
            decoded.lod,
        );
    }
    face_entities
}

/// Log one rigged-mesh face's `TextureEntry` for BoM diagnostics (R22, gated by
/// `SL_VIEWER_LOG_AVATAR_FACES=1`): its sampled bake slot (an `IMG_USE_BAKED_*`
/// sentinel) or real texture id, plus the tint and UV placement the reference
/// viewer applies. The tint alpha reveals a hidden alpha-cut / "onion shell" layer
/// (`tint a=0`) and the slot reveals whether a mesh body's arm samples the classic
/// `upper` bake or a `universal` (`leftarm`/`aux*`) one.
fn log_rigged_face(mesh_key: MeshKey, index: usize, face: &TextureFace, bom: Option<&BomFace>) {
    let [r, g, b, a] = face.color;
    let source = match bom {
        Some(bom) => format!("BoM slot {}", bom.slot_name()),
        None => format!("tex {}", face.texture_id),
    };
    info!(
        "rigged face {mesh_key} #{index}: {source} tint=({r},{g},{b},a={a}) \
         repeats=({:.3},{:.3}) offset=({:.3},{:.3}) rot={:.3}",
        face.scale_s, face.scale_t, face.offset_s, face.offset_t, face.rotation
    );
}
