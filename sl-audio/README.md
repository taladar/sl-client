# sl-audio

Engine-agnostic audio mixer for Second Life / OpenSim viewers — the **one
mixer** every audio source feeds. No source opens its own audio device: in-world
sound effects, UI sounds, the parcel music stream, media-on-a-prim and page
audio, and voice all route through here, so mute, per-category volume,
spatialisation and the source cap are decided in one place. Routing everything
through a single mixer is what lets media-on-a-prim audio be genuinely
positional — something no Second Life viewer manages today.

This crate is the foundation the `viewer-audio-backend` roadmap task builds; the
per-source producers (in-world sounds, UI sounds, the GStreamer / CEF hand-offs,
voice) are separate tasks that plug into it.

## What is here

- **Buses** (`Bus` / `BusLevel`) — the fixed volume categories (master, SFX,
  ambient, UI, music, media, voice) and their gain/mute state. Mute retains the
  previous level and never stops a source, so looped and attached SL sounds stay
  time-coherent while silent. These are exactly the categories the volume panel
  exposes.
- **Listener** (`Listener` / `EarMode`) — where the ears are (camera vs. avatar
  head) and the math turning a world position into the listener-relative offset
  the spatial node wants.
- **Source cap** (`SoundPriority` / `VoicePool`) — SL asks for more simultaneous
  sounds than any device wants; the pool caps voices and evicts by priority
  (tier, loudness, distance).
- **Clip decode** (`decode_clip` / `DecodedClip` / `ClipCache`) — decode short
  SL sounds once (Ogg Vorbis / WAV), resampled to the device rate, cached by
  asset id; never per trigger, never through a GStreamer pipeline.
- **Mixer** (`Mixer`) — the firewheel-backed graph: a master volume node feeding
  the device, one volume node per category feeding the master, 2-D and spatial
  clip playback, and a realtime-safe pushed-PCM path for streams whose clock is
  not the sound card's.
- **Pushed PCM** (`PushProducer` / `PushStreamConfig`) — a realtime-safe
  resampling channel plus a custom firewheel node, for GStreamer `appsink`, CEF
  `OnAudioStreamPacket` (planar) and decoded Opus.
- **`AudioMixer`** — the trait `Mixer` implements, so the backend stays a swap
  rather than a rewrite.

## Backend

Built on the engine-agnostic [`firewheel`](https://crates.io/crates/firewheel)
audio graph (its `volume`, `spatial_basic` and `sampler` nodes), with
`symphonium` for clip decode and
[`fixed-resample`](https://crates.io/crates/fixed-resample) for the pushed-PCM
channel. There is no Bevy dependency; the viewer wires this crate to the ECS
with its own thin glue.

## Smoke test

`cargo run -p sl-audio --example play_test --release` opens the default output
device and, over a few seconds, plays a clip, a panning spatial clip, and a
pushed sine tone that is muted (still running, silently) and then restored. It
is an ears-on check of the paths the unit tests cannot cover.
