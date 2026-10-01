//! The CEF subprocess helper.
//!
//! CEF (Chromium) is multi-process: the browser process spawns renderer, GPU
//! and utility subprocesses. Pointing `CefSettings.browser_subprocess_path`
//! at this tiny binary keeps those subprocesses out of the viewer executable
//! (no Bevy, no re-parsed CLI). It must be installed next to the viewer
//! binary.
//!
//! It is a target of the **viewer's** package, not of `sl-cef` whose code it
//! runs: so every build of the viewer — its tests included, and a test run
//! that builds no other package — puts it in the same target directory. When
//! it lived in `sl-cef`, web media was silently off in any build that had not
//! also built that package.

/// Entry point: run the CEF subprocess main and exit with its code.
fn main() {
    let code = sl_cef::chromium::execute_child_process();
    if code < 0 {
        // Not launched by CEF at all — a human ran it. Explain and leave.
        #[expect(
            clippy::print_stderr,
            reason = "a person ran the helper by hand: standard error is the only place to tell \
                      them, and the subprocess has no logging set up to use instead"
        )]
        {
            eprintln!(
                "sl-cef-helper is the CEF subprocess helper for the sl-client viewer; \
                 it is started automatically and not meant to be run directly."
            );
        }
        std::process::exit(0);
    }
    std::process::exit(code);
}
