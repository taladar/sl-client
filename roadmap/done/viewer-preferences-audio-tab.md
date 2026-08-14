---
id: viewer-preferences-audio-tab
title: Preferences — audio tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The **audio** tab of the preferences floater ([[viewer-preferences-floater]]):
the master and per-bus volumes (ambient, sound effects, UI, media, voice,
streaming) and the mute-on-focus-loss and related audio toggles — each control
bound to the typed settings store through the floater's binding.

The actual audio backend is a separate, out-of-scope concern; this tab owns the
settings surface only.

Reference (Firestorm, read-only): `llfloaterpreference*` (the audio panel).

Builds on: [[viewer-preferences-floater]].

## Done

New viewer module **`src/preferences_audio.rs`** (`TAB_ID = "audio"`, tab
label "Sound & media", slotted between graphics and world-UI — the
reference strip order). The tab registers **no settings of its own**; every
row binds a key owned and applied by its feature module:

- **Volumes**: per `Bus::ALL` (master, sfx, ambient, UI, music, media,
  voice) a 0–1 slider and a mute checkbox bound to the volume panel's
  existing `{bus}_volume` / `{bus}_mute` keys — one source of truth with
  the bottom-bar cluster, quick prefs and the parcel-stream bar.
- **Behaviour**: three new settings, each with a real consumer wired in
  its owning module:
  - `MuteWhenMinimized` (Bool, off; `volume_panel.rs`) — silences the
    **master bus mixer-side** while the window is unfocused, inside
    `apply_volume_settings_to_mixer`. The stored `master_mute` is never
    written, so the bar's mute glyph stays put and refocus restores the
    exact level (mute retains bus gain). Deliberate deviation: focus loss,
    not only minimise — Wayland exposes no reliable minimised signal.
  - `MediaSoundsEarLocation` (U32, 0; `audio.rs`) — a camera / avatar
    combo; `drive_audio` now swaps the listener **position** to the
    body-root anchor's current-frame `Transform` (the `own_avatar_pose`
    idiom, not the frame-late `GlobalTransform`), orientation always the
    camera's — the reference `audio_update_listener` shape.
  - `EnableCollisionSounds` (Bool, on; `world_sounds.rs`) — gates the
    viewer-synthesized material collision one-shots in
    `ingest_collisions` (backlog drained while off, so re-enable does not
    burst); scripted `llCollisionSound` triggers are deliberately not
    gated.
- **Streaming**: the existing `MusicStreamEnabled` (parcel-music autoplay)
  and `MediaAutoPlayEnabled` (media-on-a-prim autoplay) surfaced as
  checkboxes (constants made `pub(crate)`).
- **Output device**: `AudioOutputDevice` (String, "" = system default;
  `audio.rs`), a combo over `Mixer::output_devices()` — device names ride
  the Fluent key-fallback as their own labels. The applier
  (`apply_output_device`, `Last`, before the pump) rebuilds the graph via
  `rebuild_and_restart` on change or on a persisted device at startup,
  with an explicit fall-back to the system default when a name fails
  (the mixer's automatic fallback only covers a running device
  disappearing); the last-applied name is always recorded so a broken
  name never retries per frame. The list **re-enumerates every 2 s while
  the floater is open** (user request: PipeWire / Pulse hot-plugs are
  common; cpal has no change notification, so it polls) and updates the
  combo in place through the new `ui_combo::SetComboOptions` message —
  equal list a no-op, deferred while the popover is open, selection
  clamped, `ComboBindingValues` moved in the same pass.

Out of scope by design (no consumer exists yet): voice toggles / devices /
push-to-talk (the voice tasks), gesture sounds (no gesture playback), the
per-UI-sound editor (own settings surface), doppler / rolloff factors.

Verified by 14 new headless tests (registered defaults per module; the
focus-mute truth table + a mixer-backed app test pinning "silenced in the
mixer, store untouched, gain retained"; `resolve_listener` mode table;
`device_switch` cases; the collision gate predicate; distinct row-label
keys; device options lead with the default; `SetComboOptions` replace /
clamp / popover-deferral; the open-floater poll gate) and live on the
local grid: a screenshot run (floater pre-opened via `preferences_visible`
and `SL_VIEWER_PREFERENCES_TAB=audio`) shows the tab with reference defaults
and the device combo on "System default", audio stream up on the default
device, no audio warnings. The ear-mode / focus-mute / collision / device
switch appliers are covered by the unit tests above; audible A/B on a
live scene was not part of this pass.

Reference: `panel_preferences_sound.xml`, `llvieweraudio.cpp`
(`audio_update_listener`), `llfloaterpreference.cpp`.
