//! The avatar **overrides**: the debug / A/B knobs that change how avatars are
//! posed and drawn — joint overrides off, a frozen T-pose, the client
//! locomotion fallback forced on, body physics forced on, a forced hand pose,
//! and the bake-on-mesh diagnostic skins — as one resource per app.
//!
//! They were environment reads made by the systems that consumed them, which
//! two viewers in one process could never disagree about, and which a test
//! could not state without touching the process environment. Like the render
//! overrides, the viewer reads them once ([`AvatarOverrides::from_env`]) when
//! it builds its App, and a test states its own.
//!
//! Every variable keeps its name and its meaning. The `SL_VIEWER_LOG_*`
//! diagnostics are not here: they change only what is logged, to the process's
//! one subscriber.

use bevy::prelude::*;

use sl_anim::HandPose;

/// The bake-on-mesh diagnostic skin every BoM face wears instead of its bake
/// (R22): to tell a texture / UV-seam artifact from a geometry one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSkin {
    /// A generated UV grid through the bake's own UV transform
    /// (`SL_VIEWER_DEBUG_AVATAR_GRID=1`): a broken grid is a UV-mapping
    /// problem, a continuous one means the seams are baked skin content.
    UvGrid,
    /// A flat neutral skin (`SL_VIEWER_DEBUG_AVATAR_FLAT=1`): a seam that
    /// vanishes was texture, one that stays is geometry or normals.
    Flat,
}

/// The overrides of how avatars are **posed**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoseOverrides {
    /// Leave worn rigged meshes' joint position overrides (R1) off the
    /// skeleton (`SL_VIEWER_JOINT_OVERRIDES=0`), to compare the pre-override
    /// skeleton in one session.
    pub joint_overrides_disabled: bool,
    /// Freeze every avatar at its shaped **rest** skeleton — which in Second
    /// Life *is* the T-pose — with no keyframe animation, no procedural idle,
    /// and none of the look-at / locomotion / reach / body-physics adjusters
    /// folded in (`SL_VIEWER_TPOSE=1`). An avatar's AO walks, turns and fidgets
    /// it, so two runs never frame the same body the same way; frozen, an A/B
    /// of anything that shapes the body (a shape slider, a collision-volume
    /// displacement, a joint override) is comparable. The GPU-avatar scheduler
    /// mirrors the freeze (no playback staged, idle disabled in pass B).
    pub t_pose: bool,
    /// Force every avatar's requested hand pose
    /// (`SL_VIEWER_HAND_POSE_TEST=<index>`), overriding the playing animation.
    pub hand_pose: Option<HandPose>,
}

/// The overrides of how avatars **move**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionOverrides {
    /// Keep the client-side locomotion fallback driving even while the
    /// simulator animates the avatar (`SL_VIEWER_FORCE_CLIENT_LOCOMOTION=1`),
    /// so it can be exercised on a root presence.
    pub client_locomotion_forced: bool,
    /// The reference's `physics_test` switch (`SL_VIEWER_PHYSICS_TEST=1`):
    /// force every body-physics motion's `Max_Effect` on, so an avatar wearing
    /// no tuned physics wearable still bounces.
    pub body_physics_forced: bool,
}

/// Every avatar override, per app. `Default` overrides nothing.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AvatarOverrides {
    /// How avatars are posed.
    pub pose: PoseOverrides,
    /// How avatars move.
    pub motion: MotionOverrides,
    /// Draw every bake-on-mesh face in a diagnostic skin instead of its bake.
    pub diagnostic_skin: Option<DiagnosticSkin>,
}

impl AvatarOverrides {
    /// Read every knob from the environment — once, by the viewer, while its
    /// App is built.
    #[must_use]
    pub fn from_env() -> Self {
        let is_one = |key: &str| std::env::var(key).as_deref() == Ok("1");
        Self {
            pose: PoseOverrides {
                joint_overrides_disabled: std::env::var("SL_VIEWER_JOINT_OVERRIDES").as_deref()
                    == Ok("0"),
                t_pose: is_one("SL_VIEWER_TPOSE"),
                hand_pose: std::env::var("SL_VIEWER_HAND_POSE_TEST")
                    .ok()
                    .and_then(|raw| raw.trim().parse().ok())
                    .and_then(HandPose::from_index),
            },
            motion: MotionOverrides {
                client_locomotion_forced: is_one("SL_VIEWER_FORCE_CLIENT_LOCOMOTION"),
                body_physics_forced: is_one("SL_VIEWER_PHYSICS_TEST"),
            },
            // The grid takes precedence over the flat skin.
            diagnostic_skin: if is_one("SL_VIEWER_DEBUG_AVATAR_GRID") {
                Some(DiagnosticSkin::UvGrid)
            } else if is_one("SL_VIEWER_DEBUG_AVATAR_FLAT") {
                Some(DiagnosticSkin::Flat)
            } else {
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AvatarOverrides, PoseOverrides};

    /// Two apps in one process can disagree about a knob.
    #[test]
    fn two_apps_in_one_process_can_disagree() {
        use bevy::prelude::*;

        let mut posed = App::new();
        posed.init_resource::<AvatarOverrides>();
        let mut frozen = App::new();
        frozen.insert_resource(AvatarOverrides {
            pose: PoseOverrides {
                t_pose: true,
                ..PoseOverrides::default()
            },
            ..AvatarOverrides::default()
        });
        assert!(!posed.world().resource::<AvatarOverrides>().pose.t_pose);
        assert!(frozen.world().resource::<AvatarOverrides>().pose.t_pose);
    }
}
