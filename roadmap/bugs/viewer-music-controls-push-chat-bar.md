---
id: viewer-music-controls-push-chat-bar
title: Parcel music controls push the nearby chat bar up when they appear
topic: viewer
status: bugs
origin: user report (2026-08-01, aditi live testing)
refs: [viewer-streaming-audio, viewer-ui-bottom-toolbar]
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

When the parcel's streaming-music control cluster appears (the parcel has a
music-stream URL), the **nearby chat bar** at the bottom of the screen is
**pushed upward** instead of staying anchored just above the button bar.
Toggling the music row's visibility visibly shifts the chat bar's vertical
position.

## Cause

Both controls are children of the same bottom-area **upper** stack, which is a
full-width **column** (`bottom_toolbar.rs`, `BottomArea::upper` — see
`spawn_bottom_toolbar`). The nearby chat bar spawns into it
(`nearby_chat_bar.rs` `spawn_nearby_chat_bar`, `ChildOf(area.upper)`) and the
parcel-audio cluster spawns into it too (`parcel_audio.rs`
`spawn_parcel_audio_bar`, `ChildOf(area.upper)`), hidden until the parcel has a
stream. Because the column stacks its children vertically, making the music row
visible grows the column and reflows the chat bar's position — the two fight
for the same vertical slot rather than the music row sitting in a fixed place
relative to the chat bar.

## Where to look / fix direction

- Decide the intended stacking (reference Firestorm: the nearby-chat bar rides
  directly above the button bar; the nearby-media / music row sits above the
  chat bar). Fix the **child order** (or an explicit order/flex arrangement) in
  the `upper` column so the chat bar keeps its slot just above the button bar
  and the music row lands **above** it — appearing/disappearing without moving
  the chat bar.
- The spawn order is non-deterministic-ish: both are one-shot `Update` systems
  guarded by a `Local<bool>`, so whichever runs first is the first (top) child.
  Anchor the order deliberately rather than relying on spawn timing.
- Confirm on aditi (a parcel with a music stream) and in the gallery / bottom-
  toolbar harness that the chat bar stays put as the music row toggles.
