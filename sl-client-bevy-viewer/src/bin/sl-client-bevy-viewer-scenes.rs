//! The render gallery binary: a thin shell over
//! [`sl_client_bevy_viewer::render_gallery::run`].
//!
//! See that module for what the gallery is for. Everything of substance lives in
//! the library, which is what lets this binary render the viewer's real geometry
//! converters rather than a second, drifting copy of them.

/// Entry point: hand straight over to the library.
fn main() {
    sl_client_bevy_viewer::render_gallery::run();
}
