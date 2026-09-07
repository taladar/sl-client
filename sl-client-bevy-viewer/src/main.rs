//! The viewer binary: a thin shell over [`sl_client_bevy_viewer::run`].
//!
//! Everything of substance lives in the library, so the gallery binary
//! (`sl-client-bevy-viewer-gallery`) can build the very same UI modules against
//! it rather than a second, drifting copy.

/// Entry point: hand straight over to the library, and report what it says.
///
/// Returning the `Result` itself would exit non-zero just the same, but it
/// prints the error's **`Debug`**, which buries the one sentence saying what
/// went wrong inside the struct carrying it and shows the cause only as
/// nesting. Rendering `Display` and walking the source chain is what a person
/// reading a failed unattended run actually needs.
///
/// Reported through `tracing` at `error!`, the level the crate already uses for
/// a launch that fails outright, so the failure lands in the same log as
/// everything that led to it and is visible even at `RUST_LOG=error`. The
/// subscriber outlives [`run`](sl_client_bevy_viewer::run) — only the profiler
/// guard it holds is dropped when that returns.
///
/// [`ExitCode::FAILURE`] is `1` for every failure. The exact code a failing
/// `AppExit` chose is named in the message rather than becoming the status:
/// this returns normally so those guards still flush, which
/// `std::process::exit` would skip — losing the log that explains the failure
/// in the very act of reporting it.
fn main() -> std::process::ExitCode {
    let Err(error) = sl_client_bevy_viewer::run() else {
        return std::process::ExitCode::SUCCESS;
    };
    tracing::error!("{error}");
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        tracing::error!("  caused by: {cause}");
        source = cause.source();
    }
    std::process::ExitCode::FAILURE
}
