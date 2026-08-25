//! The avatar **ground probe** (P31.14): what is actually under an avatar's feet.
//!
//! The reference viewer's `LLVOAvatar::getGround` resolves the surface under a point
//! via `LLWorld::resolveStepHeightGlobal`, which uses exactly two inputs: the region's
//! **land height** and the avatar's **collision (foot) plane** — the `mFootPlane` the
//! simulator sends on every avatar `ObjectUpdate`. It takes whichever surface is higher
//! (the one a short downward ray meets first), and *does not* raycast arbitrary object
//! geometry: the sim has already done the avatar-vs-world collision and reports the
//! resulting surface as the plane. So a prim ramp, staircase or skybox platform the
//! avatar stands on comes through the foot plane, not a client-side ray.
//!
//! This probe does the same. For each sample point — the avatar's body root and each
//! pre-IK ankle — it resolves the ground as the higher of the terrain land height and
//! the [`AvatarMotion`]-carried collision plane at that horizontal position, with the
//! plane's normal when it wins. That is faithful to the reference *and* far cheaper than
//! the whole-scene [`bevy::picking::mesh_picking::ray_cast::MeshRayCast`] this used to
//! run three times per avatar per frame. The foot IK and landing recovery
//! ([`crate::locomotion_ik`]) consume the result unchanged.
//!
//! It runs as its own system, after the pose pass, reading the ankle joints' globals as
//! the pose pass left them **last** frame (a frame of staleness invisible at any frame
//! rate, and the same order of lag the reference's once-per-frame probe carries).

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, AnimationPose, RegionHandle, VolumeDeformations};

use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::AvatarBody;
use crate::coords::region_offset_bevy;
use crate::terrain::TerrainState;
use crate::world_api::AvatarMotion;
use crate::world_api::AvatarState;

/// How far **above** a sample point a resolved surface may sit and still count as the
/// ground under it, metres (the reference's `getGround` probes from `+1` on Z).
const PROBE_ABOVE: f32 = 1.0;

/// How far **below** a sample point a resolved surface may sit and still count as the
/// ground under it, metres (the reference's `-1`). Deliberately short: the ground is
/// what the avatar is standing *on*, so a surface further than this reads as **airborne**
/// (nothing under the foot), exactly as the old `±1 m` raycast did.
const PROBE_BELOW: f32 = 1.0;

/// A collision plane whose up-axis component is below this is treated as absent: a
/// near-vertical foot plane cannot be a floor to stand on, and dividing by its tiny
/// `nz` would explode. The simulator's foot plane for a supported avatar is up-facing.
const PLANE_NORMAL_EPSILON: f32 = 1.0e-3;

/// One ground sample, in **Bevy world** space: where the surface is, and which way it
/// faces. The caller converts into whatever frame it needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GroundHit {
    /// The point on the surface (Bevy world metres).
    pub(crate) point: Vec3,
    /// The surface's upward unit normal (Bevy world). Always points up-ish: a
    /// back-facing plane is flipped, so a two-sided prim floor still reads as ground.
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
#[derive(Debug, Resource, Default)]
pub struct AvatarGround {
    /// The samples of each rigged avatar seen this frame.
    probes: HashMap<AgentKey, AgentGround>,
    /// Where to cast each avatar's two foot samples: its ankles' Bevy-world positions in
    /// the **pre-IK** pose (keyframe + idle + look-at), published by the pose pass.
    ///
    /// This *must not* be the ankles' posed positions, which is what reading the joint
    /// entities' `GlobalTransform`s would give — that closes a feedback loop with a
    /// vicious limit cycle in it. A standing leg is at ~99.5% of full extension, where
    /// the IK's gain is enormous (a 2 cm ankle move is ~50° of knee); when a foot's goal
    /// falls out of the leg's reach the solve straightens the leg and the ankle lands
    /// *short* of the goal, somewhere else horizontally; the next probe therefore samples
    /// the ground somewhere else, the goal comes back into reach, the ankle snaps back —
    /// and the knees buzz. Sampling the ground under the **pre-IK** ankle keeps the probe
    /// a function of the animation alone, which is smooth, so nothing the IK does can
    /// perturb its own input.
    targets: HashMap<AgentKey, (Vec3, Vec3)>,
    /// The last collision (foot) plane seen for each avatar (region-local
    /// `[nx, ny, nz, w]`), cached so a momentarily plane-less update (e.g. a compressed
    /// one) holds the last good plane rather than dropping to a terrain-only guess —
    /// exactly as the reference viewer holds `mFootPlane`. Pruned to the live rigged
    /// avatars each frame.
    planes: HashMap<AgentKey, [f32; 4]>,
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

/// `SL_VIEWER_LOG_GROUND=1` logs, per avatar and only when it *changes*, whether the
/// simulator is sending a collision (foot) plane and the resolved root-ground height —
/// enough to confirm at a glance that the plane path is live (and, on a grid that sends
/// no plane, that it fell back to land-only). Silent by default; read once.
fn log_ground_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("SL_VIEWER_LOG_GROUND").is_ok_and(|value| value == "1" || value == "true")
    })
}

