---
id: viewer-screenshot-fixed-resolution
title: Pin the capture resolution, so two viewers can be compared at all
topic: viewer
status: done
origin: Firestorm cross-check harness plan (2026-09-01)
points: 2
refs: [viewer-screenshot-wait-for-quiescence]
---

Context: [context/testing.md](../context/testing.md).

`--screenshot-dir` captured `Screenshot::primary_window()`, so a frame was
whatever size the window happened to be — Bevy's default, or whatever a
tiling WM handed us. That is fine while the only consumer is this viewer's
own pixel oracles, which classify colours at CPU-projected points and do
not care about absolute size; it is fatal the moment a frame is put beside
Firestorm's, because two images of different dimensions cannot be diffed,
tiled into a contact sheet, or compared at a named pixel.

Shipped as **`--capture-size WxH`**, env `SL_VIEWER_CAPTURE_SIZE` — the
variable Firestorm's harness reads too, so one env block sizes both
viewers. It defaults to 1080p rather than to the window, as the Firestorm
side already did, so a run's frames are comparable without anyone
remembering to ask. A malformed value is refused by `clap` before the
viewer starts (`1920`, `0x1080`, `axb`, `9000x1080`, all covered by unit
tests): a silent fallback would produce a full run of unusable frames
whose only symptom is that the diff step later refuses them.

**It does not resize the window, and must not.** The plan said to apply
the size to the primary window before the first capture, which the
Firestorm side then proved insufficient — a window size is a *request*,
and a tiling compositor answers it with its own size, more than once
within one run. Firestorm watched a sequence change resolution between
`frame_000` and `frame_001` when the window lost focus, and now pins the
size it hands `saveSnapshot` instead of reading it off the window. The
same reasoning applies here and lands the same way: the flag points the
`ViewerCamera` at an off-screen image of exactly `WxH` and captures
*that*, so the window manager cannot pick the resolution or change it
mid-sequence. `--capture-size` rather than `--window-size` because a flag
that does not size the window should not be called one; the environment
variable keeps Firestorm's name for the parity that matters.

Two consequences of capturing the camera rather than the window, both of
which are what the cross-check wants:

- **The frames hold the world and nothing else**, unless asked: the UI,
  the HUD layer and the edit gizmos are drawn by their own cameras, and
  each is now an independent switch — see [[viewer-capture-layers]], which
  followed immediately from this.
- **The window shows a preview** of the very image being captured.
  Without it the window would show an uncleared surface, which looks
  exactly like a rendering bug; with it, what is on screen is what lands
  in the frame. The window is handed back to the world camera once the
  last frame is written, so the logout grace period is a live view again.

The preview cost three wrong turns, all of which rendered a *plausible*
black window, and all three are the kind that will be made again:

1. It was a `bevy_ui` `ImageNode`, and showed the world as **black with
   only the glowing prims visible**. A UI image node alpha-blends, and a
   viewer frame's alpha is not opacity — it carries the glow mask (and the
   HDR brightness the PNG write already drops). It is now a textured quad
   with an unlit `AlphaMode::Opaque` material, which ignores the texture's
   alpha exactly as the PNG write does; being a 3D quad on its own render
   layer it also stays out of the UI, which a UI capture routes into the
   frame.
2. The quad was `ChildOf` its camera with the identity transform, which
   puts it exactly **at** the camera. Nothing rendered.
3. The preview camera was `Msaa::Off` and not `Hdr`, while the HUD / UI
   camera sharing the window is `Msaa::Sample4` + `Hdr`. Bevy keys a
   view's main texture on those, so the two cameras got *separate*
   textures and the later camera's blit to the swapchain overwrote the
   preview's — a black window with the UI still drawn over it. `hud.rs`
   states this rule ("must match the world camera's sample count and
   HDR-ness — the two share the window's view-target chain"); it applies
   to any camera added to the window, not only that one.

Refs [[viewer-screenshot-wait-for-quiescence]] — the capture still waits
for quiescence, which is unaffected: the pin is applied once at
`PostStartup`, long before the first frame is armed, so nothing re-triggers
texture or LOD work part-way through a sequence.
