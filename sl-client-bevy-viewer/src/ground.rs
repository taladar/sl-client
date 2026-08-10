//! The avatar **ground probe** (P31.14): what is actually under an avatar's feet.
//!
//! The reference viewer's `LLVOAvatar::getGround` casts a short vertical ray around a
//! point (`LLWorld::resolveStepHeightGlobal`, from 1 m above to 1 m below) against the
//! **world** — objects as well as terrain — and returns the surface point and its
//! normal, falling back to the land height when the ray hits nothing. The locomotion
//! foot IK and the landing recovery ([`crate::locomotion_ik`]) both need it: a terrain
//! lookup alone cannot see the prim ramp, staircase or platform an avatar is standing
//! on, and static walkable prims carry no avian collider (only *physical* — i.e.
//! dynamic — objects do), so a physics-engine query cannot see them either.
//!
//! So the probe raycasts the **rendered geometry**, via Bevy's [`MeshRayCast`], which
//! covers every prim, mesh and sculpt face along with the land patches, and it accepts
//! a surface only when it is explicitly ground-like — an object face
//! ([`PrimFaceEntity`]) or a land patch ([`TerrainSurface`]) — and not part of any
//! avatar. Without that filter the ray would happily plant an avatar's feet on its own
//! shoes, the water plane, or a passing particle.
//!
//! It runs as its own system rather than inside the pose pass because [`MeshRayCast`]
//! reads every `GlobalTransform` and the pose pass *writes* them, which Bevy will not
//! let one system do at once. It reads the ankle joints' globals as the pose pass left
//! them **last** frame — a frame of staleness that is invisible at any frame rate the
//! viewer runs at, and the same order of lag the reference's own once-per-frame probe
//! carries.

use std::collections::HashMap;

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use sl_client_bevy::AgentKey;

use crate::avatars::{AvatarBody, AvatarState};
use crate::objects::PrimFaceEntity;
use crate::terrain::TerrainSurface;

/// How far **above** the probed point the ray starts, metres. The reference's
/// `getGround` offsets by `+1` on the global Z axis.
const PROBE_ABOVE: f32 = 1.0;

/// How far **below** the probed point the ray reaches, metres (the reference's `-1`).
/// Deliberately short: the ground is what the avatar is standing *on*, not the land a
/// hundred metres below the skybox platform it is standing on.
const PROBE_BELOW: f32 = 1.0;

/// When the avian terrain hit sits within this many metres of the foot, the avatar
/// is standing on land (or a physical mover), so the still-unaccelerated object
/// `MeshRayCast` is skipped. Kept tight so a low prim step is not mistaken for land.
const ON_LAND_BAND: f32 = 0.15;

/// `SL_VIEWER_GROUND_PROBE_SPATIAL=1` routes the terrain half of the ground probe
/// through avian's BVH-accelerated `SpatialQuery` (Stage 1 of the
/// `viewer-perf-avatar-ground-probe` plan) instead of the unaccelerated
/// whole-scene `MeshRayCast`; the object half still uses `MeshRayCast` until
/// static prims get colliders (Stage 2). Off by default — read once. When off,
/// nothing changes: no terrain colliders are built and the probe casts as before.
pub(crate) fn ground_probe_spatial_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("SL_VIEWER_GROUND_PROBE_SPATIAL")
            .is_ok_and(|value| value == "1" || value == "true")
    })
}

/// One ground sample, in **Bevy world** space: where the surface is, and which way it
/// faces. The caller converts into whatever frame it needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GroundHit {
    /// The point on the surface (Bevy world metres).
    pub(crate) point: Vec3,
    /// The surface's upward unit normal (Bevy world). Always points up-ish: a
    /// back-facing hit is flipped, so a two-sided prim floor still reads as ground.
    pub(crate) normal: Vec3,
}

/// One avatar's ground samples for this frame: under its body root (the reference
/// height the feet are measured against) and under each ankle.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct AgentGround {
    /// The ground under the avatar's body root — the surface it is standing *on*.
    /// [`None`] when the avatar is airborne (nothing within the probe's reach).
    pub(crate) root: Option<GroundHit>,
    /// The ground under the left ankle.
    pub(crate) left: Option<GroundHit>,
    /// The ground under the right ankle.
    pub(crate) right: Option<GroundHit>,
}

