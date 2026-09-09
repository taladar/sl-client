//! The facing the simulator places the agent at on arrival, applied at once.
//!
//! A teleport re-places the avatar: the destination simulator turns it to the
//! look-at the teleport asked for (OpenSim's `ScenePresence.RotateToLookAt`) and
//! states that facing in the `AgentMovementComplete` it confirms the arrival with
//! — before it has streamed a single `ObjectUpdate` from the new region.
//!
//! Nothing else in this viewer knows the avatar's facing: it is read from the
//! `ObjectUpdate` stream ([`AvatarMotion`]), the camera orbits the facing it finds
//! there, and the minimap is oriented to that camera. So without this module the
//! avatar stands at its **pre-teleport** heading until the destination's first
//! update for it lands, and then turns to the real one — which swings the
//! third-person camera around the avatar and rotates the whole minimap with it
//! (`viewer-arrival-orientation-snap`).
//!
//! The reference viewer has the same information and does not wait for it either:
//! `process_agent_movement_complete` slams the agent frame to the stated look-at
//! (`gAgentCamera.slamLookAt` → `LLAgent::resetAxes`) and re-seats the camera on
//! the avatar **without animating** (`setFocusOnAvatar(true, false)`), and
//! `process_teleport_local` does the same for an intra-region teleport. This
//! module is that slam: it writes the arrival facing onto the own avatar's
//! authoritative motion, snaps the rendered orientation to it (rather than easing
//! the P31.7 turn into place), re-seeds the walk heading, and snaps the camera so
//! the follow does not glide around the body.
//!
//! Only a **teleport** arrival slams. A region crossing carries the facing over
//! the border — the avatar was already facing that way and the simulator did not
//! turn it (`m_gotCrossUpdate` suppresses OpenSim's `RotateToLookAt`) — and the
//! reference likewise applies the look-at on a teleport alone. A degenerate
//! look-at (no horizontal component) states no facing and is ignored: OpenSim
//! substitutes the agent's velocity and then a fixed default when it has none to
//! report, and a simulator that never tracked one sends zero.

use bevy::prelude::*;
use sl_client_bevy::{SlEvent, SlIdentity, SlSessionEvent, Vector};

use crate::camera::facing_from_yaw;
use crate::coords::{sl_rotation_to_quat, sl_to_bevy_rotation};
use crate::movement::rotation_from_yaw;
use crate::world_api::{
    AvatarControls, AvatarInterp, AvatarMotion, AvatarState, CameraRig, ViewerCamera,
};

/// The shortest **horizontal** look-at length that states a facing. A shorter one
/// is degenerate — an all-zero vector, or one pointing straight up or down — and
/// carries no heading to slam to, so the arrival is left to the simulator's
/// object updates as before.
const MIN_LOOK_AT_LENGTH: f32 = 1.0e-3;

