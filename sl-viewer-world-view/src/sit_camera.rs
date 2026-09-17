//! The **scripted sit camera** and **forced mouselook** a seat can impose when the
//! avatar sits on it (`viewer-sit-target-and-stand-button`).
//!
//! When the simulator answers a sit request (`AvatarSitResponse`, surfaced as
//! `Event::SitResult`) the reply can carry a scripted
//! camera the seat's script set with `llSetCameraEyeOffset` / `llSetCameraAtOffset`
//! (a vehicle's driving view, a ride's fixed shot) and a `ForceMouselook` flag (a
//! weapon HUD, a first-person vehicle). This module reproduces the reference's
//! `process_avatar_sit_response` handling of both:
//!
//! - **Sit camera** — while seated with a scripted camera, the third-person camera
//!   sits at `seat + eye_offset` and looks at `seat + at_offset`, both offsets in
//!   the seat's own frame (so the view rides the seat). Enabled only when the two
//!   offsets actually differ (the reference's 1 mm threshold), i.e. the script set
//!   a camera at all. `crate::camera::position_camera` reads `SitCamera` and
//!   applies the pose; this module only tracks it.
//! - **Forced mouselook** — a seat that forces mouselook drops the camera into
//!   mouselook on sit; standing restores third person (the script-interface
//!   survey's "drop into mouselook on sit and restore on stand").
//!
//! **A reply is not a sit.** `AvatarSitResponse` is the simulator's *approval*:
//! the avatar may still have to walk to the seat (`autopilot`), the walk can be
//! cancelled, and the simulator need never honour the sit at all — so neither half
//! may act on the reply alone. Both are therefore armed by the reply and only take
//! effect once the avatar is actually sitting on that very seat (`seated_on`),
//! the reference's arrangement: both sit-camera consumption sites conjoin
//! `isSitting()`, and the forced mouselook is entered from
//! `LLVOAvatar::sitOnObject` — the avatar taking the seat — not from the message
//! handler. (The reply's `autopilot` flag is not the gate: it says the sit needs a
//! walk first, not whether the walk ever arrived.)
//!
//! Cleared on stand: when `seated_on` goes empty the scripted camera is dropped
//! and, if this module forced mouselook, third person is restored.
//!
//! Reference (Firestorm, read-only): `process_avatar_sit_response` (`llviewermessage`),
//! `LLAgentCamera::setSitCamera` / the sit-camera branch of `calcCameraPositionTargetGlobal`
//! / `calcFocusPositionTargetGlobal` (`llagentcamera`).

use bevy::prelude::*;

use sl_client_bevy::{ObjectKey, SlAgentParcel, SlEvent, SlIdentity, SlSessionEvent};

use crate::camera::FocusTarget;
use sl_viewer_world_api::{AvatarState, CameraMode, CameraRig, ViewerCamera};

/// The squared distance (metres²) the eye and at offsets must differ by for a sit
/// camera to count as "set" — the reference's `CAMERA_POSITION_THRESHOLD_SQUARED`
/// (`0.001 m` squared). Below it the seat set no meaningful camera and the ordinary
/// follow applies.
const OFFSET_THRESHOLD_SQUARED: f32 = 0.001 * 0.001;

/// The scripted sit camera the current seat imposes (if any), and whether this
/// module forced the camera into mouselook for it.
#[derive(Debug, Resource, Default)]
pub(crate) struct SitCamera {
    /// The scripted camera the newest sit response described: the seat and its eye
    /// / focus offsets. `None` when that seat set no camera. Armed by the reply, in
    /// force only while [`SitCamera::engaged`] — a sit still being walked to, or
    /// one that was never completed, leaves the camera alone.
    armed: Option<SitCameraOffsets>,
    /// Whether the avatar is actually sitting on [`SitCamera::armed`]'s seat, i.e.
    /// whether that camera is in force (the reference's `isSitting()` conjunct).
    engaged: bool,
    /// The seat whose sit response asked for mouselook (the reference's
    /// `mForceMouselook`), or `None` when the newest reply asked for none. Acted on
    /// when the avatar takes *that* seat, so neither a cancelled sit nor a stale
    /// flag from an earlier one drops the camera into first person.
    force_mouselook_seat: Option<ObjectKey>,
    /// The seat this module has already dropped the camera into mouselook for, so
    /// it is done once per sit and a user who leaves mouselook while still seated
    /// is not shoved back into it.
    mouselook_applied_for: Option<ObjectKey>,
    /// Whether the mouselook currently in force is the one this module entered, so
    /// standing knows to restore third person. Lapses the moment the camera leaves
    /// that mouselook by any other route — a mouselook the user chose themselves is
    /// theirs to keep, whatever the seat asked for earlier.
    forced_mouselook: bool,
}

