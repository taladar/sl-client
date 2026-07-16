//! The viewer binary: a thin shell over [`sl_client_bevy_viewer::run`].
//!
//! Everything of substance lives in the library, so the gallery binary
//! (`sl-client-bevy-viewer-gallery`) can build the very same UI modules against
//! it rather than a second, drifting copy.

/// Entry point: hand straight over to the library.
fn main() -> Result<(), sl_client_bevy_viewer::Error> {
    sl_client_bevy_viewer::run()
}