/// Apply the arrival facing of a teleport to the own avatar the moment the
/// simulator states it, instead of waiting for the destination's first
/// `ObjectUpdate` to turn the body.
///
/// Ordered after the avatar object fold and before the dead-reckoner
/// (`crate::physics::drive_avatar_motion`), so this frame's write is the one the
/// anchor is posed from: an `ObjectUpdate` still echoing the *source* region's
/// facing that arrives in the same batch as the arrival is overridden by it, and
/// the rendered orientation the dead-reckoner writes is already at the target.
pub fn slam_arrival_facing(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    mut motions: Query<(&mut AvatarMotion, Option<&mut AvatarInterp>)>,
    mut controls: ResMut<AvatarControls>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    // The last arrival in the batch wins: two teleports cannot both be current,
    // and the newer statement is the one that describes where the agent is now.
    let mut arrival = None;
    for event in events.read() {
        if let Some(yaw) = arrival_yaw(event) {
            arrival = Some(yaw);
        }
    }
    let Some(yaw) = arrival else {
        return;
    };
    debug!("arrival: slamming the own avatar's facing to {yaw:.3} rad");
    // The authoritative facing, so everything that reads the avatar's heading —
    // the camera follow, the minimap it orients, the walk direction — has the
    // arrival's answer this frame rather than the pre-teleport one.
    if let Some(anchor) = identity.agent_id.and_then(|own| avatars.body_root_of(own))
        && let Ok((mut motion, interp)) = motions.get_mut(anchor)
    {
        let rotation = rotation_from_yaw(yaw);
        let bevy_rotation = sl_to_bevy_rotation().mul_quat(sl_rotation_to_quat(&rotation));
        motion.rotation = rotation;
        // Snap the *rendered* orientation too. The P31.7 ease exists to smooth the
        // steps of a client-driven turn; a teleport's re-placement is not a turn,
        // and easing into it is exactly the visible swing this fixes. (The
        // dead-reckoner re-seeds its prediction from the changed motion below,
        // which leaves this the eased value's own start point.)
        if let Some(mut interp) = interp
            && interp.apply_rotation
        {
            interp.rendered_rotation = bevy_rotation;
        }
    }
    // The walk heading the body advertises: taken by the movement driver next
    // frame, which turns the tracked heading to it and states it to the simulator
    // at once — the reference's `send_agent_update(true, true)` after the slam.
    controls.forced_heading = Some(yaw);
    // And the camera: re-seat it on the avatar without animating, so the rear view
    // is behind the arrival facing on the first frame rather than orbiting around
    // to it (which is what rotates the minimap). Un-seeding is how this rig snaps:
    // the next pose it writes is taken whole instead of eased from the last one.
    if let Ok(mut rig) = cameras.single_mut() {
        rig.seeded = false;
        // In mouselook the camera *is* the facing (the body follows the aim), so
        // the aim itself is what the arrival turns.
        rig.aim_along(facing_from_yaw(yaw));
    }
}

/// The Second Life heading a session event states the agent arrived facing, or
/// `None` for any other event, a non-teleport arrival (a crossing / login carries
/// the facing over), or a degenerate look-at.
fn arrival_yaw(event: &SlEvent) -> Option<f32> {
    let look_at = match &event.0 {
        SlSessionEvent::AgentArrived {
            look_at,
            teleport: true,
            ..
        }
        | SlSessionEvent::TeleportLocal { look_at, .. } => look_at,
        _other => return None,
    };
    yaw_of_look_at(look_at)
}

/// The Second Life yaw (radians about the up axis) a look-at direction states, or
/// `None` when it has no horizontal component to take a heading from.
///
/// The reference flattens and normalises the look-at the same way before slamming
/// it (`LLAgentCamera::slamLookAt`).
fn yaw_of_look_at(look_at: &Vector) -> Option<f32> {
    let (x, y) = (look_at.x, look_at.y);
    (x.hypot(y) >= MIN_LOOK_AT_LENGTH).then(|| y.atan2(x))
}

