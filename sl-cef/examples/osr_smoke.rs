//! Live smoke test of the crate API: initialise the backend (with the
//! `sl-cef-helper` subprocess binary), open one offscreen surface on a real
//! page, pump until it painted after the load finished, write the frame as a
//! PPM, and shut down cleanly.
//!
//! Run with the helper built and the CEF runtime files on the library path:
//!
//! ```console
//! cargo build -p sl-cef
//! LD_LIBRARY_PATH=target/debug cargo run -p sl-cef --example osr_smoke
//! ```

use sl_cef::chromium::CefMediaBackend;
use sl_cef::{BackendConfig, MediaBackend, SurfaceConfig};

/// Initialise, load `https://example.com/`, dump one frame, shut down.
fn main() {
    let target_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .and_then(|dir| dir.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("target/debug"));
    let helper = target_dir.join("sl-cef-helper");
    let cache = std::env::temp_dir().join("sl-cef-osr-smoke");

    let mut backend = CefMediaBackend::initialize(&BackendConfig {
        cache_dir: cache,
        subprocess_path: Some(helper),
        locale: Some(String::from("en-US")),
        user_agent_product: None,
    })
    .unwrap_or_else(|error| panic!("backend init failed: {error}"));

    let surface = backend
        .create_surface(&SurfaceConfig {
            width: 800,
            height: 600,
            initial_url: String::from("https://example.com/"),
            isolated: true,
            max_fps: 30,
            muted: true,
            loop_media: false,
        })
        .unwrap_or_else(|error| panic!("surface creation failed: {error}"));

    let start = std::time::Instant::now();
    let mut seen_generation = 0_u64;
    let mut written = false;
    while start.elapsed() < std::time::Duration::from_secs(20) {
        backend.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let status = surface.status();
        if !status.loading && status.progress >= 1.0 {
            written = surface.with_new_frame(&mut seen_generation, &mut |frame| {
                let mut out = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
                for pixel in frame.bgra.chunks_exact(4) {
                    let (b, g, r) = (
                        pixel.first().copied().unwrap_or(0),
                        pixel.get(1).copied().unwrap_or(0),
                        pixel.get(2).copied().unwrap_or(0),
                    );
                    out.push(r);
                    out.push(g);
                    out.push(b);
                }
                fs_err::write("osr-smoke-frame.ppm", &out)
                    .unwrap_or_else(|error| panic!("writing the frame failed: {error}"));
                println!(
                    "wrote osr-smoke-frame.ppm ({}x{})",
                    frame.width, frame.height
                );
            });
            if written {
                break;
            }
        }
    }
    assert!(written, "no frame captured within the timeout");
    println!("final status: {:?}", surface.status());

    drop(surface);
    backend.shutdown();
    println!("clean shutdown OK");
}
