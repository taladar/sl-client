---
id: viewer-cef-audio-mixer-handoff
title: CEF page audio into the shared mixer (OnAudioStreamPacket)
topic: viewer
status: blocked
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
