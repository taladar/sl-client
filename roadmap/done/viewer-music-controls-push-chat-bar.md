---
id: viewer-music-controls-push-chat-bar
title: Parcel music controls push the nearby chat bar up when they appear
topic: viewer
status: done
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

## Fix

The two controls do not overlap horizontally (the chat bar takes the leading
half of the screen, the music cluster the trailing side), so the right answer
was to put them **side by side on one row** directly on top of the button bar,
not stacked. `spawn_bottom_toolbar` (`bottom_toolbar.rs`) now builds the
bottom-area *upper* region as a **row** whose two children are fixed 50%-wide,
bottom-aligned halves — a leading slot (`BottomArea::upper_leading`) and a
trailing slot (`BottomArea::upper_trailing`) — spawned in that fixed order so
which half a control lands in never depends on spawn timing. The nearby-chat
bar (`nearby_chat_bar.rs`) parents into `upper_leading` and fills it (the 50%
split is now the slot's, so its input width became 100% and the old
`BAR_WIDTH_FRACTION` is gone); the parcel-audio cluster (`parcel_audio.rs`)
parents into `upper_trailing`. Because each half is fixed-width and independent,
the music cluster appearing/disappearing (or being `display: none` off-stream)
never reflows the chat bar. RTL mirrors for free (both halves justify by
leading/trailing, resolved from `UiDirection`).

Covered by the `bottom_toolbar` test
`upper_row_splits_into_fixed_leading_and_trailing_halves` (one row,
deterministic leading-then-trailing order, fixed 50% widths, above the button
bar). Verified live on aditi at a parcel with a music stream: the
chat bar and music cluster share one line above the buttons and the chat bar
holds its place.
