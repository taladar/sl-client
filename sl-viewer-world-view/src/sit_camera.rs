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
//! Cleared on stand: when [`SlAgentParcel::seated_on`] goes empty the scripted
//! camera is dropped and, if this module forced mouselook, third person is
//! restored.
//!
//! Reference (Firestorm, read-only): `process_avatar_sit_response` (`llviewermessage`),
//! `LLAgentCamera::setSitCamera` / the sit-camera branch of `calcCameraPositionTargetGlobal`
//! / `calcFocusPositionTargetGlobal` (`llagentcamera`).

use bevy::prelude::*;

use sl_client_bevy::{ObjectKey, SlAgentParcel, SlEvent, SlSessionEvent};

use crate::camera::FocusTarget;
use crate::world_api::{CameraMode, CameraRig, ViewerCamera};

/// The squared distance (metres²) the eye and at offsets must differ by for a sit
/// camera to count as "set" — the reference's `CAMERA_POSITION_THRESHOLD_SQUARED`
/// (`0.001 m` squared). Below it the seat set no meaningful camera and the ordinary
/// follow applies.
const OFFSET_THRESHOLD_SQUARED: f32 = 0.001 * 0.001;

/// The scripted sit camera the current seat imposes (if any), and whether this
/// module forced the camera into mouselook for it.
#[derive(Debug, Resource, Default)]
pub(crate) struct SitCamera {
    /// The active scripted camera: the seat and its eye / focus offsets. `None`
    /// when the seat set no camera, or the avatar is not seated.
    active: Option<SitCameraOffsets>,
    /// Whether this module dropped the camera into mouselook for the current sit,
    /// so standing knows to restore third person.
    forced_mouselook: bool,
}

impl SitCamera {
    /// The active scripted camera's `(seat, eye offset, at offset)` — the offsets in
    /// the seat's local Second Life frame — or `None` when no sit camera is set.
    pub(crate) fn offsets(&self) -> Option<(ObjectKey, Vec3, Vec3)> {
        self.active
            .as_ref()
            .map(|offsets| (offsets.seat, offsets.eye, offsets.at))
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
            (ingest_sit_result, clear_sit_camera_on_stand).chain(),
        );
    }
}

/// Ingest each `AvatarSitResponse`: set (or clear) the scripted sit camera from the
/// reply's eye / at offsets, and drop into mouselook when the seat forces it —
/// mirroring the reference's `process_avatar_sit_response`.
fn ingest_sit_result(
    mut events: MessageReader<SlEvent>,
    mut sit_camera: ResMut<SitCamera>,
    mut mode: ResMut<CameraMode>,
    mut cameras: Query<(&Transform, &mut CameraRig), With<ViewerCamera>>,
) {
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
        sit_camera.active = if eye.distance_squared(at) > OFFSET_THRESHOLD_SQUARED {
            Some(SitCameraOffsets {
                seat: *sit_object,
                eye,
                at,
            })
        } else {
            None
        };
        // A seat that forces mouselook drops the camera into first person on sit; the
        // aim is seeded from the current view so the drop is continuous, matching the
        // manual mouselook toggle.
        if *force_mouselook {
            if *mode != CameraMode::Mouselook {
                if let Ok((transform, mut rig)) = cameras.single_mut() {
                    rig.aim_along(transform.forward().as_vec3());
                }
                *mode = CameraMode::Mouselook;
            }
            sit_camera.forced_mouselook = true;
        }
    }
}

/// Clear the scripted sit camera the moment the avatar stands
/// ([`SlAgentParcel::seated_on`] goes empty), and — if this module forced
/// mouselook on sit — restore third person, the survey's "restore on stand".
fn clear_sit_camera_on_stand(
    parcel: Res<SlAgentParcel>,
    mut sit_camera: ResMut<SitCamera>,
    mut mode: ResMut<CameraMode>,
    mut focus: ResMut<FocusTarget>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
    mut was_seated: Local<bool>,
) {
    let seated = parcel.seated_on.is_some();
    if *was_seated && !seated {
        sit_camera.active = None;
        if sit_camera.forced_mouselook {
            sit_camera.forced_mouselook = false;
            // Restore third person only if we are still in the mouselook we forced —
            // if the user has since left it, leave their choice alone.
            if *mode == CameraMode::Mouselook {
                *mode = CameraMode::ThirdPerson;
                *focus = FocusTarget::Avatar;
                if let Ok(mut rig) = cameras.single_mut() {
                    rig.resnap();
                }
            }
        }
    }
    *was_seated = seated;
}
