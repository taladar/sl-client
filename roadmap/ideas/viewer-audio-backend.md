---
id: viewer-audio-backend
title: Audio backend — device, decode, listener & mixer
topic: viewer
status: ideas
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The audio foundation every other audio task sits on. Today there is **nothing**:
no audio crate anywhere in the workspace, and no sound decoder at all.
`sl-asset` fetches and caches a sound as an *opaque blob* (the asset classes are
there — `AssetType::Sound` and `AssetType::SoundWav` in `sl-proto`) but nobody
turns those bytes into samples, and the viewer never opens an audio device.

**Choose a third-party backend — do not write a decoder or a mixer.** The first
fleshing-out step is a survey against what SL actually needs (many concurrent
3-D sources, low trigger latency, Ogg Vorbis decode, and a long-running stream
for the parcel player): candidates are `rodio`, `kira`, `symphonia` + `cpal`,
and a `gstreamer` / `libvlc` binding for the awkward streaming codecs. Bevy's
own `bevy_audio` is already present in the default features and must be
evaluated honestly rather than assumed adequate — spatialisation, source count
and streaming are where it tends to fall short. Note that [[viewer-voice-audio]]
also needs *capture* and a WebRTC-friendly path, so the choice should not paint
that into a corner.

Then integrate it: device selection and hot-plug, decoding fetched sound assets
(Ogg Vorbis; keep the decoded PCM cached — SL replays the same clips
constantly), the **listener** and where it sits (camera vs. avatar head is a
reference-viewer preference, and it changes how everything sounds), the mixer
with a source cap, priority and eviction, master and per-category volumes plus
mute, and how the audio thread meets Bevy's schedule without stalling a frame.

Consumed by [[viewer-in-world-sounds]] (3-D positional),
[[viewer-ui-sound-effects]] (the 2-D bus), [[viewer-streaming-audio]] (the
parcel stream) and [[viewer-voice-audio]].

Reference (Firestorm, read-only): `llaudio/llaudioengine_*`, `lllistener_*`,
`llaudiodecodemgr`.

Builds on: `sl-asset` fetch + cache and the existing `AssetType::Sound` classes.
Supersedes the MVP "no sound" non-goal.
