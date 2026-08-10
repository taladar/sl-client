---
id: viewer-gst-audio-mixer-handoff
title: GStreamer audio into the shared mixer (parcel stream + video PCM)
topic: viewer
status: done
origin: follow-up filed while implementing viewer-video-playback /
  viewer-streaming-audio with the interim direct audio path (2026-07-22)
blocked_by: [viewer-audio-backend]
refs: [viewer-video-playback, viewer-streaming-audio, viewer-volume-panel]
---

Context: [context/viewer.md](../context/viewer.md).

Replace `sl-gst`'s interim direct audio output (`playbin3`'s default
`autoaudiosink`) with the shared mixer once [[viewer-audio-backend]]
exists. Two consumers, two buses:

- **Parcel radio stream** (`AudioStreamPlayer`,
  `sl-client-bevy-viewer/src/parcel_audio.rs`): swap the audio sink for an
  f32 `appsink` and push the PCM through the mixer's resampling channel
  (`fixed_resample::ResamplingChannel` — the stream's clock is not the
  sound card's) onto the **music bus, stereo, not spatialised**.
- **Media-on-a-prim video** (`GstMediaSurface` in `sl-gst/src/surface.rs`):
  same `appsink` hand-off, but onto the **media bus, spatialised at the
  prim** — the place we beat the reference viewer, whose plugin plays
  straight to the OS device with `setPan()` an empty stub.

The part that must be settled first (flagged in the original task): **A/V
sync and clock ownership**. Today both of GStreamer's sinks follow the
pipeline clock, which solves sync by accident. Once the mixer owns the
audio device, something must keep the video `appsink`'s frame delivery in
step with audio the mixer now schedules — either slave the pipeline clock
to the mixer's device clock, or run the audio `appsink` `sync=false` and
let the resampling channel's drift correction absorb the difference.
Decide before wiring, not after.

Volume / mute plumbing stays as-is externally: the per-surface
`set_volume` / `set_muted` and the persisted `MusicStreamVolume` setting
simply start driving mixer-bus gains instead of `playbin` properties (the
[[viewer-volume-panel]] reads the same buses).

Builds on: `sl-gst` (both players), `parcel_audio.rs`, `media_prim.rs` —
all shipped with the interim path and an API already shaped for this swap.

Cleanup when this lands: `parcel_audio.rs`'s cluster has its **own** stream
volume slider + mute (driving `playbin` via the interim path). Once the parcel
stream routes through the mixer's **music** bus, remove those in favour of the
[[viewer-volume-panel]] music-bus control (or make them drive the bus), so there
is one volume for the stream, not two in series.

## Done (2026-08-10)

The hand-off is an engine-agnostic **`sl_media::AudioSink`** trait (configure /
push interleaved / push planar / set-muted / stopped) plus a
`MediaSurface::set_audio_sink`. GStreamer implements the push side in a new
`sl-gst/src/audio_sink.rs`: an
`audioconvert ! capsfilter(native-endian F32, stereo) ! appsink` bin set as
`playbin3`'s `audio-sink`, whose callback reads the source rate from the caps
(re-`configure`s only on a change) and pushes stereo f32.

- **A/V sync / clock ownership (settled before wiring):** the audio `appsink`
  runs `sync=true`, so GStreamer paces it to the *pipeline* clock exactly like
  the video sink — audio and video stay in step by construction. The only new
  clock is the device's, and the mixer's `fixed_resample` channel (every push
  crosses it) is the pipeline↔device drift corrector. `sync=false` was rejected
  because a file decoder running ahead of realtime would flood the channel.
- **Parcel radio** (`AudioStreamPlayer`): music bus, stereo, 2-D. The player
  builds the `appsink` bin only when a sink is attached, else keeps the interim
  `autoaudiosink` (standalone / no-mixer). `playbin` volume/mute stay at unity —
  the bus owns them.
- **Prim video** (`GstMediaSurface`): media bus, **spatialised at the prim**.
  The surface always routes to the `appsink` (the viewer attaches the sink right
  after creation); `media_prim`'s `place_media_audio` feeds the prim-face world
  position each frame.
- **Viewer bridge** (`media_audio.rs`): `MixerSink` (the `AudioSink`; normalises
  to stereo, feeds a mixer input, realtime-safe for the streaming threads) +
  `MixerStream` (the viewer-thread handle: (re)opens the mixer input on a format
  change, closes on stop, keeps a spatial input on its prim). A format change is
  a producer swap the viewer performs — the shape the mixer already uses for a
  device hot-plug.
- **Cleanup done:** the parcel bar's inline volume slider + mute now drive the
  **music bus** settings (`music_volume` / `music_mute`, the same the volume
  panel edits) rather than `playbin`; the redundant `MusicStreamVolume` setting
  is gone. One volume for the stream, not two in series.

Live-testable on OpenSim/aditi (parcel radio + a media prim with a video URL);
unit tests cover the stereo normalisation and the sink state transitions.
