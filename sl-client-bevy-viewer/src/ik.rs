//! The three-joint inverse-kinematics solver the locomotion adjusters need
//! (P31.14): a port of the reference viewer's `LLJointSolverRP3`
//! (`indra/llcharacter/lljointsolverrp3.cpp`).
//!
//! "RP3" is the reference's own name for it — a closed-form solver in *3D real
//! projective space* rather than an iterative one. Given a two-bone chain
//! `A → B → C` (hip → knee → ankle, or shoulder → elbow → wrist) and a goal
//! position for `C`, it produces new **world** rotations for `A` and `B` that put
//! `C` on the goal, choosing among the infinitely many solutions with:
//!
//! - a **pole vector** (in `A`'s parent's frame) that fixes which way the chain's
//!   plane faces, i.e. which way the knee/elbow points;
//! - an optional **bend axis** (`b_axis`, in `B`'s own frame) which, when given,
//!   replaces the derived chain-plane normal with the joint's actual hinge axis —
//!   what the leg IK uses, so the knee bends like a knee rather than folding
//!   sideways when the chain happens to be straight;
//! - a **twist** about the `A → goal` axis.
//!
//! The solver is *pure*: it takes the chain's current world positions/rotations and
//! returns the corrected world rotations, so it carries no skeleton, no Bevy state
//! and no per-frame memory. [`crate::locomotion_ik`] converts between it and the
//! viewer's [`AnimationPose`](sl_client_bevy::AnimationPose) locals; a later reach /
//! aim adjuster (P31.15) reuses it unchanged.
//!
//! Everything is in **one fixed frame** — for the leg IK, the avatar-local Second
//! Life frame the deformed skeleton's world matrices live in. The reference works in
//! its agent frame for the same reason: the chain and the goal must be commensurable.
//!
//! Frames aside, the one translation from the reference worth stating: Linden's
//! `LLQuaternion a * b` applies `a` **then** `b`, the opposite order of glam's
//! `Quat::mul_quat`, so every composition here is written mirrored (Linden
//! `cgRot * pRot * twistRot` becomes `twist.mul_quat(plane).mul_quat(cg)`).

use bevy::prelude::*;

/// The parallelism / degeneracy tolerance the reference uses throughout `solve()`
/// (its literal `0.001f`): two directions count as parallel when `1 - |â · b̂|` is
/// below it.
const PARALLEL_EPSILON: f32 = 0.001;

/// The squared-length below which the reference treats the `A→B × B→C` cross
/// product as degenerate (its literal `0.001f` on `magVecSquared`), i.e. the chain
/// is straight and its plane is undefined from the bones alone.
const DEGENERATE_CROSS: f32 = 0.001;

/// The static configuration of one two-bone IK chain: how to disambiguate among the
/// solutions that all put `C` on the goal. Set up once (the reference's
/// `setPoleVector` / `setBAxis` / `setTwist`) and reused every frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct JointSolver {
    /// The pole vector in `A`'s **parent's** frame: the chain's solution plane is
    /// rotated so it contains this direction, which is what makes a knee point
    /// forward rather than anywhere on the cone around `A → goal`.
    pole_vector: Vec3,
    /// The hinge axis of `B` in `B`'s **own** frame, if the chain has one. When set,
    /// it is used as the chain-plane normal instead of deriving one from the bones —
    /// the legs use it so the knee keeps bending about its real axis even when the
    /// leg is straight (where the derived normal is undefined).
    b_axis: Option<Vec3>,
    /// A twist about the `A → goal` axis, radians (the reference's `mTwist`).
    twist: f32,
}

/// One frame's state of a two-bone chain, in a single fixed frame (for the leg IK,
/// the avatar-local Second Life frame): where the three joints currently are, where
/// `C` should end up, and the current world rotations the solver corrects.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Chain {
    /// World position of `A` (the hip / shoulder).
    pub(crate) a_pos: Vec3,
    /// World position of `B` (the knee / elbow).
    pub(crate) b_pos: Vec3,
    /// World position of `C` (the ankle / wrist) — the end effector.
    pub(crate) c_pos: Vec3,
    /// World position the end effector should reach.
    pub(crate) goal: Vec3,
    /// `A`'s current world rotation.
    pub(crate) a_rot: Quat,
    /// `B`'s current world rotation.
    pub(crate) b_rot: Quat,
    /// The world rotation of `A`'s **parent** (the pelvis / chest), which the pole
    /// vector is expressed in.
    pub(crate) a_parent_rot: Quat,
}

/// The solved world rotations for `A` and `B`. `C` is unchanged — the reference
/// leaves the end effector's own orientation to the caller (the stand motion
/// conforms the ankle to the ground normal afterwards).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Solved {
    /// `A`'s corrected world rotation.
    pub(crate) a_rot: Quat,
    /// `B`'s corrected world rotation.
    pub(crate) b_rot: Quat,
}