/// Resolve the ground under one **Bevy-world** `sample`, given the avatar's collision
/// `plane` (region-local `[nx, ny, nz, w]`, or `None`), the region's `land_sl_z` at that
/// horizontal position (or `None` when terrain is not ingested), and the region's Bevy
/// `offset` from the scene origin.
///
/// Takes the higher of the plane and the land, exactly as the reference's
/// `resolveStepHeightGlobal` prefers the foot plane over the land only when it sits
/// above it. Returns [`None`] when neither is known, or when the resolved surface is out
/// of the probe's short vertical reach (the avatar is airborne over it). Pure, so it is
/// unit-testable without a live terrain or region.
fn resolve_ground(
    sample: Vec3,
    plane: Option<[f32; 4]>,
    land_sl_z: Option<f32>,
    offset: Vec3,
) -> Option<GroundHit> {
    // The sample's Second Life region-local horizontal (the inverse of `sl_to_bevy_vec`
    // with the region offset removed: `bevy (x, y, z) -> sl (x, -z, y)`).
    let sl_x = sample.x - offset.x;
    let sl_y = -(sample.z - offset.z);
    // The plane's Second Life Z at this horizontal position and its up-facing Bevy
    // normal, or `None` when the plane is absent or near-vertical.
    let plane_ground = plane.and_then(|[nx, ny, nz, w]| {
        if nz.abs() < PLANE_NORMAL_EPSILON {
            return None;
        }
        let sl_z = (w - nx * sl_x - ny * sl_y) / nz;
        // Second Life normal -> Bevy direction (`sl_to_bevy_vec` with no offset); flip a
        // down-facing plane so a two-sided surface still reads as ground.
        let bevy_normal = Vec3::new(nx, nz, -ny);
        let bevy_normal = if bevy_normal.y < 0.0 {
            Vec3::new(-bevy_normal.x, -bevy_normal.y, -bevy_normal.z)
        } else {
            bevy_normal
        };
        Some((sl_z, bevy_normal.normalize_or(Vec3::Y)))
    });
    // Pick the higher surface. The land uses an up normal — when the avatar actually
    // stands on sloped terrain the sim reports that slope *as the foot plane*, so the
    // plane branch carries the real normal; the bare-land branch is the airborne /
    // no-plane fallback where the normal does not drive any foot roll.
    let (ground_sl_z, normal) = match (plane_ground, land_sl_z) {
        (Some((plane_z, plane_normal)), Some(land_z)) => {
            if plane_z >= land_z {
                (plane_z, plane_normal)
            } else {
                (land_z, Vec3::Y)
            }
        }
        (Some((plane_z, plane_normal)), None) => (plane_z, plane_normal),
        (None, Some(land_z)) => (land_z, Vec3::Y),
        (None, None) => return None,
    };
    let ground_y = ground_sl_z + offset.y;
    // Airborne guard: a surface outside the probe's short vertical reach is not the
    // ground under this foot (the old raycast returned nothing beyond `±1 m`).
    if ground_y > sample.y + PROBE_ABOVE || ground_y < sample.y - PROBE_BELOW {
        return None;
    }
    Some(GroundHit {
        point: Vec3::new(sample.x, ground_y, sample.z),
        normal,
    })
}

/// Resolve the ground under one Bevy-world `sample` for an avatar in `region`: look up
/// the land height there and combine it with the avatar's collision `plane` via
/// [`resolve_ground`].
fn ground_under(
    sample: Vec3,
    plane: Option<[f32; 4]>,
    region: RegionHandle,
    origin: Option<RegionHandle>,
    terrain: &TerrainState,
) -> Option<GroundHit> {
    let offset = region_offset_bevy(region, origin);
    let sl_x = sample.x - offset.x;
    let sl_y = -(sample.z - offset.z);
    let land_sl_z = terrain.land_height(region, sl_x, sl_y);
    resolve_ground(sample, plane, land_sl_z, offset)
}

