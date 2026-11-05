---
id: viewer-video-playback
title: Video playback backend (a second media engine, not the browser)
topic: viewer
status: done
origin: user request (2026-07)
refs: [viewer-audio-backend, viewer-media-zero-copy-frames, viewer-media-served-mime-probe]
---

Context: [context/viewer.md](../context/viewer.md).

A dedicated video player, separate from the embedded browser — because the
reference viewer needs one and so will we. Firestorm does not play video through
CEF: it ships **`media_plugin_libvlc`** (and, on Linux,
**`media_plugin_gstreamer10`**) alongside `media_plugin_cef`, and
`mime_types.xml` dispatches `video/*` and `audio/*` to the video plugin while
HTML goes to the browser. Our primary platform already uses GStreamer in the
reference viewer — this posture is not exotic.

The reason is codecs. Stock prebuilt CEF is open-codec-only (VP8 / VP9 / AV1 /
Opus / Vorbis), and we have ruled out the alternative on principle: Linden Lab
builds *their own* Chromium with `proprietary_codecs=true`, which means
maintaining a Chromium build and funding the AVC/AAC patent pools. So direct
`.mp4` and HLS URLs — the bulk of media-on-a-prim video — need a second engine.

## Backend: GStreamer, with system decoders only

`gstreamer-rs` (bindings MIT/Apache-2.0; GStreamer core is LGPL, so dynamic
linking keeps us clean). `playbin3` / `uridecodebin3` plus `adaptivedemux2`
gives HLS and DASH — the thing we would otherwise be reinventing.

**We ship no encumbered decoder.** The Firefox/Fedora posture: the application
carries no H.264/AAC implementation and the *system* provides one. GStreamer's
OS-backed decoder plugins are thin wrappers containing no codec code:

| Platform | Plugin | Whose licensed decoder |
| --- | --- | --- |
| Linux | `vah264dec` (VA-API), or the user's own `gst-libav` | GPU driver / user's distro |
| Windows | `d3d11h264dec` (DXVA), Media Foundation | Microsoft's OS decoder |
| macOS | `vtdec_hw` (VideoToolbox), `atdec` (AAC) | Apple's OS decoder |
| Android | `amcviddec-*` (MediaCodec) | The device's OEM decoder |

So we deliberately **do not ship `gst-libav` or gst-plugins-ugly**. What that
costs: Linux users with neither VA-API H.264 nor `gst-libav` installed get no
H.264, and some long-dead formats in Firestorm's `mime_types.xml` (FLV, WMV,
RealMedia) simply will not play. Both are acceptable — but they must **fail
loudly and usefully**: GStreamer posts `missing-plugin` bus messages
(`gstreamer_pbutils::MissingPluginMessage`) carrying a human-readable
description, so the prim should say *"needs an H.264 decoder — install
gst-libav"* rather than showing a black square. Enumerate the available decoders
at startup too (`GstRegistry`), because on a source distro the answer varies per
machine.

## Frames, audio, and sync

- **Frames.** CPU baseline first: `appsink` → NV12 → `queue.write_texture` (four
  720p streams at 30 fps is ~158 MiB/s — fine). Zero-copy (VA-API → DMA-BUF →
  wgpu external-memory import, and the D3D11 / IOSurface equivalents) is a
  **per-platform optimisation behind the same `Frame` enum
  [[viewer-media-prim-browser]] defines**, never a requirement. The
  `lumina-video` crate is prior art for the Linux path — read it, don't depend
  on it yet.
- **Audio.** Pull PCM out via a second `appsink` (f32) and hand it to
  [[viewer-audio-backend]]'s mixer, so a video's sound lands on the media bus,
  obeys mute, and is **spatialised at the prim**. Firestorm cannot do this — its
  plugin plays straight to the OS device and `setPan()` is an empty stub — so
  this is a place we beat the reference. The same PCM path also serves
  [[viewer-streaming-audio]] (a parcel radio stream is just Icecast/HLS, which
  GStreamer already handles and the pure-Rust audio crates do not).
- **A/V sync** is the open question this creates: if GStreamer decodes but does
  not output (no `autoaudiosink`, because our mixer owns the device), something
  must keep the video texture in step with audio the mixer now owns. Settle this
  early — clock ownership is not a detail you can retrofit.

## The shared surface

The prim-face texture, the MIME dispatch that chooses browser-vs-video, the
UV→pixel input injection, and the per-`MediaEntry` state are **shared with**
[[viewer-media-prim-browser]] — one `MediaBackend` trait, two implementations.
Whichever task is worked first builds that surface; the second plugs into it.
Do not build it twice. The same throttle applies (instance cap, interest-sorted
priority, sleep-time for out-of-view surfaces).

Also here: play / pause / seek / volume, the per-prim media-controls overlay,
and parcel media video.

Reference (Firestorm, read-only): `media_plugins/media_plugin_libvlc`,
`media_plugins/gstreamer10`, `llviewermedia` (the MIME → plugin dispatch),
`llpanelprimmediacontrols`, `mime_types.xml`.

Builds on: `protocol-24` media-on-a-prim (`MediaEntry` / `ObjectMedia` are
already decoded; the viewer just never reads them).

Deps: [[viewer-audio-backend]] (the video's audio must go through the
mixer) — for the *mixer hand-off only*; the interim below unblocked
playback itself.

