//! A debug screenshot-capture harness (used to diagnose R11, the base-body skin
//! distortion under animation).
//!
//! When `SL_VIEWER_SCREENSHOT_DIR` is set, the viewer saves a numbered sequence
//! of PNG frames of the primary window at a fixed interval — after a startup
//! delay long enough for login, asset decode, baking, and the debug animation to
//! settle — then quits. This lets an animated avatar be inspected offline,
//! frame by frame, without an operator sitting at the live window, and (since it
//! leaves the cursor un-grabbed) without hijacking the desktop it runs on.
//!
//! The per-frame PNG encode + disk write is done **off the main thread** on Bevy's
//! [`IoTaskPool`] (like the user-facing Snapshot floater), rather than with Bevy's
//! synchronous `save_to_disk` observer. A full-resolution PNG deflate on the frame
//! thread stalls the frame and spikes the next frame's `Time::delta`, which made
//! time-based animations (the water surface, driven by `time.elapsed_secs()`) jump
//! on the catch-up frame — the "water briefly accelerates then normal" artifact
//! seen during capture runs. Off-thread, the capture costs the frame nothing past
//! the (already off-thread) GPU read-back, so the harness better reflects live
//! behaviour.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::SlCommand;

use crate::session::{ViewerSession, request_logout};

/// The offline-inspection screenshot harness (R11): capture a numbered PNG
/// sequence of the window into `dir` after a startup delay, then quit.
///
/// Added only in screenshot mode, which is why the schedule resource is carried
/// on the plugin rather than initialised from the world.
#[derive(Debug)]
pub struct ScreenshotPlugin {
    /// Directory the PNG sequence is written to.
    pub dir: PathBuf,
}

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ScreenshotSchedule::new(self.dir.clone()))
            .add_systems(Update, (capture_screenshots, poll_screenshot_saves));
    }
}

/// The screenshot capture schedule, inserted only in screenshot mode.
#[derive(Debug, Resource)]
pub(crate) struct ScreenshotSchedule {
    /// Directory the PNG sequence is written to.
    dir: PathBuf,
    /// Seconds to wait after startup before the first capture (login + asset
    /// decode + bake + animation start all have to settle first).
    start_delay: f32,
    /// Seconds between successive captures.
    interval: f32,
    /// How many frames to capture before quitting.
    max_frames: usize,
    /// The next capture time (elapsed seconds); `None` until the delay is armed.
    next_at: Option<f32>,
    /// The index of the next frame to write.
    index: usize,
}

impl ScreenshotSchedule {
    /// A schedule writing `SL_VIEWER_SCREENSHOT_FRAMES` frames (default 30) at
    /// `SL_VIEWER_SCREENSHOT_INTERVAL` s (default 0.5) after a
    /// `SL_VIEWER_SCREENSHOT_DELAY` s startup delay (default 25). The delay is
    /// deliberately generous — and tunable without a rebuild — because a real
    /// login to a live grid, plus fetching / decoding the worn wearables and
    /// baking, can take many seconds before the animated body is fully on screen.
    #[must_use]
    pub(crate) fn new(dir: PathBuf) -> Self {
        let env_f32 = |key: &str, default: f32| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        let env_usize = |key: &str, default: usize| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        Self {
            dir,
            start_delay: env_f32("SL_VIEWER_SCREENSHOT_DELAY", 25.0),
            interval: env_f32("SL_VIEWER_SCREENSHOT_INTERVAL", 0.5),
            max_frames: env_usize("SL_VIEWER_SCREENSHOT_FRAMES", 30),
            next_at: None,
            index: 0,
        }
    }
}

/// A pending off-thread screenshot write, spawned by [`capture_screenshots`] and
/// drained by [`poll_screenshot_saves`]. The task yields the written path on
/// success, or a formatted error string, so a failed write surfaces in the log
/// rather than being swallowed.
#[derive(Debug, Component)]
pub(crate) struct ScreenshotSaveTask(Task<Result<PathBuf, String>>);