/// Probe the ground under every rigged avatar's body root and ankles (P31.14).
///
/// Resolves the surface under the body root and under each of the two ankle **targets**
/// the pose pass published last frame (the *pre-IK* ankle positions — see
/// `AvatarGround::targets` for why using the posed ones instead sets the knees buzzing)
/// from the terrain land height and the simulator's collision plane, and records it for
/// [`crate::locomotion_ik`].
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries"
)]
pub fn probe_avatar_ground(
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    library: Option<Res<AvatarAssetLibrary>>,
    globals: Query<&GlobalTransform>,
    motions: Query<&AvatarMotion>,
    terrain: Res<TerrainState>,
    mut ground: ResMut<AvatarGround>,
    // Edge-triggered `SL_VIEWER_LOG_GROUND` state: the last plane-present flag logged
    // per avatar, so the trace fires only on a change, not every frame.
    mut logged_plane: Local<HashMap<AgentKey, bool>>,
) {
    ground.probes.clear();
    let Some(body) = body else {
        return;
    };
    let origin = terrain.origin();
    let agents = state.rigged_agents();
    // Drop cached planes for avatars that are no longer rigged (despawned, or reverted to
    // a placeholder sphere), so the cache tracks the live set.
    ground.planes.retain(|agent, _plane| agents.contains(agent));
    // Fall back to the rest-pose ankles on an avatar the pose pass has not published
    // targets for yet (its very first frame).
    let ankles = (
        body.joint_index("mAnkleLeft"),
        body.joint_index("mAnkleRight"),
    );
    let mut probed: Vec<(AgentKey, AgentGround)> = Vec::with_capacity(agents.len());
    for agent in agents {
        // A seated avatar's pose is owned by the sit animation, not the ground, so it
        // needs no probe. Safe because `locomotion_ik` gates the airborne branch on
        // `seated` — otherwise the absent ground would read as airborne.
        if state.is_seated(agent) {
            continue;
        }
        let Some(root) = state.body_root_of(agent) else {
            continue;
        };
        let Ok(root_global) = globals.get(root) else {
            continue;
        };
        let Ok(motion) = motions.get(root) else {
            continue;
        };
        // Refresh the cached plane whenever this update carried one; otherwise reuse the
        // last good plane (see [`AvatarGround::planes`]).
        if let Some(plane) = motion.collision_plane() {
            let _prev = ground.planes.insert(agent, plane);
        }
        let plane = ground.planes.get(&agent).copied();
        let region = motion.region();
        let (left_point, right_point) = match ground.targets.get(&agent).copied() {
            Some((left, right)) => (Some(left), Some(right)),
            None => {
                // Before the pose driver has published probe targets (an avatar's
                // first frame), fall back to a one-shot rest solve of the shaped
                // skeleton (Phase 4 removed the ankle joint entities) and read the
                // ankle worlds, composing each avatar-frame position with the
                // avatar-root global.
                let rest = state.deformations(agent).and_then(|deform| {
                    let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
                    let skeleton = library.as_deref()?.skeleton();
                    Some(skeleton.deformed_world_matrices(
                        deform,
                        &VolumeDeformations::default(),
                        &overrides,
                        &AnimationPose::default(),
                    ))
                });
                let ankle = |index: Option<usize>| -> Option<Vec3> {
                    let local = rest.as_ref()?.get(index?)?.w_axis.truncate();
                    Some(root_global.transform_point(local))
                };
                (ankle(ankles.0), ankle(ankles.1))
            }
        };
        let probes = AgentGround {
            root: ground_under(root_global.translation(), plane, region, origin, &terrain),
            left: left_point.and_then(|point| ground_under(point, plane, region, origin, &terrain)),
            right: right_point
                .and_then(|point| ground_under(point, plane, region, origin, &terrain)),
        };
        if log_ground_enabled() {
            let present = plane.is_some();
            if logged_plane.insert(agent, present) != Some(present) {
                info!(
                    "ground {agent:?}: collision plane {}, root ground {:?}",
                    if present {
                        "present"
                    } else {
                        "ABSENT (land-only fallback)"
                    },
                    probes.root.map(|hit| hit.point.y),
                );
            }
        }
        probed.push((agent, probes));
    }
    ground.probes.extend(probed);
}

#[cfg(test)]
mod tests {
    use super::{PROBE_ABOVE, PROBE_BELOW, resolve_ground};
    use bevy::prelude::Vec3;

    /// A flat foot plane (up normal, `w` the height) grounds a sample right at the plane
    /// height, with an up normal — the common standing case.
    #[test]
    fn flat_plane_grounds_at_its_height() {
        let sample = Vec3::new(5.0, 20.4, 3.0);
        let hit = resolve_ground(sample, Some([0.0, 0.0, 1.0, 20.0]), None, Vec3::ZERO);
        assert!(
            hit.is_some_and(|hit| (hit.point.y - 20.0).abs() <= 1.0e-4
                && (hit.point.x - 5.0).abs() <= 1.0e-4
                && (hit.point.z - 3.0).abs() <= 1.0e-4
                && hit.normal.abs_diff_eq(Vec3::Y, 1.0e-4)),
            "flat plane should ground at 20 with the horizontal preserved and an up normal, got {hit:?}"
        );
    }

