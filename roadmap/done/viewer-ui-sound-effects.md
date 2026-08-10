---
id: viewer-ui-sound-effects
title: UI sound effects
topic: viewer
status: done
origin: user request (2026-07)
blocked_by: [viewer-audio-backend, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The non-spatial half of sound: the viewer's own feedback sounds, played on a 2-D
bus with no position and no attenuation. The reference has a whole set of them
(the `UISnd*` settings) — button click, alert, invalid operation, money paid /
received, teleport, snapshot shutter, incoming IM and chat, typing, window open
/ close — each individually overridable and mutable, under their own volume
category.

Two concrete hooks already waiting:

- **The typing sound.** `typing.rs` says it in as many words: P31.9 shipped the
  typing *animation* and deliberately left the *sound* out because the viewer
  has no sound playback. This closes that gap.
- **Gesture sound steps.** The gesture runtime in [[viewer-gesture-runtime]]
  sequences animation + sound + chat + wait steps; its sound steps play through
  this bus.

Scope: the sound set and its defaults, loading them (shipped assets vs. fetched
from the grid — the reference's UI sounds are asset UUIDs, so they come down the
same `sl-asset` path), the volume category and per-sound mute, the preferences
surface, and the plumbing that lets any UI or notification raise a sound without
reaching into the audio engine directly.

Reference (Firestorm, read-only): the `llui` sound settings and
`LLUI::sSettingGroups` sound lookups, `llgesturemgr` (sound steps).

Builds on: `typing.rs` (the recorded gap) and the notification / UI surfaces.

Deps: [[viewer-audio-backend]] (device + mixer),
[[viewer-ui-widget-scaffold]] (the events that raise the sounds).

## Progress (2026-08-10)

Shipped `ui_sounds.rs`: the `UiSound` catalogue with the reference `UISnd*`
default UUIDs, a persisted per-sound enable + overridable-asset settings pair,
the `PlayUiSound` message any surface writes, a login prefetch, and a driver
playing on the shared mixer's UI bus (via the shared
[[viewer-in-world-sounds]] `SoundCache`). Auto-emitters wired: the **typing**
chirp (closing the `typing.rs` gap), **money up/down**, **teleport-out**, and
the **snapshot** shutter. The rest of the catalogue is registered and playable
through `PlayUiSound` for future callers (the gesture runtime's sound steps),
deliberately not auto-emitted yet.

**Skin/theme override** (user request): a skin sets a `-sk-uisnd-<key>` CSS
property to a `url("file")` — bundled with the skin, resolved relative to its
stylesheet by `bevy_flair`, loaded as a Bevy `AudioSource` and decoded through
*our* pipeline onto the UI bus (not `bevy_audio`) — or a `"uuid"` grid asset.
Resolution order is user setting → skin → reference default. The `bevy_flair`
custom parser + `Placeholder` and the `SkinUiSounds` component live in
`ui_sounds.rs`; the CSS properties are registered from the skin plugin.

Not done here (own tasks): the Preferences **audio tab** surface
([[viewer-preferences-audio-tab]]) for tuning these, and the gesture runtime's
sound steps ([[viewer-gesture-runtime]]).