/// Each rigged avatar's ground samples, refreshed every frame by
/// [`probe_avatar_ground`] and read by the locomotion adjusters.
#[derive(Resource, Default)]
pub(crate) struct AvatarGround {
    /// The samples of each rigged avatar seen this frame.
    probes: HashMap<AgentKey, AgentGround>,
    /// Where to cast each avatar's two foot rays: its ankles' Bevy-world positions in
    /// the **pre-IK** pose (keyframe + idle + look-at), published by the pose pass.
    ///
    /// This *must not* be the ankles' posed positions, which is what reading the joint
    /// entities' `GlobalTransform`s would give — that closes a feedback loop with a
    /// vicious limit cycle in it. A standing leg is at ~99.5% of full extension, where
    /// the IK's gain is enormous (a 2 cm ankle move is ~50° of knee); when a foot's goal
    /// falls out of the leg's reach the solve straightens the leg and the ankle lands
    /// *short* of the goal, somewhere else horizontally; the next probe therefore samples
    /// the ground somewhere else, the goal comes back into reach, the ankle snaps back —
    /// and the knees buzz. Casting through the pre-IK ankle keeps the probe a function of
    /// the animation alone, which is smooth, so nothing the IK does can perturb its own
    /// input.
    targets: HashMap<AgentKey, (Vec3, Vec3)>,
}

impl AvatarGround {
    /// The ground samples under `agent`, or all-[`None`] if it was not probed.
    #[must_use]
    pub(crate) fn get(&self, agent: AgentKey) -> AgentGround {
        self.probes.get(&agent).copied().unwrap_or_default()
    }

    /// Publish `agent`'s **pre-IK** ankle world positions for the next frame's probe.
    /// Called by the pose pass, which is the only place the un-adjusted pose exists.
    pub(crate) fn set_probe_targets(&mut self, agent: AgentKey, left: Vec3, right: Vec3) {
        let _prev = self.targets.insert(agent, (left, right));
    }
}

/// Cast one ground ray straight down through `point` (Bevy world), returning the
/// surface it lands on. `accept` decides which entities count as ground.
///
/// When `spatial` is `Some` (the `SL_VIEWER_GROUND_PROBE_SPATIAL` fast path), the
/// avian BVH — which covers the terrain (and physical movers) — is cast first: if
/// it reports a surface right at the foot, the avatar is standing on it and the
/// whole-scene `MeshRayCast` is skipped. Otherwise (elevated on a static prim, or
/// the fast path off) it falls through to the `MeshRayCast` as before. Both paths
/// are computed **fresh every frame** — no caching, so no stale-ground foot IK.
fn probe(
    ray_cast: &mut MeshRayCast,
    spatial: Option<&SpatialQuery>,
    filter: &SpatialQueryFilter,
    point: Vec3,
    accept: &(impl Fn(Entity) -> bool + Sync),
) -> Option<GroundHit> {
    let origin = Vec3::new(point.x, point.y + PROBE_ABOVE, point.z);
    if let Some(spatial) = spatial
        && let Some(hit) =
            spatial.cast_ray(origin, Dir3::NEG_Y, PROBE_ABOVE + PROBE_BELOW, true, filter)
        && hit.distance <= PROBE_ABOVE + ON_LAND_BAND
    {
        // Standing on an avian-covered surface (land / physical mover): use it and
        // skip the object MeshRayCast. `normal` is world-space; flip a down-facing
        // hit so a two-sided surface still reads as ground (as the mesh path does).
        // Per-component arithmetic to stay clear of the `arithmetic_side_effects`
        // lint on the glam vector operators; the ray is straight down, so the hit
        // point is the origin dropped by the hit distance.
        let normal = if hit.normal.y < 0.0 {
            Vec3::new(-hit.normal.x, -hit.normal.y, -hit.normal.z)
        } else {
            hit.normal
        };
        return Some(GroundHit {
            point: Vec3::new(origin.x, origin.y - hit.distance, origin.z),
            normal: normal.normalize_or(Vec3::Y),
        });
    }
    let ray = Ray3d::new(origin, Dir3::NEG_Y);
    let settings = MeshRayCastSettings::default()
        .with_filter(accept)
        .with_visibility(RayCastVisibility::Any)
        .always_early_exit();
    let (_entity, hit) = ray_cast
        .cast_ray(ray, &settings)
        .iter()
        .find(|(_entity, hit)| hit.distance <= PROBE_ABOVE + PROBE_BELOW)?;
    // A prim floor's underside faces down; the feet still stand on it, so take the
    // up-facing side of whatever was hit.
    let normal = if hit.normal.y < 0.0 {
        Vec3::new(-hit.normal.x, -hit.normal.y, -hit.normal.z)
    } else {
        hit.normal
    };
    Some(GroundHit {
        point: hit.point,
        normal: normal.normalize_or(Vec3::Y),
    })
}