impl SitCamera {
    /// The scripted camera in force: its `(seat, eye offset, at offset)` — the
    /// offsets in the seat's local Second Life frame — or `None` when no sit camera
    /// is set, or the avatar is not (yet) seated on the seat that set it.
    pub(crate) fn offsets(&self) -> Option<(ObjectKey, Vec3, Vec3)> {
        let offsets = self.armed.as_ref().filter(|_| self.engaged)?;
        Some((offsets.seat, offsets.eye, offsets.at))
    }
}

/// A scripted sit camera: the seat and its eye / focus offsets in the seat's local
/// frame (pure Second Life space — [`crate::camera::position_camera`] composes them
/// onto the seat's world transform).
#[derive(Debug)]
struct SitCameraOffsets {
    /// The seat object, resolved to its scene entity by full key each frame.
    seat: ObjectKey,
    /// The camera eye offset in the seat's frame (`llSetCameraEyeOffset`).
    eye: Vec3,
    /// The camera focus offset in the seat's frame (`llSetCameraAtOffset`).
    at: Vec3,
}

/// The sit-camera plugin: track the scripted camera / forced mouselook a seat
/// imposes, and clear it on stand.
#[derive(Debug, Clone, Copy, Default)]
pub struct SitCameraPlugin;

impl Plugin for SitCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SitCamera>().add_systems(
            Update,
            (
                ingest_sit_result,
                engage_sit_camera,
                clear_sit_camera_on_stand,
            )
                .chain(),
        );
    }
}

/// Ingest each `AvatarSitResponse`: **arm** (or disarm) the scripted sit camera
/// from the reply's eye / at offsets, and record the seat that wants mouselook —
/// mirroring the reference's `process_avatar_sit_response`, which likewise only
/// records both (`setSitCamera` / `setForceMouselook`) and leaves acting on them to
/// the moment the avatar is actually seated. [`engage_sit_camera`] does that.
fn ingest_sit_result(mut events: MessageReader<SlEvent>, mut sit_camera: ResMut<SitCamera>) {
    for event in events.read() {
        let SlSessionEvent::SitResult {
            sit_object,
            camera_eye_offset,
            camera_at_offset,
            force_mouselook,
            ..
        } = &event.0
        else {
            continue;
        };
        let eye = Vec3::new(
            camera_eye_offset.x,
            camera_eye_offset.y,
            camera_eye_offset.z,
        );
        let at = Vec3::new(camera_at_offset.x, camera_at_offset.y, camera_at_offset.z);
        // A camera is set only when the eye and at offsets meaningfully differ (the
        // seat's script called `llSetCamera*Offset`); otherwise the ordinary follow
        // applies.
        sit_camera.armed = if eye.distance_squared(at) > OFFSET_THRESHOLD_SQUARED {
            Some(SitCameraOffsets {
                seat: *sit_object,
                eye,
                at,
            })
        } else {
            None
        };
        // Recorded both ways, as the reference's `setForceMouselook(force_mouselook)`
        // is: a seat that does not force mouselook must retract the claim of the one
        // handed off from, or standing off the second seat takes the user out of a
        // mouselook they chose themselves.
        sit_camera.force_mouselook_seat = force_mouselook.then_some(*sit_object);
    }
}

