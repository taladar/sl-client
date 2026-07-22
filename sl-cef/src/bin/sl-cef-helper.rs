//! The CEF subprocess helper.
//!
//! CEF (Chromium) is multi-process: the browser process spawns renderer, GPU
//! and utility subprocesses. Pointing `CefSettings.browser_subprocess_path`
//! at this tiny binary keeps those subprocesses out of the viewer executable
//! (no Bevy, no re-parsed CLI). It must be installed next to the viewer
//! binary; `cargo build` places both in the same target directory.

/// Entry point: run the CEF subprocess main and exit with its code.
fn main() {
    let code = sl_cef::chromium::execute_child_process();
    if code < 0 {
        // Not launched by CEF at all — a human ran it. Explain and leave.
        eprintln!(
            "sl-cef-helper is the CEF subprocess helper for the sl-client viewer; \
             it is started automatically and not meant to be run directly."
        );
        std::process::exit(0);
    }
    std::process::exit(code);
}
