---
id: test-assets-gesture-codec
title: Read and write a gesture asset (LLMultiGesture)
topic: test
status: ready
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 3
refs: [viewer-static-asset-library, viewer-gesture-management-ui]
---

Context: [context/testing.md](../context/testing.md).

[[viewer-static-asset-library]] vendored 76 Linden `.gesture` assets and
the viewer now serves them from disk — but nothing in the workspace can
*read* one. `AssetType::Gesture` appears only as an inventory-item class
and a group-notice icon; there is no parser and no writer, which is why
the vendored-content test can check no more than "the first line is `2`".

The format is `LLMultiGesture::deserialize`
(`indra/llcharacter/llmultigesture.cpp`): a version line, the trigger key
and modifier mask, the trigger string, the replacement string, then a
counted list of steps — `animation` (play/stop, name, asset id), `sound`,
`chat`, `wait` (time and/or "wait for animations") — and a trailing NUL.
It is line-oriented like the wearable format, so it belongs beside
`sl_avatar::WearableAsset` in shape if not in crate.

Wanted:

- a decoder producing a typed `Gesture { key, mask, trigger, replacement,
  steps }`, tested against the vendored library — 76 real assets is an
  unusually good corpus, and the vendored-content test should be upgraded
  from "starts with 2" to "parses" once it exists;
- an encoder, so `sl-test-assets` can write a fixture gesture whose steps
  a test chose;
- the fixture itself: a gesture that plays the chest-twist animation and
  says a known line, so the gesture path has an end-to-end oracle.

This unblocks [[viewer-gesture-management-ui]], which cannot list a
gesture's steps without reading one.

Acceptance: every vendored `.gesture` parses; a written gesture round
trips; the vendored-content test asserts parsing rather than a version
line.
