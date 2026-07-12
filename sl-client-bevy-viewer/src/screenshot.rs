//! A debug screenshot-capture harness (used to diagnose R11, the base-body skin
//! distortion under animation).
//!
//! When `SL_VIEWER_SCREENSHOT_DIR` is set, the viewer saves a numbered sequence
//! of PNG frames of the primary window at a fixed interval — after a startup
//! delay long enough for login, asset decode, baking, and the debug animation to
//! settle — then quits. This lets an animated avatar be inspected offline,
//! frame by frame, without an operator sitting at the live window, and (since it
//! leaves the cursor un-grabbed) without hijacking the desktop it runs on.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use sl_client_bevy::SlCommand;

use crate::session::{ViewerSession, request_logout};

/// The screenshot capture schedule, inserted only in screenshot mode.
#[derive(Resource)]
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

/// Capture the primary window to `frame_NNN.png` on the schedule, then request a
/// clean grid logout once the last frame is taken.
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
) {
    let now = time.elapsed_secs();
    let start_delay = schedule.start_delay;
    let next_at = *schedule.next_at.get_or_insert(start_delay);
    if now < next_at {
        return;
    }
    if schedule.index >= schedule.max_frames {
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
        .observe(save_to_disk(path));
    schedule.index = schedule.index.saturating_add(1);
    schedule.next_at = Some(now + schedule.interval);
}
