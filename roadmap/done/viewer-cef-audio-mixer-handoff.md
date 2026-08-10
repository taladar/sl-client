---
id: viewer-cef-audio-mixer-handoff
title: CEF page audio into the shared mixer (OnAudioStreamPacket)
topic: viewer
status: done
origin: follow-up filed while implementing viewer-media-prim-browser with
  the interim direct audio path (2026-07-22)
blocked_by: [viewer-audio-backend]
refs: [viewer-media-prim-browser, viewer-volume-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Route the embedded browser's page audio through the shared mixer once
[[viewer-audio-backend]] exists. Today CEF plays audio directly to the OS
device and the only host-side control is the per-surface mute — exactly
the Firestorm limitation (its Dullahan wrapper hides `CefAudioHandler`,
forcing the PulseAudio sink-input hack) that going direct to the `cef`
crate was meant to escape.

The work, in `sl-cef`'s `chromium.rs` behind the existing
`sl-media` boundary:

- Implement **`CefAudioHandler`** on the browser client:
  `OnAudioStreamStarted` (channel layout + sample rate),
  `OnAudioStreamPacket` (**planar** f32 PCM — de-interleave before
  pushing), `OnAudioStreamStopped` / `OnAudioStreamError`. Enable with
  `audio_muted=false` plus the handler so CEF stops opening its own
  output stream.
- Push each surface's PCM through the mixer's resampling channel (CEF's
  clock is neither the sound card's nor GStreamer's) onto the **media
  bus**:
  - media-on-a-prim surfaces **spatialised at the prim** — genuinely
    positional browser audio, which no SL viewer manages today;
  - UI surfaces (web floater, login page, profile web tab) stereo, 2-D.
- Re-express `MediaSurface::set_muted` / the future volume as gains on
  that per-surface mixer input rather than CEF's own mute, so the
  [[viewer-volume-panel]]'s media bus governs pages and videos alike.

Watch-outs recorded now so they are not rediscovered: packets arrive on
CEF's audio thread (the handler must only push into the realtime-safe
channel, never touch Bevy state), and a surface can start/stop its stream
many times per page (each `OnAudioStreamStarted` may change sample rate /
channels — rebuild the channel, do not assume one format per surface).

Builds on: `sl-cef` (handler + surface plumbing), `media_prim.rs` (the
prim position feeding the spatialiser).

## Done (2026-08-10)

Implemented on top of the same `sl_media::AudioSink` boundary as the GStreamer
hand-off ([[viewer-gst-audio-mixer-handoff]]).

- **`CefAudioHandler`** (`OsrAudioHandler` in `sl-cef/src/chromium.rs`, via the
  `cef` crate's `wrap_audio_handler!`): `on_audio_stream_started` reads the rate
  and channel count and `configure`s the sink; `on_audio_stream_packet`
  reconstructs the planar f32 channel slices and pushes them; `stopped` /
  `error` close the input. The handler is returned from the client's
  `audio_handler`, so installing it makes CEF deliver PCM here instead of
  opening its own output.
- **Threading watch-out honoured:** the handler holds only an
  `Arc<Mutex<CefAudioState>>` (never the `!Send` `SurfaceShared`), because
  `on_audio_stream_packet` fires on CEF's audio thread; it does nothing but hand
  the samples to the realtime-safe mixer channel. Each `on_audio_stream_started`
  re-`configure`s, so a mid-page rate/channel change rebuilds the input (no
  one-format-per-surface assumption).
- **Routing:** page audio goes to the **media** bus — prim surfaces spatialised
  at the prim (`spatial = true` in `MediaSurfaces::create_kind`), UI surfaces
  (web floater, login, profile web tab) 2-D. The viewer's `MixerSink` normalises
  CEF's planar PCM to stereo (mono duplicated, >2ch keeps the front two).
- **Mute:** `MediaSurface::set_muted` drives the sink's mute (the page keeps
  delivering PCM, silenced at the mixer input) once a sink is attached, and
  CEF's own device mute is cleared on attach — the media bus and any future
  per-surface volume govern it, not `CefAudioHandler`-less muting. Without a
  sink, `set_muted` falls back to CEF's device mute.

Live-testable on a media prim / UI browser panel with a page that plays audio;
the stereo-normalisation is unit-tested in `media_audio.rs`.
