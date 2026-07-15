---
id: viewer-video-playback
title: Video playback backend (a second media engine, not the browser)
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-audio-backend]
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

Deps: [[viewer-audio-backend]] (the video's audio must go through the mixer).
