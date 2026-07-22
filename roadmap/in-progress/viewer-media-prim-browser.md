---
id: viewer-media-prim-browser
title: Media-on-a-prim & embedded web browser
topic: viewer
status: in-progress
origin: reference-viewer feature-cluster survey (2026-07)
refs: [viewer-audio-backend, viewer-video-playback]
---

Context: [context/viewer.md](../context/viewer.md).

Render web pages onto prim faces (media-on-a-prim) and provide a general
in-viewer browser. Four distinct consumers, and the first one is easy to
overlook: **the SL login page is itself HTML in a browser view**
(`panel_login.xml` declares a `web_browser` named `login_html`), so a browser
must be alive *before login*. Then: the in-viewer browser floater, search /
profiles / marketplace / L$ purchase floaters, and MoaP surfaces in-world.

The protocol is **already done** (`protocol-24`): `Event::ObjectMedia` delivers
a `MediaEntry` per prim face over the `ObjectMedia` cap, with
`RequestObjectMedia` / `SetObjectMedia` / `NavigateObjectMedia` commands, and
`MediaEntry` carries the URL, MIME type, whitelist, `first_click_interact`,
auto-play and auto-zoom flags. The runtimes ingest it (`sl-client-bevy/
src/media.rs`) but **the viewer crate never reads it**. This task is the surface
and the engine.

## Engine choice (surveyed 2026-07)

**Prebuilt CEF via the `cef` crate (tauri-apps/cef-rs).** It is the only option
that is simultaneously tri-platform, web-compatible enough for the real SL login
/ marketplace / profile pages, offscreen-renderable, and consumable **as a
prebuilt binary** — the crate downloads the Spotify CDN builds, so we never
build Chromium. It is Apache-2.0 and actively maintained, and it already ships
an **`osr_texture_import` layer that hands you a `wgpu::Texture`** (DMA-BUF on
Linux, D3D11 shared handle on Windows, IOSurface on macOS) pinned to **wgpu 29 —
exactly Bevy 0.19's wgpu**. `bevy_cef` (Bevy 0.19) exists as a reference
integration, but its Linux path is CPU-only; take ideas from it and depend on
`cef` directly.

Use **`cef` directly, not Dullahan** (the wrapper Firestorm uses): Dullahan does
not expose `CefAudioHandler`, which is precisely why Firestorm's browser audio
bypasses the viewer entirely and can only be attenuated by a PulseAudio
sink-input hack with `setPan()` left an empty stub. Going direct gets us
`OnAudioStreamPacket` (non-interleaved f32 PCM + timestamps) → our mixer →
**MoaP audio actually spatialised at the prim**, which no SL viewer does today.

**Dependency shape (deliberate).** Everything here is a *direct* dependency we
can patch or vendor: `cef` speaks plain `wgpu`, and GStreamer knows nothing of
wgpu or Bevy at all. Depending on `cef` rather than `bevy_cef` is what keeps us
off the Bevy-wrapper upgrade treadmill — we are never waiting for a middle crate
to adopt a new Bevy. Prefer that rule generally: take the engine-agnostic core
and write the thin Bevy glue ourselves. The one alignment to watch is that `cef`
and Bevy must agree on the **wgpu major version** (both on 29 today) — the same
mismatch that disqualified other candidates, and the reason to track cef-rs's
wgpu bumps when Bevy moves.

Rejected: **WPE WebKit** (the elegant answer — offscreen by design, system
GStreamer for media — but there is no credible Windows port, so it forecloses
Win/macOS); **Servo** (embedding API is real now, but ~20 % of Baseline web
features and Verso archived in 2025 — it cannot render the marketplace);
**Ultralight** (proprietary, licence incompatible with an open-source viewer);
**wry** (no offscreen rendering at all); **Blitz** (no JS by design);
**CDP/screenshot streaming** (absurd for per-frame surfaces).

## Codecs — and why in-page video is a known, accepted gap

Stock prebuilt CEF is `ffmpeg_branding=Chromium`: **VP8 / VP9 / AV1 / Opus /
Vorbis / FLAC, but no H.264, HEVC or AAC.** Linden Lab avoids this by *building
their own* Chromium with `proprietary_codecs=true` — we will not: it means
maintaining a Chromium build and funding the AVC/AAC patent pools.

