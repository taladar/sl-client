# sl-media

The engine-agnostic media-surface boundary for a Second Life / OpenSim
viewer: the `MediaBackend` / `MediaSurface` traits plus the plain data
types crossing them (CPU BGRA frames, portable keyboard/mouse input,
navigation and playback status), shared by the two media engines —

- **`sl-cef`** — the offscreen Chromium web engine (media-on-a-prim web
  pages, the embedded browser, login/marketplace/profile pages);
- **`sl-gst`** — the GStreamer playback engine (direct video / audio
  URLs, HLS/DASH manifests, parcel radio streams).

The viewer's surface-driving, texture-mirroring and input-routing code
speaks only these traits, so either engine can be swapped or disabled
without touching the UI layer — the reference viewer's `LLPluginClassMedia`
boundary, without the out-of-process plugin machinery.

Also here: `classify_url`, the URL → media-kind dispatch table (the
reference's `mime_types.xml` dispatch, simplified to URL scheme / path
extension) that decides which engine a media URL is handed to.

Design notes live in the workspace roadmap
(`roadmap/in-progress/viewer-media-prim-browser.md`,
`roadmap/in-progress/viewer-video-playback.md`): input crosses the
boundary portably (Windows virtual-key codes + committed text, never
native key blobs), frames cross as CPU BGRA buffers (zero-copy stays
deferred headroom), and playback controls (play / pause / seek / volume)
are default-no-op trait methods so each engine implements only its half.
