---
id: viewer-audio-backend
title: Audio backend — device, decode, listener & mixer
topic: viewer
status: ready
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The audio foundation every other audio task sits on. Today there is **nothing**:
no audio crate anywhere in the workspace, and no sound decoder at all.
`sl-asset` fetches and caches a sound as an *opaque blob* (the asset classes are
there — `AssetType::Sound` and `AssetType::SoundWav` in `sl-proto`) but nobody
turns those bytes into samples, and the viewer never opens an audio device.

This task owns the **one mixer** that everything else feeds:

| Source | Feeds in as | Bus |
| --- | --- | --- |
| In-world SFX ([[viewer-in-world-sounds]]) | decoded Ogg clips | SFX, **spatial** |
| UI sounds ([[viewer-ui-sound-effects]]) | decoded clips | UI, 2-D |
| Parcel stream ([[viewer-streaming-audio]]) | GStreamer PCM | music, **stereo** |
| Video + page audio ([[viewer-video-playback]], [[viewer-media-prim-browser]]) | GStreamer / CEF PCM | media, **spatial at the prim** |
| Voice ([[viewer-voice-audio]]) | decoded Opus | voice, **stereo** (see below) |

Getting that right means **no source may open its own audio device** — which is
exactly Firestorm's mistake: its browser audio bypasses the viewer entirely and
can only be attenuated by a PulseAudio sink-input hack, with `setPan()` left an
empty TODO. Route everything through one mixer and media-on-a-prim audio becomes
genuinely positional, which no SL viewer manages today.

## Mixer choice (surveyed 2026-07)

The acid test is **pushed PCM** (GStreamer `appsink`, CEF
`OnAudioStreamPacket`, decoded Opus) plus, decisively, **microphone capture in
the same audio graph as the output** — because echo cancellation needs the
post-mix signal and the mic on one clock (see [[viewer-voice-audio]]).

- **`bevy_seedling` / Firewheel — recommended.** The only crate with all of:
  native voice pools with priority eviction (`SamplerPool` + `PoolSize` +
  `SamplePriority`), real buses and sends, HRTF, a custom-node API, and **mic
  capture as a graph node** (`AudioGraphInput`), so an AEC node can see the mic
  and the final mix in one `process()` call. It also hands us
  `input_to_output_latency_seconds`, the delay hint AEC3 wants.
- **`bevy_kira_audio` / Kira — the fallback.** Published for Bevy 0.19 today,
  custom `Sound` trait for pushed PCM, tracks as buses. But it has **no audio
  input at all**, so the mic becomes a separate cpal stream on a different
  device clock and we own drift correction and delay estimation by hand — the
  classic AEC foot-gun. It also has no HRTF.
- **`bevy_audio` — out.** It *can* take pushed PCM (a `Decodable` over a ring
  buffer), but it has no bus graph, no voice pool, no priority eviction, no
  capture. Bevy's own audio working group calls rodio "unsuitable for game use".
- `oddio` / `bevy_oddio`: dead (2023). FMOD / Wwise: **ruled out** — a
  proprietary SDK you must fetch by hand to even build (Firestorm's actual
  situation) breaks distro packaging and contributor CI.

**Risk to accept up front:** `bevy_seedling` for Bevy 0.19 is **not on
crates.io** — master only, with Firewheel pinned to a git commit — and the Bevy
WG flags "nontrivial UB" in Firewheel. Firewheel is nonetheless where Bevy's own
audio is heading. Prefer depending on the **engine-agnostic core** with our own
thin Bevy glue, so we are never waiting on a wrapper crate to adopt a new Bevy,
and keep the mixer behind our own trait so Kira remains a swap rather than a
rewrite.

## The rest of the work

- **Pushed PCM.** Firewheel deleted its stream nodes (a rework is open), so the
  push node is ours — but the primitive is right there:
  `fixed_resample::ResamplingChannel`, a realtime-safe SPSC with automatic
  sample-rate conversion and under/overflow correction, which is what Firewheel
  itself uses for its mic path. **Every external source must go through it** —
  the sound card's clock, GStreamer's clock and CEF's clock are all different,
  and this is where that drift dies. (CEF's PCM is planar; de-interleave first.)
- **Clip decode.** SL sounds are short Ogg Vorbis. Decode **once** on the
  asset-load path into a cached sample keyed by asset id (symphonia; `lewton` is
  dead since 2021) — never per trigger. Do **not** spin a GStreamer pipeline per
  sound effect: that is milliseconds of latency and hundreds of KB for a
  footstep, and SL fires dozens at once.
- **Buses and mute.** One volume node per category (master / SFX / UI / ambient
  / media / music / voice — see [[viewer-volume-panel]]). Mute must retain the
  previous level, and must **not** stop the source: SL's looped and attached
  sounds have to stay time-coherent.
- **Source cap.** SL asks for more simultaneous sounds than any device wants —
  cap the pool and evict by priority (distance, loudness, attachment).
- **The listener** — camera vs. avatar head is a reference-viewer preference,
  and it changes how everything sounds.
- Device selection and hot-plug, and how the audio thread meets Bevy's schedule
  without stalling a frame.

Reference (Firestorm, read-only): `llaudio/llaudioengine_*`, `lllistener_*`,
`llaudiodecodemgr` — and `media_plugins/cef/linux_volume_catcher.cpp` as the
cautionary tale.

Builds on: `sl-asset` fetch + cache and the existing `AssetType::Sound` classes.
Supersedes the MVP "no sound" non-goal.
