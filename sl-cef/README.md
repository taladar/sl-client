# sl-cef

Offscreen **Chromium (CEF)** web-media engine for Second Life / OpenSim
viewers. Wraps the [`cef`](https://crates.io/crates/cef) crate (prebuilt CEF
binaries from the Spotify CDN — Chromium is never built locally) behind a
small, engine-agnostic API so a viewer can render web content both onto
in-world prim faces (media-on-a-prim) and into UI widgets (embedded browser
panels):

- `MediaBackend` — initialise once, create surfaces, pump once per frame,
  shut down.
- `MediaSurface` — one windowless browser: navigation, resize, portable
  mouse/keyboard input in; CPU BGRA frames, status snapshots and popup
  requests out.

Input crosses the boundary as *Windows virtual-key code + committed text*
(the `vk` module), never native key blobs. Frames cross as CPU BGRA buffers
(`on_paint`); GPU zero-copy is deferred headroom. Each surface can run in an
isolated in-memory request context so hostile in-world pages cannot read
another surface's cookies.

The crate also ships the `sl-cef-helper` binary — CEF's subprocess
executable, which must be installed next to the embedding binary (cargo
places both in the same target directory).

Build note: the `cef-dll-sys` build script downloads the CEF binary
distribution on first build (respects `CEF_PATH`; this workspace pins it to
`.cef/` via `.cargo/config.toml`) and copies the runtime files
(`libcef.so`, `*.pak`, `locales/`, …) into the cargo target directory.
