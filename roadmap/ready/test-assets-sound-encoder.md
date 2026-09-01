---
id: test-assets-sound-encoder
title: A synthetic sound asset the fake grid can serve
topic: test
status: ready
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 3
refs: [test-shared-test-assets, test-fake-grid-builtin-sounds]
---

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