## Progress (2026-07-22)

The engine, the dispatch and the transport controls are implemented:

- **`sl-media` crate**: the `MediaBackend` / `MediaSurface` traits moved
  out of `sl-cef` (which re-exports them) into a shared crate, extended
  with the playback half — `PlaybackStatus` (state / position / duration /
  seekable / buffering) on `SurfaceStatus`, default-no-op `play` / `pause`
  / `seek` / `set_volume` methods, `loop_media` on `SurfaceConfig` — plus
  `classify_url`, the scheme/extension → web-vs-video dispatch table
  (unit-tested). Extension-based dispatch is a documented simplification
  of the reference's HTTP content-type probe; extension-less media URLs
  land in the browser.
- **`sl-gst` crate**: `GstMediaBackend` — one `playbin3` per surface with
  a `videoconvert ! appsink` BGRA video sink; frames are stride-stripped
  into the shared frame store on the streaming thread (unit-tested against
  `videotestsrc`) and mirrored by the existing engine-agnostic texture
  path. The bus pump defers pipeline actions out of the lock (a flushing
  seek can block on the appsink thread), holds/resumes on buffering, loops
  on EOS when `auto_loop`, and turns errors + `missing-plugin` messages
  into loud `load_error` text (unit-tested via a nonexistent file URI).
  A/V sync is GStreamer's own for now: both sinks follow the pipeline
  clock, audio to the system device (the same interim as CEF page audio).
- **Viewer**: the media engine holds both backends
  (`--disable-video-media` mirrors `--disable-web-media`);
  `media_prim` classifies each entry's URL and starts the surface on the
  right engine (re-dispatching when a server-side navigate crosses
  engines); the floating controls bar grows a **video mode** — play-pause,
  restart, a seek scrubber + position ∕ duration read-out (hidden for
  unseekable live streams), mute, zoom, open-external, with the stream
  title / decoder-gap error in the status slot. A focused video face
  deliberately leaves the keyboard with the world (nothing to type at).

Done since: the PCM hand-off ([[viewer-gst-audio-mixer-handoff]]) and legacy
whole-parcel media (2026-11-05, below). Split out as their own tasks: zero-copy
per-platform frame paths ([[viewer-media-zero-copy-frames]], already filed when
viewer-media-prim-browser closed) and a served-MIME probe
([[viewer-media-served-mime-probe]], only if extension dispatch proves
insufficient).

## Legacy whole-parcel media (2026-11-05)

`sl-viewer-world-view/src/parcel_media.rs`, the reference's
`LLViewerParcelMedia` + `LLViewerParcelMediaAutoPlay`:

- The agent parcel's media (properties, then any later `ParcelMediaUpdate`)
  plays on the engine its **declared MIME type** picks — `sl_media::
  classify_mime` / `classify_media`, after `mime_types.xml`: a known type wins,
  an empty one falls back to `classify_url` (where the reference probes), and
  `none/none` never plays. The served-MIME probe item above therefore only
  remains for prim media and for parcels with no type.
- Every face whose texture is the media texture wears the surface (a
  `ParcelMediaFace` component holds its own material, put back on stop or when
  the face changes texture); a face with live prim media keeps that. 2-D audio.
- Autoplay once per parcel entry: five seconds on it, agent under 0.1 m/s, the
  texture over 24 × 24 px on screen, behind the new `ParcelMediaAutoPlayEnable`
  preference (off, the reference default).
- Script commands (`command_steps`): stop / pause / unload / seek always; play,
  loop and a seek with nothing loaded only with `PermAllowScriptedMedia` on
  (Firestorm's default is off) — both new rows on the audio preferences tab.
- Leaving the parcel stops it unless the next names the same media; a changed
  URL / texture of the same type restarts it (`on_parcel_change`).
- *Play* click action toggles it (pause / resume / start); *open media* on a
  face showing it does the same, else opens the URL as before. The
  `ClickActionPlayUnavailable` notification is gone.
- Nearby Media lists it as *Parcel Streaming Media* under the stream.

Not ported: the first-use `ParcelCanPlayMedia` warning, the *ask* autoplay
mode (unsupported in the reference's own settings comment), the media filter
(`viewer-media-playback-policies`). `media_auto_scale` is not consulted — it
picks the page's render resolution there, not the face coverage, and the
surface here always fills the face.

Automation: `Probe::ParcelMedia` (`ParcelMediaReadout`: URL, type, texture,
engine, faces wearing it; `Viewer::parcel_media`, `sl-viewer-ctl
parcel-media`), the server mirrors `SimSession::send_parcel_media_update` /
`send_parcel_media_command`, and a `StageBuilder::video_media` switch (stage
viewers now run without GStreamer unless asked, on both backends). The stage
`e2e_parcel_media` has the fake grid state the media on the stock plywood and
send a script's play: refused while `PermAllowScriptedMedia` is off, playing on
the video engine and on the plywood faces once on, listed as *Parcel Streaming
Media (playing)* in Nearby Media, and off every face after the script's stop.

Tests: `cargo test --release -p sl-viewer-world-view parcel_media` (the pure
decisions plus an app test through the real GStreamer engine: a face wearing
the texture swaps to the media material, another keeps its own, stop puts it
back), `-p sl-media` (the MIME dispatch), and the viewer's
`a_left_click_on_a_prim_does_what_the_prim_says`.
