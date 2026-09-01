---
id: test-assets-sound-encoder
title: A synthetic sound asset the fake grid can serve
topic: test
status: done
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 3
refs: [test-shared-test-assets, test-fake-grid-builtin-sounds]
---

Done (2026-09-01), in two pieces, because the encoder is not fixture work.

**`sl-sound`** — a new crate holding the Ogg Vorbis *encoder*:
`encode_sl_sound` (mono, 44.1 kHz, quality `0.05`, the 30-second clip limit —
exactly the reference viewer's `encode_vorbis_file` in
`indra/llaudio/llvorbisencode.cpp`) and `encode_vorbis` (the same encoder
without the upload rules, for a fixture that wants another rate or two
channels). The bytes come from the aoTuV/Lancer-patched libvorbis through the
`vorbis_rs` safe bindings, built from source — the crate the task asked for in
preference to a hand-rolled framer, so what a fixture serves is a real Ogg
Vorbis stream rather than one only our own decoder accepts.

It is a crate of its own rather than a `sl-audio` feature because `sl-audio`
pulls firewheel and cpal: a headless fake grid must be able to write a sound
without linking an audio device. Encoding also has no engine ties, unlike
decoding, which must resample into the mixer's sample resource — so the two
halves genuinely belong apart. The viewer's eventual sound upload calls
`sl-sound` too.

The Ogg stream serial is a **constant** (`STREAM_SERIAL`), so the same samples
always encode to the same bytes. A serial exists only to tell chained logical
bitstreams apart and a sound asset is always exactly one; determinism is worth
more here than a random serial, and it drops the `getrandom` dependency.

**`sl-test-assets::sound`** — the fixtures: `tone_samples` / `tone` (a faded
sine at any rate), `marker_tone` (a quarter second at the grid's own settings)
and `tones::{LOW, MID, HIGH}` — 220 / 440 / 880 Hz, an octave apart, the audio
counterpart of `markers::{RED, GREEN, BLUE}`. The five-millisecond fade at each
end is load-bearing: a sine cut off mid-cycle ends on a step, which is a click
and a broadband smear in the one spectral peak the fixture is supposed to have.

Three tests in `sl-test-assets` decode a fixture back through **`symphonium`**
— the decoder `sl-audio`'s `decode_clip` plays clips with — and assert one
channel, the exact frame count, and a Goertzel peak at the tone's own frequency
at least eight times its neighbours'. Five more in `sl-sound` pin the encoder
itself through libvorbis' own decoder: the round trip, byte-for-byte
determinism, a stereo signal keeping both channels, the malformed-signal
refusals, and a non-grid sample rate. One in `sl-fake-grid` fetches a sound
asset over `ViewerAsset` under a fixture's own id and gets the bytes back.

**Not done here.** The fake grid can *serve* a sound but cannot yet *play* one:
`AttachedSound` / `SoundTrigger` are messages a simulator sends, and
`SimSession` has no sender for either, so the viewer's `world_sounds` path
still has no fixture driving it. Filed as
[[test-fake-grid-object-sounds]]. The six built-in UI sound ids that 404 on
arrival remain [[test-fake-grid-builtin-sounds]], which this unblocks.

Context: [context/testing.md](../context/testing.md).

`AssetType::Sound` is the one asset class the workspace can **decode** but
not **produce**. `sl-audio` pulls `symphonium` with the `ogg` + `vorbis`
features, so a viewer plays what a grid serves; nothing anywhere can write
a sound asset, so no fixture can hand it one.

That leaves every sound path untestable end to end: an in-world
`SoundTrigger` / `AttachedSound`, a parcel media stream's fallback, the
UI sounds of [[test-fake-grid-builtin-sounds]], and the collision and
gesture sounds a scripted object plays.

Add `sl-test-assets::sound`, beside `anim` and `mesh`: an **Ogg Vorbis**
encoder writing a short tone — the container Second Life's own built-in
sounds use, so what a fixture serves is what a viewer expects. A tone
rather than noise because a tone is *assertable*: a decode test can check
the fundamental frequency and the sample count, which white noise cannot
support.

Wanted, roughly:

- `tone(frequency_hz, seconds, sample_rate) -> Vec<u8>` — one channel;
- a `markers`-style set of fixed tones so two sounds in one test are
  distinguishable by pitch the way two textures are by colour;
- a round trip through `symphonium` in a unit test — the decoder is the
  contract, exactly as `sl-anim` is for the animation encoder and
  `sl-mesh` for the mesh one.

The encoder crate is the open question: `symphonium` decodes only. Either
add `vorbis-rs` / `ogg` + `vorbis_rs` as a `sl-test-assets` dependency, or
hand-write an Ogg page framer around a minimal Vorbis stream. Prefer the
crate unless its licence or dependency weight is a problem — the point of
the fixture is the *bytes being real*, and a hand-rolled encoder that
only our own decoder accepts would defeat it.

Acceptance: a fixture sound decodes through `symphonium` to the tone it
was written as; the fake grid can serve one under any asset id.
