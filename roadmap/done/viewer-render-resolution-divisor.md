---
id: viewer-render-resolution-divisor
title: Render at a reduced resolution (RenderResolutionDivisor)
topic: viewer
status: done
origin: gap found wiring viewer-rlv-debug-settings-commands (2026-09-07)
refs: [viewer-rlv-debug-settings-commands, viewer-rlv-vision-render]
---

Context: [context/viewer.md](../context/viewer.md).

Render the 3D scene at `1/n` of the window's resolution and upscale it to fill
the view — the reference viewer's `RenderResolutionDivisor` debug setting.

The viewer has no reduced-resolution path at all today: every camera renders
straight to the window's swapchain image at its full size. What is wanted is
the ordinary offscreen-then-blit shape — the world camera renders to an image
of `size / n`, and that image is drawn to the window — with the UI drawn at
full resolution on top, because the point is a coarser *world*, not a coarser
interface.

Two callers want it, for opposite reasons:

- **performance.** It is the bluntest quality lever there is, and the one a
  user reaches for when a region is too heavy for their machine. It belongs in
  the graphics preferences beside the other `Render*` settings.
- **RLV vision impairment.** `@setdebug_renderresolutiondivisor:<n>=force` is
  the oldest and cheapest blur a collar can impose
  ([[viewer-rlv-debug-settings-commands]] wired the command up and left it
  answering nothing, precisely because there is no setting behind it that
  anything looks at). Landing this makes `ViewerRlvExt::debug_value` /
  `set_debug_value` able to answer that row, and turns the `@setdebug=n`
  settings-editor gate — already wired and unit-tested — from a rule over an
  empty set into one that hides a real row.

Scope: the render path, a `RenderResolutionDivisor` setting registered with the
rest of the render family, the graphics-preferences control for it, and the
two `ViewerRlvExt` arms that then have something to read and write. Note the
reference's rule that a value a script wrote is **never** persisted to disk
(its `DBG_PERSIST` flag) — with a real writable row that rule finally has
something to protect, and belongs at the write site.

The related but distinct RLVa effects — the `@setsphere` blur / darken /
chromatic system and `@setoverlay` — are [[viewer-rlv-vision-render]]; this is
the plain resolution lever, which is a graphics feature that RLV merely
happens to be able to drive.

## Done (2026-09-20)

`sl-viewer-world-scene/src/resolution_divisor.rs` (+ `.wgsl`), registered as
`ResolutionDivisorPlugin` in the viewer's full render stack. The world camera
is pointed at an `Rgba16Float` image `1/n` the window's physical size; one
overlay camera stretches that image over the window before drawing its own
content; the interface is untouched and stays at full resolution. At the
default divisor of `1` nothing in the module does anything at all — no image is
allocated, no component is inserted and the pass finds no view — so the
ordinary frame is the one the viewer drew before this existed.

### The scale factor is the whole trick

A reduced render target is a trap for everything that maps between the screen
and the world — the cursor pick, the edit gizmos' drag rays, the beacon arrows,
the name-tag projection — because all of them go through
`Camera::viewport_to_world` / `world_to_viewport`, and those work in *logical*
pixels derived as `physical_size / scale_factor`.

Bevy's `ImageRenderTarget` carries a `scale_factor` of its own, so the image is
given the window's **divided by the divisor**: its physical size shrinks, its
logical size does not, and not one of those call sites needed changing or even
knows the divisor exists. The alternative was a `/ n` at every projection site
in four crates, where the one that was missed would have been a pick landing
where the cursor is not.

### Why the upscale is a render pass and not a sprite or a UI node

The overlay cameras (the gizmo layer at order 1, the HUD + UI layer at 2) carry
`ClearColorConfig::None` because they are drawn *onto the world camera's frame*
in the window's shared main texture. Divert the world camera and that texture
holds whatever was last in it, so something has to put the reduced frame there
first, under the gizmos and the HUD and over nothing.

- A **sprite** would blend by the frame's alpha, and in this viewer a frame's
  alpha is the glow mask rather than coverage.
- A **UI node** draws in the UI pass, which runs *after* the HUD attachments —
  it would cover them.

So it is a fullscreen pass in `Core3dSystems::Prepass` on the view carrying a
`WorldUpscale`, writing through `ViewTarget::get_color_attachment` — the
multisampled attachment the camera's own main pass then loads, exactly as
`underwater_fog` does and for the same reason (a post-process would read and
rewrite a resolved copy the next resolve discards).

Which overlay carries `WorldUpscale` is recomputed each frame: the
lowest-ordered `OverlayCamera` still pointed at the window. That is the gizmo
camera in the ordinary viewer, the HUD camera in a build without the edit
tools, and *nothing* in a harness that routed both elsewhere — in which case
the divisor is forced back to `1` rather than rendering into an image nobody
shows. A harness that pointed the world camera at a target of its own (the
readback and full-stack tiers both spawn the viewer's camera bundle straight
onto an image) is likewise left alone: the drive system only moves a target
that is a window or the image it made itself.

### The reference's two clamps, kept

`effective_divisor` reproduces `LLPipeline::refreshCachedSettings`: anything at
or below `1` is off, and a divisor at least as large as the smaller side is
held below it (Firestorm's FIRE-7066 fix, which exists because the unclamped
version divided a dimension to zero). On top of those, a ceiling of 16 — the
reference has none, but a 1/32 render serves nobody and the tab's ladder needs
a last rung.

### The control is a ladder, not a slider

