//! The render gallery binary: a thin shell over
//! [`sl_viewer_gallery::render_gallery::run`].
//!
//! See that module for what the gallery is for. What this shell adds is the
//! `assets/` root — whose development fallback is *this* crate's compile-time
//! directory, so only a binary in this crate can resolve it — and the tracing
//! subscriber the viewer's `profile-*` features configure.

/// Entry point: resolve the asset root, then hand over to the gallery crate and
/// let its [`AppExit`] **be** the process's exit status — `AppExit` implements
/// `Termination`, so a failing run reaches a harness's exit-code check with its
/// own code intact.
///
/// [`AppExit`]: bevy::app::AppExit
fn main() -> bevy::app::AppExit {
    // Held for the whole process so the Chrome profiler (if enabled) flushes.
    let _tracing_guards = sl_client_bevy_viewer::init_tracing();
    sl_viewer_gallery::render_gallery::run(sl_client_bevy_viewer::asset_root::asset_plugin(None))
}
