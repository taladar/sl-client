---
id: viewer-parcel-audio-bar-backing-unskinned
title: The parcel audio bar's backing is a colour no skin can reach
topic: viewer
status: bugs
origin: viewer-vintage-skin second pass (2026-09-26)
refs: [viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The parcel streaming-audio cluster (`sl-viewer-audio/src/parcel_audio.rs`,
`spawn_parcel_audio_cluster`) paints its backing strip from a Rust constant
(`BAR_BACKGROUND`) and carries no skin class, so it stays a dark navy strip in
every skin — visibly out of place on Vintage's grey chrome. Its buttons were
already `.sk-action-button` and are skinned; its title label is a text role.

## Done when

The strip wears a skin class (or an existing chrome role such as the toolbar
bar's), so a skin decides its colour, and the flat skins look as they do now.