The graphics tab offers **Full / Half / Quarter / Eighth / Sixteenth** as a
combo, beside the three power-of-two combos that tab already has (shadow-map
size, mirror resolution, mirror update rate). A linear 1–16 slider was the
first shape and it was wrong: what a divisor costs goes as `1/n²`, so `1 → 2`
throws away three quarters of the world's pixels while `8 → 9` throws away a
fiftieth — an evenly-spaced control would have spent most of its travel on
steps nobody can perceive and crammed the useful range into its first inch.
Each rung here halves both axes, so one step is one consistent change in cost
and in softness. It also reads: a combo shows the value it holds, and no slider
in this viewer does ([[viewer-sliders-show-no-value]], filed from this same
live check).

The *setting* stays a free `u32` — `effective_divisor` accepts any value and an
RLV script may write one — so this is the ladder the tab offers, not a
restriction. A test pins that the ladder halves, starts at 1, and ends exactly
at `MAX_RESOLUTION_DIVISOR`: a tab offering a value the clamp then silently
reduces would be a control that lies.

### RLV: the row that answers something

`ViewerRlvExt` now borrows the settings store **mutably**, which is what
`set_debug_value` needed and why `RlvRun`'s `settings` became a `ResMut`.
`@getdebug_renderresolutiondivisor` reads the stored value (falling back to `1`
for a viewer with no store, since "the world is drawn at the window's
resolution" is true either way), and
`@setdebug_renderresolutiondivisor:<n>=force` writes it to the global scope —
the same layer the graphics tab's own control writes, so the user can see and
undo what a script did.

The `DBG_PERSIST` rule came with it, and needed a new primitive:
`SettingsStore::set_persist`, the reference's `LLControlVariable::setPersist`.
After a script write the row persists only while the stored value *is* the
declared default, so a collar's blur is live for the session, absent from the
user's settings file, and gone after a relog — and persistence comes back the
moment the value is the default again.

The `@setdebug=n` settings-editor gate, wired and unit-tested by
[[viewer-rlv-debug-settings-commands]] over an empty set, now hides a real row.

### What the live check found

Two rounds on the local grid. The first showed the picking claim holding (the
scale-factor trick works) and the world reducing while the interface stayed
sharp — and also, intermittently, **the whole frame going blocky, interface
included, on the way back to divisor 1**, plus sub-pixel drift in the UI layout
between 1 and 2.

One cause for both. Bevy's `camera_system` refreshes a camera's `target_info`
— the physical size every later stage reads — when the window or image it
*names* changes, when the camera is new, when its viewport moves, or when its
projection changes. Pointing a camera at a **different** target is in none of
those, so the world camera came back to the window still claiming the reduced
image's size; `prepare_view_targets` sizes a target's shared main texture from
the *first* camera it visits in the group, so that stale claim could build the
**window's** texture at `1/n` and every overlay, the HUD and the whole UI were
then drawn into it and stretched. Which camera is visited first is what made it
intermittent.

`announce_target_change` marks the camera's `Projection` changed whenever the
target's shape moves — a different target, or the same image at a new size.
That is the one recompute trigger in Bevy's list a consumer can reach, and the
recompute is wanted anyway: the new target's aspect ratio is exactly what the
projection has to be rebuilt from. The resize case needs it too, because the
`AssetEvent::Modified` Bevy would otherwise notice arrives after
`camera_system` has already run for the frame.

### What it does not fix, and that is a parity gap of its own

Avatar name plates and `llSetText` hover text are **soft at any divisor above
1**, because both are world geometry on the main layer and so are rendered into
the reduced target with everything else. (Their apparent *size* is right — the
billboard shader derives metres-per-pixel from the viewport — it is only the
glyphs that are magnified.) The reference draws neither in its world pass:
`LLHUDObject::renderAll()` runs from `render_ui()` after
`gPipeline.renderFinalize()`, i.e. after the upscale. Moving ours wants an
overlay camera and an order slot in a ladder three crates spawn into, so it is
[[viewer-world-text-in-the-overlay-pass]] rather than part of this.

### Tests

Twenty-two new ones and one rewritten, all offline and all without a GPU.

Pure, over the arithmetic: the two reference clamps and the "never reaches
zero" floor; that a reduced target keeps the window's logical size across four
scale-factor / divisor pairs; that the registered default is the reference's.

App-level, over every decision the drive system makes — which is where this can
go wrong in ways a screenshot would not show. A minimal app with the real
settings store, an image store, a primary window and the viewer's own three
cameras pins that: the default allocates **nothing at all**; a divisor diverts
the world camera to a correctly-sized image with the divided scale factor and
marks the *lowest-ordered* overlay; turning it off restores the window and
drops the mark; a window resize moves the image in place rather than
reallocating (the handle is what both the target and the mark name); with no
overlay left on the window the divisor is forced off rather than rendering into
an image nothing shows; and a world camera a harness pointed at its own target
is never moved.

And the two that came out of the live check: that every change of target
shape announces itself and a quiet frame does not, in both directions and
across a resize. Without the announcement the "back to 1" assertion is what
fails, which is the symptom that was actually seen.

The tab: that the resolution ladder halves, starts full, and ends at the
clamp.

RLV: the read fallback and write refusal with no store; the write round trip;
the persistence rule in both directions, including that a held row is absent
from the serialized scope; that no other allowlist row is writable and that a
write of the right row in the wrong type is refused. Plus `set_persist` itself
at the store level.