/// The seat the avatar is **actually** sitting on — the reference's `isSitting()`,
/// answered with the seat's identity.
///
/// Two signals, and both are needed. [`SlAgentParcel::seated_on`] names the seat but
/// is the session's own bookkeeping, set the moment the simulator *approves* the sit
/// (`AvatarSitResponse`) — an approval the avatar may still have to walk to, and
/// which the simulator may never honour. [`AvatarState::is_seated`] is the avatar's
/// own object stream (a non-zero `ParentID` — the very signal that draws the avatar
/// on the seat), so it says whether the sit actually happened, but not for the
/// camera's purposes which object it happened on. Conjoined, they answer both.
fn seated_on(
    parcel: &SlAgentParcel,
    identity: &SlIdentity,
    avatars: &AvatarState,
) -> Option<ObjectKey> {
    let agent = identity.agent_id?;
    if !avatars.is_seated(agent) {
        return None;
    }
    parcel.seated_on
}

/// Put the armed sit camera in force — and enter the forced mouselook — once the
/// avatar is actually seated on the seat that asked for them, the reference's
/// `isSitting()` conjunct and its `LLVOAvatar::sitOnObject` mouselook branch. A sit
/// still being walked to (`autopilot`), or one that was cancelled on the way, never
/// reaches this.
fn engage_sit_camera(
    // The three resources [`seated_on`] reads, bundled into one tuple param (a
    // tuple of `SystemParam`s is itself a `SystemParam`) — they are one question
    // asked of three tables, not three inputs.
    sit_state: (Res<SlAgentParcel>, Res<SlIdentity>, Res<AvatarState>),
    mut sit_camera: ResMut<SitCamera>,
    mut mode: ResMut<CameraMode>,
    mut cameras: Query<(&Transform, &mut CameraRig), With<ViewerCamera>>,
) {
    let (parcel, identity, avatars) = &sit_state;
    let seated = seated_on(parcel, identity, avatars);
    let engaged = sit_camera
        .armed
        .as_ref()
        .is_some_and(|offsets| seated == Some(offsets.seat));
    // Written only on a change: the camera reads this resource every frame and has
    // no interest in a change flag raised by an unchanged value.
    if sit_camera.engaged != engaged {
        sit_camera.engaged = engaged;
    }
    // The claim on the current mouselook lasts exactly as long as that mouselook:
    // once the camera is out of it, whatever the user does next is their own choice
    // and standing must not undo it.
    if sit_camera.forced_mouselook && *mode != CameraMode::Mouselook {
        sit_camera.forced_mouselook = false;
    }
    // The drop into first person happens once per sit — keyed by the seat, so a user
    // who leaves mouselook while still seated is not shoved back into it. The aim is
    // seeded from the current view so the drop is continuous, matching the manual
    // mouselook toggle.
    let forcing = sit_camera
        .force_mouselook_seat
        .filter(|seat| seated == Some(*seat));
    if let Some(seat) = forcing
        && sit_camera.mouselook_applied_for != Some(seat)
    {
        if *mode != CameraMode::Mouselook {
            if let Ok((transform, mut rig)) = cameras.single_mut() {
                rig.aim_along(transform.forward().as_vec3());
            }
            *mode = CameraMode::Mouselook;
            // Named in the log like every other mode change
            // (viewer-wasd-moves-flycam-in-world): this one is not the user's
            // doing at all, so it is the most worth saying out loud.
            info!("camera: → mouselook (the seat {seat:?} forces first person)");
        }
        sit_camera.mouselook_applied_for = Some(seat);
        sit_camera.forced_mouselook = true;
    }
}