Consequences, stated plainly so nobody re-litigates them mid-implementation:

- **YouTube works** — it negotiates VP9/AV1 + Opus via `isTypeSupported`.
- **H.264/AAC *inside* a page does not** (Vimeo embeds, hand-rolled
  `<video src=".mp4">`). This is unfixable at the embedder level: Chromium
  decodes and composites `<video>` internally in the GPU process, there is **no
  embedder-supplied-codec hook** (NPAPI/PPAPI are gone; the Widevine CDM is
  decryption, not decoding; `CefMediaRouter` is Cast), and the JS-overlay trick
  (hide the element, composite our own player behind it) breaks on z-order,
  scroll, CSS transforms and cross-origin iframes. Accept the gap.
- **Direct video URLs never reach CEF at all** — they dispatch by MIME type to
  [[viewer-video-playback]], where the codec comes from the user's *system*
  decoders. That is the bulk of MoaP video, and it is why the split exists.

## The surface, and the traps

Put both engines behind **one `MediaBackend` trait** — MIME/URL + size in;
frames, input and PCM across the boundary — with CEF and GStreamer behind it on
desktop and room for the Android system WebView later (CEF has **no Android
support**; `android.webkit.WebView` offscreen via SurfaceTexture is a research
problem, not a solved one — do not design for it, just do not foreclose it).

Three things must not leak through that boundary:

- **Input must be portable.** Firestorm tunnels *raw native key blobs* (Win32
  `MSG`/wParam/lParam, SDL2 keysyms, NSEvent) to CEF — which is exactly why its
  Linux keyboard support needed downstream patches, and it is meaningless on
  Android. `CefKeyEvent` can be built portably from `windows_key_code` +
  `character` + `unmodified_character` + modifiers, so pass **keycode + text**
  and synthesise the event inside the backend. (Note `windows_key_code` is a
  Windows VK code even on Linux/macOS — one ~100-line table.) For IME, feed the
  *composed* text from Bevy's `Ime::Commit` as CHAR events; full in-browser IME
  composition is a later problem.
- **Zero-copy is an optimisation, never a requirement.** Ship the CPU path first
  (`OnPaint` gives BGRA + dirty rects; 8 surfaces at 512² and 30 fps is ~240
  MiB/s of `write_texture` — about 1 % of PCIe, and it is exactly what the
  reference viewer has shipped for a decade: Dullahan is a CPU memcpy into
  shared memory). The Linux DMA-BUF fast path is **not blocked upstream in
  CEF** — CEF hands over the dmabuf fds, modifier and format perfectly well.
  The gaps are Rust-side and ours to fix: cef-rs's import looks spec-incomplete
  (it omits `VkExternalMemoryImageCreateInfo` and handles a single plane only),
  and it needs `VK_EXT_image_drm_format_modifier`, which wgpu does not enable on
  the device Bevy creates (escape hatch: build the device ourselves via
  `RenderCreation::Manual`). CEF also wants `--use-angle=gl-egl`. So treat
  zero-copy as **deferred headroom pending fixes we could contribute**, not a
  dead end — and spike it before believing any of it.
- **Process model.** CEF re-execs our own binary as its subprocesses, so
  `cef_execute_process()` must run at the top of `main()`
  **before Bevy exists**; macOS additionally needs four helper `.app` bundles
  (cef-rs ships tooling for this). Use `external_message_pump` +
  `do_message_loop_work()` from a Bevy system — CEF cannot own the event loop,
  winit does.

  Run CEF **in-process**, i.e. *not* behind an SLPlugin-style helper of our own.
  Firestorm's helper process exists for crash isolation, but CEF is already
  multi-process internally (a renderer crash does not take the browser process
  with it), and that extra IPC hop is exactly what forces Firestorm onto the CPU
  shared-memory path. In-process is what makes zero-copy reachable at all. The
  trade is honest: a crash in the `libcef` browser process takes the viewer with
  it — mitigate by bounding the surface count, not by adding a hop.

