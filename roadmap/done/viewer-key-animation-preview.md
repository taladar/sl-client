---
id: viewer-key-animation-preview
title: One animation-preview window per animation
topic: viewer
status: done
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 2
refs: [viewer-keyed-floater-audit, viewer-key-texture-preview,
  viewer-profile-floater-single-instance]
---

Context: [context/viewer.md](../context/viewer.md).

`inventory_properties.rs`'s **animation preview** (`"preview-animation"`) is
still a singleton: opening a second animation repoints the one window. Two
animations open at once is how a resident compares them, and the window is not
one of the kinds the reference deliberately keeps single.

Convert it onto the keyed scaffold ([[viewer-profile-floater-single-instance]]):

- **Key** by the animation's asset id (a `FloaterKey::subject`).
- Move `PreviewState::animation` onto the window root as a component;
  [[viewer-key-texture-preview]] takes the texture half, and whichever lands
  second deletes the resource.
- Open through `KeyedFloaters::open`, build the content at spawn rather than
  through `DeferredFloaterContent`, and order the open system
  `.after(FloaterSystems::Commands)`.
- Re-opening an animation already up **raises** it rather than restarting it.
- Check what a second preview means for **playback**: if the preview plays the
  animation on the agent (rather than in an isolated view), two windows must
  not fight over the same avatar — decide and write down whether the second
  window's play stops the first's, or whether play stays one-at-a-time with the
  windows merely holding their own subject.

## How to verify

Open two different animations from inventory: two windows, each on its own
animation, each closable on its own; re-opening one raises it. Pin it with an
`instances` unit test mirroring `two_landmarks_open_two_windows`.

## Done (2026-09-12)

Keyed by the animation asset, alongside [[viewer-key-texture-preview]], whose
entry carries what the pair deleted. `AnimationPreviewState` is a component on
the window root and the Play / Stop buttons **read it through**
`host_floater` rather than capturing an asset id in their observer closures:
with two previews open the two Play buttons are the same control in two
windows, and only the window says which animation it is about.

**What a second preview means for playback**, the question this task asked to
answer and write down: nothing has to arbitrate. The preview plays the
animation on the agent, and an avatar plays as many animations at once as it is
told to, so two windows are two independent transports — each Play sends
`PlayAnimation` for its own asset and each Stop stops its own. The module
header says so.