    /// A prim floor plane above the terrain wins over the land — the avatar is held at
    /// the prim, not sunk to the terrain below it.
    #[test]
    fn plane_above_land_wins() {
        let sample = Vec3::new(0.0, 25.3, 0.0);
        // Plane at Z = 25, land at 20.
        let hit = resolve_ground(sample, Some([0.0, 0.0, 1.0, 25.0]), Some(20.0), Vec3::ZERO);
        assert!(
            hit.is_some_and(|hit| (hit.point.y - 25.0).abs() <= 1.0e-4),
            "should stand on the prim (25), got {hit:?}"
        );
    }

    /// With no plane the terrain land height is the ground (the airborne / no-plane
    /// fallback), with an up normal.
    #[test]
    fn land_only_grounds_on_terrain() {
        let sample = Vec3::new(0.0, 20.5, 0.0);
        let hit = resolve_ground(sample, None, Some(20.0), Vec3::ZERO);
        assert!(
            hit.is_some_and(|hit| (hit.point.y - 20.0).abs() <= 1.0e-4
                && hit.normal.abs_diff_eq(Vec3::Y, 1.0e-4)),
            "land should ground at 20 with an up normal, got {hit:?}"
        );
    }

    /// A surface far below the sample is not the ground under the foot: the avatar is
    /// airborne over it, so the probe reports nothing (as the old `±1 m` raycast did).
    #[test]
    fn far_ground_reads_airborne() {
        // Land 20, sample 5 m above it — outside the probe's reach.
        let sample = Vec3::new(0.0, 25.0, 0.0);
        assert!(
            resolve_ground(sample, None, Some(20.0), Vec3::ZERO).is_none(),
            "a foot {PROBE_BELOW} m+ above the ground must read airborne"
        );
        // And a surface above the sample beyond the up-reach is likewise rejected.
        let below = Vec3::new(0.0, 20.0 - PROBE_ABOVE - 0.5, 0.0);
        assert!(resolve_ground(below, None, Some(20.0), Vec3::ZERO).is_none());
    }

    /// Neither a plane nor a land height (terrain mid-rebuild, no plane) → no ground.
    #[test]
    fn no_plane_no_land_is_none() {
        assert!(resolve_ground(Vec3::new(0.0, 20.0, 0.0), None, None, Vec3::ZERO).is_none());
    }

    /// A sloped foot plane grounds each horizontal position at the right height and
    /// carries the tilted (Bevy-world) normal, so the foot IK can roll the ankle onto
    /// the slope. Plane tilts along Second Life +Y: `n = (0, -sin, cos)`, `w = cos*z0`.
    #[test]
    fn sloped_plane_projects_and_tilts() {
        let angle: f32 = 20.0_f32.to_radians();
        let (s, c) = (angle.sin(), angle.cos());
        // Plane through Second Life (x, 0, 20) tilting up toward +Y: n·p = w with
        // n = (0, -s, c), w = c*20.
        let plane = [0.0, -s, c, c * 20.0];
        // Sample over Second Life y = 2 (Bevy z = -2). Plane Z there: (w + s*2)/c.
        let sample = Vec3::new(0.0, 21.0, -2.0);
        let hit = resolve_ground(sample, Some(plane), None, Vec3::ZERO);
        let expected_z = (c * 20.0 + s * 2.0) / c;
        // Bevy normal = sl (0, -s, c) -> (0, c, s): tilted, up-ish.
        assert!(
            hit.is_some_and(|hit| (hit.point.y - expected_z).abs() <= 1.0e-3
                && hit.normal.y > 0.0
                && hit.normal.z.abs() > 0.1),
            "sloped plane should ground at {expected_z} with a tilted normal, got {hit:?}"
        );
    }

    /// The region offset shifts the resolved ground into Bevy world (a neighbour region's
    /// avatar is probed against terrain placed at that offset).
    #[test]
    fn region_offset_shifts_the_ground() {
        let offset = Vec3::new(256.0, 0.0, -256.0);
        let sample = Vec3::new(256.0, 20.5, -256.0);
        let hit = resolve_ground(sample, None, Some(20.0), offset);
        // Land SL Z 20 + offset.y 0 = 20, at the sample's horizontal.
        assert!(
            hit.is_some_and(|hit| (hit.point.y - 20.0).abs() <= 1.0e-4
                && (hit.point.x - 256.0).abs() <= 1.0e-4
                && (hit.point.z + 256.0).abs() <= 1.0e-4),
            "offset land should ground at 20 at the sample horizontal, got {hit:?}"
        );
    }
}