#[cfg(test)]
mod tests {
    use super::{arrival_yaw, slam_arrival_facing, yaw_of_look_at};
    use crate::camera::facing_from_yaw;
    use crate::world_api::{
        AvatarControls, AvatarEntities, AvatarInterp, AvatarMotion, AvatarState, CameraRig,
        ViewerCamera,
    };
    use bevy::prelude::{App, Entity, Update};
    use sl_client_bevy::{
        AgentKey, RegionCoordinates, RegionHandle, SlEvent, SlIdentity, SlSessionEvent, Uuid,
        Vector,
    };

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A look-at vector.
    fn look(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// An `AgentArrived` facing `look_at`, from a teleport or not.
    fn arrived(look_at: Vector, teleport: bool) -> SlEvent {
        SlEvent(SlSessionEvent::AgentArrived {
            region_handle: RegionHandle(1),
            position: RegionCoordinates::new(128.0, 128.0, 25.0),
            look_at,
            teleport,
        })
    }

    /// East is heading zero and north a quarter turn, matching the Second Life
    /// frame the body rotation is built in.
    #[test]
    fn a_look_at_states_its_second_life_heading() -> Result<(), TestError> {
        let east = yaw_of_look_at(&look(1.0, 0.0, 0.0)).ok_or("east states a heading")?;
        assert!(east.abs() < 1.0e-6, "east is heading zero, got {east}");
        let north = yaw_of_look_at(&look(0.0, 1.0, 0.0)).ok_or("north states a heading")?;
        assert!(
            (north - core::f32::consts::FRAC_PI_2).abs() < 1.0e-6,
            "north is a quarter turn, got {north}"
        );
        Ok(())
    }

    /// A look-at with no horizontal component states no facing: an all-zero vector
    /// (a simulator that tracked none) and a straight-up one both leave the
    /// arrival to the object stream rather than slamming the body to an arbitrary
    /// heading.
    #[test]
    fn a_degenerate_look_at_states_no_heading() {
        assert!(
            yaw_of_look_at(&look(0.0, 0.0, 0.0)).is_none(),
            "an all-zero look-at states no heading"
        );
        assert!(
            yaw_of_look_at(&look(0.0, 0.0, 1.0)).is_none(),
            "a purely vertical look-at states no heading"
        );
    }

    /// Only a teleport arrival slams: a crossing (and the initial login) carries
    /// the facing across the border, so its stated look-at is not applied.
    #[test]
    fn only_a_teleport_arrival_states_a_heading() {
        assert!(
            arrival_yaw(&arrived(look(0.0, 1.0, 0.0), true)).is_some(),
            "a teleport arrival states the facing to slam to"
        );
        assert!(
            arrival_yaw(&arrived(look(0.0, 1.0, 0.0), false)).is_none(),
            "a crossing / login arrival does not re-place the agent"
        );
    }

    /// An intra-region teleport (`TeleportLocal`) states its facing too — the
    /// reference slams the same look-at there (`process_teleport_local`).
    #[test]
    fn an_intra_region_teleport_states_its_heading() -> Result<(), TestError> {
        let event = SlEvent(SlSessionEvent::TeleportLocal {
            position: RegionCoordinates::new(64.0, 64.0, 25.0),
            look_at: look(-1.0, 0.0, 0.0),
        });
        let yaw = arrival_yaw(&event).ok_or("an intra-region teleport states its facing")?;
        assert!(
            (yaw.abs() - core::f32::consts::PI).abs() < 1.0e-6,
            "west is a half turn, got {yaw}"
        );
        Ok(())
    }

    /// The mouselook aim the slam takes is the same direction the third-person
    /// follow reads off the body: Second Life east is Bevy `+X`, north is `-Z`.
    #[test]
    fn the_mouselook_aim_matches_the_body_facing() {
        let east = facing_from_yaw(0.0);
        assert!(
            east.abs_diff_eq(bevy::math::Vec3::X, 1.0e-6),
            "east faces Bevy +X, got {east}"
        );
        let north = facing_from_yaw(core::f32::consts::FRAC_PI_2);
        assert!(
            north.abs_diff_eq(bevy::math::Vec3::NEG_Z, 1.0e-6),
            "north faces Bevy -Z, got {north}"
        );
    }

    /// An app holding the own avatar (facing east), a camera whose rig is already
    /// seeded, and the movement controls — the world an arrival lands in.
    fn app_with_own_avatar() -> (App, AgentKey, Entity) {
        let mut app = App::new();
        app.add_message::<SlEvent>();
        let own = AgentKey::from(Uuid::from_u128(7));
        // The shared bare-avatar fixture, facing east (identity rotation).
        let object = crate::objects::fixture_object(sl_client_bevy::pcode::AVATAR);
        let motion = AvatarMotion::from_object(&object, true);
        let interp = AvatarInterp::seeded(&motion, 0.0, bevy::math::Vec3::ZERO);
        let anchor = app.world_mut().spawn((motion, interp)).id();
        let label = app.world_mut().spawn_empty().id();
        let mut avatars = AvatarState::default();
        avatars
            .objects
            .insert(own, AvatarEntities { anchor, label });
        app.insert_resource(avatars);
        app.insert_resource(SlIdentity {
            agent_id: Some(own),
            ..SlIdentity::default()
        });
        app.insert_resource(AvatarControls::default());
        app.world_mut().spawn((
            ViewerCamera,
            CameraRig {
                seeded: true,
                ..CameraRig::default()
            },
        ));
        app.add_systems(Update, slam_arrival_facing);
        (app, own, anchor)
    }

    /// The whole point: a teleport arrival turns the own avatar to the facing the
    /// simulator states **that frame** — its authoritative heading and its
    /// *rendered* orientation both — instead of leaving it at the pre-teleport
    /// heading until the destination's first object update, and it re-seats the
    /// camera without animating so the follow (and the minimap oriented to it)
    /// does not swing around the body.
    #[test]
    fn a_teleport_arrival_turns_the_own_avatar_at_once() -> Result<(), TestError> {
        let (mut app, _own, anchor) = app_with_own_avatar();
        let facing_north = core::f32::consts::FRAC_PI_2;
        app.world_mut()
            .write_message(arrived(look(0.0, 1.0, 0.0), true));
        app.update();

        let motion = app
            .world()
            .get::<AvatarMotion>(anchor)
            .ok_or("the own avatar keeps its motion")?;
        assert!(
            (motion.yaw() - facing_north).abs() < 1.0e-5,
            "the authoritative heading is the arrival's, got {}",
            motion.yaw()
        );
        let interp = app
            .world()
            .get::<AvatarInterp>(anchor)
            .ok_or("the own avatar keeps its interpolation")?;
        let target = crate::coords::sl_to_bevy_rotation()
            .mul_quat(crate::coords::sl_rotation_to_quat(&motion.rotation));
        assert!(
            interp.rendered_rotation.abs_diff_eq(target, 1.0e-5),
            "the rendered orientation is already there — no eased turn on arrival"
        );
        let forced = app
            .world()
            .resource::<AvatarControls>()
            .forced_heading
            .ok_or("the walk heading is told where to face")?;
        assert!(
            (forced - facing_north).abs() < 1.0e-5,
            "the walk heading follows the arrival facing, got {forced}"
        );
        let mut rigs = app.world_mut().query::<&CameraRig>();
        let rig = rigs
            .iter(app.world())
            .next()
            .ok_or("the camera is in the world")?;
        assert!(
            !rig.seeded,
            "the camera re-seats on the avatar without animating"
        );
        Ok(())
    }

    /// A crossing arrival changes nothing: the avatar carried its facing over the
    /// border, and turning it to a simulator's restated look-at would be the very
    /// snap this fixes.
    #[test]
    fn a_crossing_arrival_leaves_the_avatar_alone() -> Result<(), TestError> {
        let (mut app, _own, anchor) = app_with_own_avatar();
        app.world_mut()
            .write_message(arrived(look(0.0, 1.0, 0.0), false));
        app.update();

        let motion = app
            .world()
            .get::<AvatarMotion>(anchor)
            .ok_or("the own avatar keeps its motion")?;
        assert!(
            motion.yaw().abs() < 1.0e-5,
            "the avatar still faces east, got {}",
            motion.yaw()
        );
        let controls = app.world().resource::<AvatarControls>();
        assert!(
            controls.forced_heading.is_none(),
            "no heading is forced on a crossing"
        );
        let mut rigs = app.world_mut().query::<&CameraRig>();
        let rig = rigs
            .iter(app.world())
            .next()
            .ok_or("the camera is in the world")?;
        assert!(rig.seeded, "the camera keeps gliding as it was");
        Ok(())
    }
}