/// Component-wise vector subtraction (`a - b`), avoiding the glam `-` operator the
/// workspace `arithmetic_side_effects` lint trips on.
fn vsub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// Component-wise vector addition (`a + b`), avoiding the glam `+` operator.
fn vadd(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// Whether `a` and `b` point along the same line (either direction), to the
/// reference's [`PARALLEL_EPSILON`] — its `are_parallel()`. A zero-length vector is
/// reported parallel to everything, which is how the reference's `normVec()` of a
/// zero vector behaves in the checks that guard the singular cases.
fn are_parallel(a: Vec3, b: Vec3) -> bool {
    let (an, bn) = (a.normalize_or_zero(), b.normalize_or_zero());
    (1.0 - an.dot(bn).abs()) < PARALLEL_EPSILON
}

/// The unsigned angle between `a` and `b`, radians — the reference's
/// `angle_between()`, `atan2(|a × b|, a · b)`, which is stable for near-parallel and
/// near-antiparallel inputs where an `acos` of the normalised dot is not.
fn angle_between(a: Vec3, b: Vec3) -> f32 {
    a.cross(b).length().atan2(a.dot(b))
}

/// The rotation taking `from` onto `to` along the shortest arc (Linden's
/// `LLQuaternion::shortestArc`). Degenerate inputs yield the identity.
fn shortest_arc(from: Vec3, to: Vec3) -> Quat {
    let (from, to) = (from.normalize_or_zero(), to.normalize_or_zero());
    if from.length_squared() < f32::EPSILON || to.length_squared() < f32::EPSILON {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(from, to)
}

impl JointSolver {
    /// A solver with the given pole vector (in `A`'s parent's frame) and no bend axis
    /// or twist — the reference's constructor plus `setPoleVector`.
    pub(crate) fn new(pole_vector: Vec3) -> Self {
        Self {
            pole_vector: pole_vector.normalize_or_zero(),
            b_axis: None,
            twist: 0.0,
        }
    }

    /// Give `B` an explicit hinge axis in its own frame (the reference's `setBAxis`),
    /// so the chain plane follows the joint's real bend axis rather than one derived
    /// from the (possibly straight) bones.
    #[must_use]
    pub(crate) fn with_b_axis(mut self, b_axis: Vec3) -> Self {
        self.b_axis = Some(b_axis.normalize_or_zero());
        self
    }

    /// Twist the solution about the `A → goal` axis by `twist` radians (the
    /// reference's `setTwist`).
    #[must_use]
    #[expect(
        dead_code,
        reason = "the reference's mTwist knob, unused by the leg IK (the stands leave \
                  it at zero) but part of the solver P31.15's reach/aim adjusters reuse"
    )]
    pub(crate) const fn with_twist(mut self, twist: f32) -> Self {
        self.twist = twist;
        self
    }

    /// Solve the chain: the world rotations for `A` and `B` that place `C` on the
    /// goal, honouring the pole vector / bend axis / twist.
    ///
    /// A faithful port of `LLJointSolverRP3::solve()`, including its early returns:
    /// when the problem is genuinely singular (the chain, the pole vector and the
    /// goal all collinear, or the goal direction parallel to the pole vector) the
    /// reference bails out and leaves the joints where they are, and so does this —
    /// the caller keeps the animation's own pose for that frame.
    pub(crate) fn solve(&self, chain: &Chain) -> Solved {
        let unchanged = Solved {
            a_rot: chain.a_rot,
            b_rot: chain.b_rot,
        };

        // The pole vector carried from A's parent's frame into the working frame.
        let pole = chain.a_parent_rot.mul_vec3(self.pole_vector);

        // The reference also computes an `A→C` vector here and, from it, an
        // `abacCompOrthoVec`; both are dead there (`abacCompOrthoVec` is assigned twice
        // and never read, and `acVec` is overwritten before its first use), so neither
        // is ported.
        let mut ab = vsub(chain.b_pos, chain.a_pos);
        let mut bc = vsub(chain.c_pos, chain.b_pos);
        let ag = vsub(chain.goal, chain.a_pos);

        let ab_len = ab.length();
        let bc_len = bc.length();
        let ag_len = ag.length();

        // The normal of the chain's current plane: the joint's real hinge axis when
        // one was given, else derived from the bones — with the reference's fallbacks
        // for a straight chain, whose plane the bones alone do not determine.
        let mut abc_norm = match self.b_axis {
            Some(axis) => chain.b_rot.mul_vec3(axis),
            None => {
                if are_parallel(ab, bc) {
                    if are_parallel(pole, ab) {
                        if are_parallel(pole, ag) {
                            // Chain, pole and goal are collinear: no solution plane.
                            return unchanged;
                        }
                        pole.cross(ag)
                    } else {
                        pole.cross(ab)
                    }
                } else {
                    ab.cross(bc)
                }
            }
        };

        // --- B's bend: open or close the chain until |A→C| can reach |A→goal| ---
        let ab_bc_angle = angle_between(ab, bc);
        let mut bend_axis = ab.cross(bc);
        if bend_axis.length_squared() < DEGENERATE_CROSS {
            // A straight chain has no bend plane of its own; use the pole's.
            bend_axis = pole.cross(ab);
        }
        let bend_axis = bend_axis.normalize_or_zero();

        // The interior angle at B that makes the chain span exactly |A→goal|
        // (the law of cosines), clamped so an unreachable goal simply straightens or
        // folds the chain as far as it goes.
        let denominator = 2.0 * ab_len * bc_len;
        if denominator.abs() < f32::EPSILON {
            return unchanged;
        }
        let cos_theta = ((ag_len * ag_len) - (ab_len * ab_len) - (bc_len * bc_len)) / denominator;
        let theta = cos_theta.clamp(-1.0, 1.0).acos();
        let b_bend = Quat::from_axis_angle(bend_axis, theta - ab_bc_angle);

        // --- A's swing: rotate the re-bent chain's tip onto the goal ---
        bc = b_bend.mul_vec3(bc);
        let cg_rot = shortest_arc(vadd(ab, bc), ag);
        ab = cg_rot.mul_vec3(ab);
        bc = cg_rot.mul_vec3(bc);
        abc_norm = cg_rot.mul_vec3(abc_norm);

        // --- A's roll: turn the chain's plane to contain the pole vector ---
        if are_parallel(ag, pole) {
            // The solution plane is undefined. The reference bails out here *before*
            // applying anything, discarding the bend and swing too, so the joints keep
            // the pose the animation gave them for this frame; do the same.
            return unchanged;
        }
        let apg_norm = pole.cross(ag).normalize_or_zero();
        if self.b_axis.is_none() && !are_parallel(ab, bc) {
            // Re-derive the plane from the *solved* bones (skipped when the chain
            // came out straight — the pre-solve normal is then the best available).
            abc_norm = ab.cross(bc);
        }
        let abc_norm = abc_norm.normalize_or_zero();
        let plane_rot = if are_parallel(abc_norm, apg_norm) {
            if abc_norm.dot(apg_norm) < 0.0 {
                // Exactly π out: flip the chain's plane about the goal axis.
                Quat::from_axis_angle(ag.normalize_or_zero(), core::f32::consts::PI)
            } else {
                Quat::IDENTITY
            }
        } else {
            shortest_arc(abc_norm, apg_norm)
        };

        let twist_rot = Quat::from_axis_angle(ag.normalize_or_zero(), self.twist);

        // Linden `cgRot * pRot * twistRot` applies cg, then plane, then twist.
        let a_delta = twist_rot.mul_quat(plane_rot).mul_quat(cg_rot);
        // `B`'s correction is its own bend **plus** `A`'s, because `B` is `A`'s child:
        // the reference sets `B`'s world rotation first and `A`'s second, and setting a
        // parent's world rotation drags its children's world rotations with it. Missing
        // this leaves the knee bent correctly but hanging off an un-swung hip, so the
        // ankle lands nowhere near the goal.
        Solved {
            a_rot: a_delta.mul_quat(chain.a_rot),
            b_rot: a_delta.mul_quat(b_bend).mul_quat(chain.b_rot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Chain, JointSolver, angle_between, are_parallel, vadd, vsub};
    use bevy::prelude::*;

    /// Absolute-difference float check (the workspace forbids bare `==` on floats).
    fn near(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} not within {eps} of {b}");
    }

    /// A straight leg-like chain along `-Z`: hip at the origin, knee 0.5 m down,
    /// ankle 1.0 m down, with every joint at identity rotation.
    fn straight_leg(goal: Vec3) -> Chain {
        Chain {
            a_pos: Vec3::ZERO,
            b_pos: Vec3::new(0.0, 0.0, -0.5),
            c_pos: Vec3::new(0.0, 0.0, -1.0),
            goal,
            a_rot: Quat::IDENTITY,
            b_rot: Quat::IDENTITY,
            a_parent_rot: Quat::IDENTITY,
        }
    }

    /// Re-derive where the end effector lands after a solve, by re-running the
    /// chain's forward kinematics with the solved world rotations. The bone offsets
    /// are the chain's own current ones expressed in each joint's local frame, so
    /// this is the same chain, just rotated.
    fn solved_effector(chain: &Chain, solved: &super::Solved) -> Vec3 {
        let ab_local = chain
            .a_rot
            .inverse()
            .mul_vec3(vsub(chain.b_pos, chain.a_pos));
        let bc_local = chain
            .b_rot
            .inverse()
            .mul_vec3(vsub(chain.c_pos, chain.b_pos));
        let b_pos = vadd(chain.a_pos, solved.a_rot.mul_vec3(ab_local));
        // B's world rotation already carries A's correction (the solver returns world
        // rotations), so the second bone only needs B's.
        vadd(b_pos, solved.b_rot.mul_vec3(bc_local))
    }

    /// The end effector lands on a reachable goal: the classic foot-planting case,
    /// where the ankle must come up 0.2 m onto a rise.
    #[test]
    fn reachable_goal_is_met() {
        let solver = JointSolver::new(Vec3::X).with_b_axis(Vec3::Y);
        let goal = Vec3::new(0.1, 0.0, -0.8);
        let chain = straight_leg(goal);
        let solved = solver.solve(&chain);
        let landed = solved_effector(&chain, &solved);
        near(landed.distance(goal), 0.0, 1e-4);
    }

    /// A goal *beyond* the chain's reach straightens it toward the goal rather than
    /// tearing the bones apart: the reference clamps `cosTheta` to `[-1, 1]`, so the
    /// chain extends fully and points at the goal. Bone lengths are preserved.
    #[test]
    fn unreachable_goal_extends_without_stretching() {
        let solver = JointSolver::new(Vec3::X).with_b_axis(Vec3::Y);
        let goal = Vec3::new(0.0, 0.0, -5.0);
        let chain = straight_leg(goal);
        let solved = solver.solve(&chain);
        let landed = solved_effector(&chain, &solved);
        // Fully extended: exactly the chain's own length from the hip, along the goal.
        near(landed.length(), 1.0, 1e-4);
        near(landed.normalize().dot(goal.normalize()), 1.0, 1e-4);
    }

    /// The bend axis decides which way the knee points: the same goal solved with
    /// opposite hinge axes bends the knee to opposite sides.
    #[test]
    fn bend_axis_picks_the_knee_side() {
        let goal = Vec3::new(0.0, 0.0, -0.7);
        let chain = straight_leg(goal);
        let forward = JointSolver::new(Vec3::X).with_b_axis(Vec3::Y).solve(&chain);
        let backward = JointSolver::new(Vec3::X)
            .with_b_axis(Vec3::new(0.0, -1.0, 0.0))
            .solve(&chain);
        // Both reach the goal…
        near(solved_effector(&chain, &forward).distance(goal), 0.0, 1e-4);
        near(solved_effector(&chain, &backward).distance(goal), 0.0, 1e-4);
        // …but the knees end up on opposite sides of the hip→ankle line (the knee's
        // X offset flips sign).
        let knee = |solved: &super::Solved| {
            let ab_local = chain
                .a_rot
                .inverse()
                .mul_vec3(vsub(chain.b_pos, chain.a_pos));
            vadd(chain.a_pos, solved.a_rot.mul_vec3(ab_local))
        };
        let (knee_forward, knee_backward) = (knee(&forward), knee(&backward));
        assert!(
            knee_forward.x * knee_backward.x < 0.0,
            "knees should bend to opposite sides, got {knee_forward:?} and {knee_backward:?}",
        );
    }

    /// A goal the chain already meets leaves it alone (up to float noise): the
    /// no-op case that runs every frame an avatar stands on flat ground.
    #[test]
    fn goal_at_the_effector_is_a_no_op() {
        let solver = JointSolver::new(Vec3::X).with_b_axis(Vec3::Y);
        let chain = straight_leg(Vec3::new(0.0, 0.0, -1.0));
        let solved = solver.solve(&chain);
        near(solved.a_rot.dot(Quat::IDENTITY).abs(), 1.0, 1e-4);
        near(solved.b_rot.dot(Quat::IDENTITY).abs(), 1.0, 1e-4);
    }

    /// The reference's degeneracy helpers behave as its `llmath` ones do.
    #[test]
    fn parallel_and_angle_helpers() {
        assert!(are_parallel(Vec3::X, Vec3::X));
        // Antiparallel counts as parallel (the reference takes |dot|).
        assert!(are_parallel(Vec3::X, Vec3::new(-2.0, 0.0, 0.0)));
        assert!(!are_parallel(Vec3::X, Vec3::Y));
        near(
            angle_between(Vec3::X, Vec3::Y),
            core::f32::consts::FRAC_PI_2,
            1e-6,
        );
        near(angle_between(Vec3::X, Vec3::new(3.0, 0.0, 0.0)), 0.0, 1e-6);
        near(
            angle_between(Vec3::X, Vec3::new(-1.0, 0.0, 0.0)),
            core::f32::consts::PI,
            1e-6,
        );
    }
}
