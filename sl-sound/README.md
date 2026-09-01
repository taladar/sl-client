# sl-sound

Ogg Vorbis encoding of Second Life / OpenSim **sound assets**.

`AssetType::Sound` is Ogg Vorbis on both grids: a short, usually mono clip a
simulator serves over `ViewerAsset` and a viewer plays through its mixer. This
crate writes one.

## Why encoding lives here and decoding does not

Decoding a sound is the mixer's job — `sl-audio`'s `decode_clip` hands the
bytes to `symphonium` and gets back a sample resource already resampled to the
output device's rate, which is meaningless away from a mixer. Encoding has no
engine ties at all: samples in, asset bytes out. Keeping it in its own crate is
what lets a fixture generator (`sl-test-assets`) and a headless fake grid write
a sound without linking an audio device, while the viewer's eventual sound
upload still has one implementation to call.

## What it writes

`encode_sl_sound` is the shape of the reference viewer's `encode_vorbis_file`
(`indra/llaudio/llvorbisencode.cpp`): mono, 44.1 kHz, quality `0.05` — the
level Linden settled on in SL-52913 as "good enough" at a low bitrate — and no
longer than the grid's 30-second clip limit. `encode_vorbis` is the same
encoder without the upload rules, for a fixture that wants another sample rate
or a stereo signal.

The bytes come out of the aoTuV/Lancer-patched libvorbis through the
`vorbis_rs` safe bindings, built from source, so what a fixture serves is what
a real viewer expects rather than something only our own decoder accepts.

## Determinism

The Ogg stream serial is a constant rather than the random one the Ogg
specification suggests: a serial only exists to tell two *chained* logical
bitstreams apart, and a sound asset is always exactly one. In exchange, the
same samples always encode to the same bytes — which is what lets a fixture
asset be compared against a recorded one and a seeded run repeat itself.

```text
let tone: Vec<f32> = /* one second of 440 Hz */;
let asset: Vec<u8> = sl_sound::encode_sl_sound(&tone)?;
// `asset` starts "OggS" and decodes through symphonium, lewton or a viewer.
```