/// Clear the scripted sit camera the moment the avatar stands ([`seated_on`] goes
/// empty — a stand, a script unseating the avatar, or a sit the simulator never
/// honoured), and — if this module forced mouselook on sit — restore third person,
/// the survey's "restore on stand".
fn clear_sit_camera_on_stand(
    // One tuple param, for the reason [`engage_sit_camera`] gives.
    sit_state: (Res<SlAgentParcel>, Res<SlIdentity>, Res<AvatarState>),
    mut sit_camera: ResMut<SitCamera>,
    mut mode: ResMut<CameraMode>,
    mut focus: ResMut<FocusTarget>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
    mut was_seated: Local<bool>,
) {
    let (parcel, identity, avatars) = &sit_state;
    let seated = seated_on(parcel, identity, avatars).is_some();
    if *was_seated && !seated {
        sit_camera.armed = None;
        sit_camera.engaged = false;
        sit_camera.force_mouselook_seat = None;
        sit_camera.mouselook_applied_for = None;
        // Restore third person only if we are still in the mouselook we forced — a
        // claim [`engage_sit_camera`] drops as soon as the camera leaves it, so a
        // mouselook the user chose for themselves is left alone.
        if sit_camera.forced_mouselook {
            sit_camera.forced_mouselook = false;
            if *mode == CameraMode::Mouselook {
                *mode = CameraMode::ThirdPerson;
                *focus = FocusTarget::Avatar;
                if let Ok(mut rig) = cameras.single_mut() {
                    rig.resnap();
                }
                info!("camera: mouselook → third person (stood off a seat that forced it)");
            }
        }
    }
    *was_seated = seated;
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, CircuitId, ObjectKey, RegionLocalObjectId, Rotation, ScopedObjectId,
        SlAgentParcel, SlEvent, SlIdentity, SlSessionEvent, Uuid, Vector,
    };

    use super::{SitCamera, clear_sit_camera_on_stand, engage_sit_camera, ingest_sit_result};
    use crate::camera::FocusTarget;
    use sl_viewer_world_api::{AvatarState, CameraMode, CameraRig, SeatedTarget, ViewerCamera};

    /// The logged-in agent every test sits (and stands) with.
    fn agent() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0xa6e_u128))
    }

    /// An arbitrary seat, distinct per `id`.
    fn seat(id: u128) -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(id))
    }

    /// A headless app with the module's three systems chained, one viewer camera,
    /// and the agent logged in but standing.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<SlEvent>()
            .init_resource::<SitCamera>()
            .init_resource::<CameraMode>()
            .init_resource::<FocusTarget>()
            .init_resource::<SlAgentParcel>()
            .init_resource::<SlIdentity>()
            .init_resource::<AvatarState>()
            .add_systems(
                Update,
                (
                    ingest_sit_result,
                    engage_sit_camera,
                    clear_sit_camera_on_stand,
                )
                    .chain(),
            );
        app.world_mut()
            .spawn((ViewerCamera, Transform::default(), CameraRig::default()));
        app.world_mut().resource_mut::<SlIdentity>().agent_id = Some(agent());
        app
    }

    /// The simulator's approval of a sit on `on`, with a scripted camera when
    /// `camera` and the mouselook flag as given.
    fn sit_response(app: &mut App, on: ObjectKey, camera: bool, force_mouselook: bool) {
        let origin = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // A camera is "set" exactly when the two offsets differ; equal offsets are
        // the seat that scripted none.
        let eye = if camera {
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.5,
            }
        } else {
            origin.clone()
        };
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::SitResult {
                sit_object: on,
                autopilot: false,
                sit_position: origin.clone(),
                sit_rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                camera_eye_offset: eye,
                camera_at_offset: origin,
                force_mouselook,
            }));
    }

    /// The session's optimistic seat *and* the avatar's own object stream agreeing
    /// that the avatar sits on `on` — `None` stands it up.
    fn sit_avatar(app: &mut App, on: Option<ObjectKey>) {
        app.world_mut().resource_mut::<SlAgentParcel>().seated_on = on;
        let mut avatars = app.world_mut().resource_mut::<AvatarState>();
        match on {
            Some(_) => {
                avatars.seated.insert(
                    agent(),
                    SeatedTarget {
                        seat: ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(7)),
                        offset: Transform::default(),
                    },
                );
            }
            None => {
                avatars.seated.remove(&agent());
            }
        }
    }

    /// The camera mode the app currently holds.
    fn mode(app: &App) -> CameraMode {
        *app.world().resource::<CameraMode>()
    }

    /// The seat whose scripted camera is in force, if any.
    fn in_force(app: &App) -> Option<ObjectKey> {
        app.world()
            .resource::<SitCamera>()
            .offsets()
            .map(|(seat, _eye, _at)| seat)
    }

    /// A sit response alone moves nothing: until the avatar is actually on the
    /// seat, the scripted camera stays out of force and mouselook is not entered.
    /// A sit that is never completed therefore never welds the camera to the seat.
    #[test]
    fn a_reply_without_a_sit_moves_no_camera() {
        let mut app = app();
        sit_response(&mut app, seat(1), true, true);
        app.update();
        assert_eq!(in_force(&app), None, "the sit has not happened yet");
        assert_eq!(mode(&app), CameraMode::ThirdPerson, "no forced mouselook");
        // Still nothing many frames later — the reply does not ripen on its own.
        app.update();
        app.update();
        assert_eq!(in_force(&app), None);
        assert_eq!(mode(&app), CameraMode::ThirdPerson);
    }

    /// Taking the seat puts the scripted camera in force and enters the forced
    /// mouselook; standing drops both.
    #[test]
    fn taking_the_seat_engages_the_camera_and_standing_clears_it() {
        let mut app = app();
        sit_response(&mut app, seat(1), true, true);
        sit_avatar(&mut app, Some(seat(1)));
        app.update();
        assert_eq!(in_force(&app), Some(seat(1)), "seated on the scripted seat");
        assert_eq!(mode(&app), CameraMode::Mouselook, "the seat forced it");
        sit_avatar(&mut app, None);
        app.update();
        assert_eq!(in_force(&app), None, "standing drops the scripted camera");
        assert_eq!(
            mode(&app),
            CameraMode::ThirdPerson,
            "standing restores third person"
        );
    }

    /// The session's optimistic seat is not enough on its own: a sit the simulator
    /// approved but never honoured (no `ParentID` on the avatar's own object) leaves
    /// the camera alone.
    #[test]
    fn an_unhonoured_sit_does_not_engage() {
        let mut app = app();
        sit_response(&mut app, seat(1), true, true);
        app.world_mut().resource_mut::<SlAgentParcel>().seated_on = Some(seat(1));
        app.update();
        assert_eq!(in_force(&app), None, "the avatar never took the seat");
        assert_eq!(mode(&app), CameraMode::ThirdPerson);
    }

    /// Sitting on a *different* seat than the one the reply described does not
    /// engage that reply's camera.
    #[test]
    fn another_seat_does_not_engage_this_reply() {
        let mut app = app();
        sit_response(&mut app, seat(1), true, false);
        sit_avatar(&mut app, Some(seat(2)));
        app.update();
        assert_eq!(in_force(&app), None, "a different seat");
    }

    /// A seat that does not force mouselook retracts the claim of the one handed
    /// off from: standing off the second seat leaves the user's own mouselook
    /// choice alone.
    #[test]
    fn a_handoff_does_not_steal_the_users_mouselook() {
        let mut app = app();
        // Seat one forces mouselook.
        sit_response(&mut app, seat(1), false, true);
        sit_avatar(&mut app, Some(seat(1)));
        app.update();
        assert_eq!(mode(&app), CameraMode::Mouselook, "seat one forced it");
        // The user leaves mouselook while still seated, and is not shoved back.
        *app.world_mut().resource_mut::<CameraMode>() = CameraMode::ThirdPerson;
        app.update();
        assert_eq!(mode(&app), CameraMode::ThirdPerson, "left by the user");
        // Straight onto a second seat that forces nothing, where the user enters
        // mouselook of their own accord.
        sit_response(&mut app, seat(2), false, false);
        sit_avatar(&mut app, Some(seat(2)));
        app.update();
        *app.world_mut().resource_mut::<CameraMode>() = CameraMode::Mouselook;
        app.update();
        // Standing must leave that choice alone.
        sit_avatar(&mut app, None);
        app.update();
        assert_eq!(
            mode(&app),
            CameraMode::Mouselook,
            "the user's own mouselook survives the stand"
        );
    }

    /// A seat whose script set no camera (equal eye and at offsets) leaves the
    /// ordinary follow in place, seated or not.
    #[test]
    fn a_seat_without_a_scripted_camera_engages_nothing() {
        let mut app = app();
        sit_response(&mut app, seat(1), false, false);
        sit_avatar(&mut app, Some(seat(1)));
        app.update();
        assert_eq!(in_force(&app), None, "no scripted camera to engage");
        assert_eq!(mode(&app), CameraMode::ThirdPerson);
    }
}