Also: reuse Firestorm's **throttle**, which is what makes many media prims
survivable at all — a hard instance cap (8), interest-sorted priorities, and a
per-surface sleep time (100/50/25/**1** Hz) so an out-of-view browser runs at 1
fps and beyond the cap is killed outright, with a heartbeat watchdog to reap a
hung engine. And treat MoaP content as **hostile**: an isolated request context
per surface (a griefer's prim must not read the marketplace's cookies), no
JS-to-native bridge, no file access, and honour `MediaEntry`'s whitelist and
`first_click_interact`.

Costs to accept: the CEF payload is ~200–400 MB installed, and we inherit
Chromium's ~monthly security-update treadmill — unavoidable for anything that
renders attacker-supplied in-world URLs. Two further cautions for the spike:
CEF's accelerated path reports **no damage rects** (whole-surface re-imports),
and Linux OSR shared textures are **reported broken on NVIDIA** (GBM buffer
usage) even with the right ANGLE flags — which is another reason the CPU path,
not zero-copy, is the thing we promise.

Reference (Firestorm, read-only): `llplugin/`, `media_plugins/cef`,
`llmediactrl`, `llviewermedia`, `llpanelprimmediacontrols`, `llviewermediafocus`
(UV→texel with wrap + power-of-2 padding correction), `mime_types.xml`.

Builds on: `protocol-24` (`MediaEntry` / `ObjectMedia`, already decoded).

Deps: [[viewer-ui-widget-scaffold]] (the floaters), [[viewer-audio-backend]]
(page audio must reach the mixer to be muted, bussed and spatialised).

## Progress (2026-07-22)

The CEF engine and both interaction surfaces are implemented and committed:

- **`sl-cef` crate**: the `MediaBackend` / `MediaSurface` traits and the CEF
  implementation — windowless browsers, external message pump, CPU BGRA
  `on_paint` path, per-surface isolated request contexts, portable input
  (Windows VK codes + committed text via `vk`), popup suppression, and the
  `sl-cef-helper` subprocess binary (`browser_subprocess_path`, so the viewer
  binary is never re-executed — same architecture as planned, cleaner entry).
  The prebuilt CEF download is pinned to `.cef/` via `.cargo/config.toml`
  (`CEF_PATH`), and binaries get an `$ORIGIN` rpath.
- **UI widget** (`browser_widget`): the `LLMediaCtrl` equivalent — a
  surface-backed image node with click-to-focus, pointer / wheel / keyboard
  routing (Tab deliberately stays with UI focus-nav; Escape releases), lazy
  surface creation at laid-out size, live in the gallery via a data-URL
  specimen.
- **Web floater** (`web_floater`, Content ▸ Web Browser): back / forward /
  stop-or-reload / address bar / secure lock / open-external / status +
  progress row, shared (trusted) request context.
- **Profile Web tab** renders the profile URL's page with the reference's
  load-time status line (`viewer-profile-web-tab-browser`, now done).
- **Media-on-a-prim** (`media_prim`): `MediaURL` version tracking →
  `RequestObjectMedia`, per-face entries, interest-ranked surface driver
  (cap 8, 30/15/5/1 fps tiers), face-material swap (unlit, original handle
  restored), reference focus model (first click focuses,
  `first_click_interact`, Escape releases, keyboard via the new
  `InputContext::Media`), `perms_interact` gating, white-list bounce-back.
- **Floating controls bar** (`media_controls`): back / forward / home /
  stop-or-reload / mute / zoom-unzoom / open-external / URL field with
  white-list check + `ObjectMediaNavigate` broadcast, progress read-out,
  secure lock, mini-controls and `perms_control` gating, projected above the
  face and hidden after ~3 s idle.

Still open here: **page audio into the mixer** (blocked on
[[viewer-audio-backend]]; until then CEF plays audio directly and the bar's
mute toggle is the only control), **spatialisation**, the **GStreamer
direct-video split** ([[viewer-video-playback]]), zero-copy (deferred
headroom, as designed), cursor-shape adoption over media, and in-page IME
composition (committed text works).