/// Probe the ground under every rigged avatar's body root and ankles (P31.14).
///
/// Casts a short vertical ray through the body root and through each of the two ankle
/// **targets** the pose pass published last frame (the *pre-IK* ankle positions — see
/// [`AvatarGround::targets`] for why using the posed ones instead sets the knees buzzing),
/// and records the surface for [`crate::locomotion_ik`].
///
/// Cannot live inside the pose pass: [`MeshRayCast`] reads every `GlobalTransform` and
/// the pose pass writes them, which Bevy will not let one system do at once.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the ray \
              caster, the avatars to probe, the globals to probe through, and the three \
              queries the ground filter is built from"
)]
pub(crate) fn probe_avatar_ground(
    mut ray_cast: MeshRayCast,
    spatial: SpatialQuery,
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    faces: Query<(), With<PrimFaceEntity>>,
    terrain: Query<(), With<TerrainSurface>>,
    mut ground: ResMut<AvatarGround>,
) {
    ground.probes.clear();
    let Some(body) = body else {
        return;
    };
    // Fast path: avian's BVH (terrain colliders + physical movers). `None` when the
    // env toggle is off, so the probe casts only the `MeshRayCast` as before.
    let spatial = ground_probe_spatial_enabled().then_some(&spatial);
    let filter = SpatialQueryFilter::default();
    let agents = state.rigged_agents();
    // Every avatar's root, so the filter can reject anything hanging off one — its base
    // body, its worn mesh, its shoes, and any other avatar standing nearby.
    let avatar_roots: Vec<Entity> = agents
        .iter()
        .filter_map(|&agent| state.body_root_of(agent))
        .collect();
    // Ground is object faces and land patches only, and never an avatar's own geometry:
    // a positive filter, so the water plane, particles and the sky can never be walked on.
    let accept = |entity: Entity| -> bool {
        if !faces.contains(entity) && !terrain.contains(entity) {
            return false;
        }
        // Walk up to the scene root; an avatar's attachments hang off its skeleton
        // joints, which hang off its body root, so this catches worn meshes too.
        let mut current = entity;
        loop {
            if avatar_roots.contains(&current) {
                return false;
            }
            match parents.get(current) {
                Ok(parent) => current = parent.parent(),
                Err(_no_parent) => return true,
            }
        }
    };
    // Fall back to the rest-pose ankles on an avatar the pose pass has not published
    // targets for yet (its very first frame).
    let ankles = (
        body.joint_index("mAnkleLeft"),
        body.joint_index("mAnkleRight"),
    );
    let mut probed: Vec<(AgentKey, AgentGround)> = Vec::with_capacity(agents.len());
    for agent in agents {
        // A seated avatar's pose is owned by the sit animation, not the ground, so
        // it needs no probe. Safe because `locomotion_ik` gates the airborne branch
        // on `seated` — otherwise the absent ground would read as airborne.
        if state.is_seated(agent) {
            continue;
        }
        let Some(root) = state.body_root_of(agent) else {
            continue;
        };
        let Ok(root_global) = globals.get(root) else {
            continue;
        };
        let joints = state.joint_entities_of(agent);
        let rest_ankle = |index: Option<usize>| -> Option<Vec3> {
            let entity = joints?.get(index?)?;
            Some(globals.get(*entity).ok()?.translation())
        };
        let (left_point, right_point) = match ground.targets.get(&agent).copied() {
            Some((left, right)) => (Some(left), Some(right)),
            None => (rest_ankle(ankles.0), rest_ankle(ankles.1)),
        };
        let probes = AgentGround {
            root: probe(
                &mut ray_cast,
                spatial,
                &filter,
                root_global.translation(),
                &accept,
            ),
            left: left_point
                .and_then(|point| probe(&mut ray_cast, spatial, &filter, point, &accept)),
            right: right_point
                .and_then(|point| probe(&mut ray_cast, spatial, &filter, point, &accept)),
        };
        probed.push((agent, probes));
    }
    ground.probes.extend(probed);
}