/// Capture the primary window to `frame_NNN.png` on the schedule, then request a
/// clean grid logout once the last frame is taken **and** its write has finished.
///
/// The PNG encode + disk write is offloaded to [`IoTaskPool`]; the logout is held
/// until every pending [`ScreenshotSaveTask`] has drained so a race between the
/// last frame's write and process exit can't truncate the final PNG(s).
///
/// The logout (rather than an immediate `AppExit`) is what lets the run leave the
/// avatar cleanly logged out: an abrupt process exit strands the grid session, and
/// the next login is then rejected until the grid times the stale presence out. The
/// actual exit is driven by the session systems (on `LoggedOut`, or the quit-deadline
/// fallback), the same as the `Esc` / `Q` quit key.
pub(crate) fn capture_screenshots(
    time: Res<Time>,
    mut schedule: ResMut<ScreenshotSchedule>,
    mut commands: Commands,
    mut session: ResMut<ViewerSession>,
    mut sl_commands: MessageWriter<SlCommand>,
    pending_saves: Query<(), With<ScreenshotSaveTask>>,
) {
    let now = time.elapsed_secs();
    let start_delay = schedule.start_delay;
    let next_at = *schedule.next_at.get_or_insert(start_delay);
    if now < next_at {
        return;
    }
    if schedule.index >= schedule.max_frames {
        // Don't log out (and so quit) while a frame's PNG is still being written
        // off-thread — dropping the task at exit would truncate the file.
        if !pending_saves.is_empty() {
            return;
        }
        info!(
            "screenshot: captured {} frames; logging out",
            schedule.index
        );
        request_logout(&mut session, &mut sl_commands, now);
        return;
    }
    let path = schedule
        .dir
        .join(format!("frame_{:03}.png", schedule.index));
    info!("screenshot: capturing {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_off_thread(path));
    schedule.index = schedule.index.saturating_add(1);
    schedule.next_at = Some(now + schedule.interval);
}

/// Build the [`ScreenshotCaptured`] observer that writes one captured frame to
/// `path` off the main thread.
///
/// The frame is decoded to an opaque RGB image on the frame thread (dropping the
/// HDR alpha, which carries brightness — the same as Bevy's `save_to_disk`), then
/// the heavy PNG deflate + disk write is handed to [`IoTaskPool`] via a
/// [`ScreenshotSaveTask`] that [`poll_screenshot_saves`] drains.
fn save_off_thread(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>, Commands) {
    move |captured, mut commands| {
        let capture_entity = captured.entity;
        let dynamic = match captured.image.clone().try_into_dynamic() {
            // Discard the alpha channel (HDR brightness) so the PNG looks right.
            Ok(dynamic) => image::DynamicImage::ImageRgb8(dynamic.to_rgb8()),
            Err(error) => {
                error!("screenshot: cannot decode capture: {error}");
                commands.entity(capture_entity).despawn();
                return;
            }
        };
        let path = path.clone();
        let task = IoTaskPool::get().spawn(async move {
            let format = image::ImageFormat::from_path(&path).map_err(|error| error.to_string())?;
            dynamic
                .save_with_format(&path, format)
                .map_err(|error| error.to_string())?;
            Ok(path)
        });
        commands.spawn(ScreenshotSaveTask(task));
        // One-shot; drop the capture entity so a save does not leak one.
        commands.entity(capture_entity).despawn();
    }
}

/// Poll the off-thread screenshot writes; when one finishes, log the saved path
/// (or the write error), then drop the task entity. Runs every frame; a write in
/// flight costs one cheap non-blocking poll.
pub(crate) fn poll_screenshot_saves(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut ScreenshotSaveTask)>,
) {
    for (entity, mut task) in &mut tasks {
        let Some(result) = block_on(poll_once(&mut task.0)) else {
            continue;
        };
        match result {
            Ok(path) => info!("screenshot: saved {}", path.display()),
            Err(error) => error!("screenshot: save failed: {error}"),
        }
        commands.entity(entity).despawn();
    }
}
