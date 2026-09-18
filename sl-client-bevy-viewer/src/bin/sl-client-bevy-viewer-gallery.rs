//! The UI gallery binary: a thin shell over [`sl_viewer_gallery::gallery::run`].
//!
//! See that module for what the gallery is for. What this shell adds is the
//! four things a gallery cannot resolve for itself, because each of them is
//! composition: the element and floater registries, the `assets/` root (whose
//! development fallback is *this* crate's compile-time directory), and the
//! tracing subscriber the viewer's `profile-*` features configure.

/// Entry point: gather the registries and the resolved asset root, then hand
/// over to the gallery crate and let its [`AppExit`] **be** the process's exit
/// status — `AppExit` implements `Termination`, so a failing run reaches a
/// harness's exit-code check with its own code intact.
///
/// [`AppExit`]: bevy::app::AppExit
fn main() -> bevy::app::AppExit {
    // Held for the whole process so the Chrome profiler (if enabled) flushes.
    let _tracing_guards = sl_client_bevy_viewer::init_tracing();
    sl_viewer_gallery::gallery::run(
        // Watch the skin `.css` files: the gallery is the skin-authoring
        // surface, so an edit re-applies live here without a restart.
        sl_client_bevy_viewer::asset_root::asset_plugin(Some(true)),
        sl_viewer_gallery::gallery::GalleryRegistry {
            elements: sl_client_bevy_viewer::ui_elements::ELEMENTS,
            floaters: sl_client_bevy_viewer::floaters::FLOATERS,
        },
    )
}
