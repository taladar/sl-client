---
id: viewer-hud-attachments-not-composited
title: A HUD attachment is built, seated and visible, and never reaches the frame
topic: viewer
status: done
origin: chasing viewer-prim-attachment-worn-but-not-rendered on the local grid
  (2026-09-09)
refs: [viewer-prim-attachment-worn-but-not-rendered, viewer-p35-1, viewer-p35-2]
---

Context: [context/viewer.md](../context/viewer.md).

Nothing worn on a **HUD** point renders. The user confirms HUD attachments did
render at some earlier point, so this is a regression; it is **older** than the
2026-09-08/09 rendering series (reproduced unchanged at `3435f9ee`, before
[[viewer-underwater-fog-swallows-translucency]] landed).

Live on the local grid, one plain 0.5 m plywood cube worn on **HUD Top Left**
(and earlier on HUD Center / HUD Center 2), with a second cube on the **Chest**
as a control: the chest cube draws, the HUD cube is nowhere — not on the window,
and not in a `--screenshot-dir` capture taken with `SL_VIEWER_CAPTURE_HUD=1`.

## Everything upstream of the composite is correct

Measured with `SL_VIEWER_LOG_ATTACHMENT_BIND=1`, which now traces all three
stages of a worn object's journey:

- the object **arrives**: `worn object circuit#1/… arrived (Prim) on point 35`;
- it is **routed**: `HUD attachment … routed to HUD point 35 (screen node …)`;
- its faces are **built, on the right layer, and visible** — the HUD census
  reports `12 drawable entit(y/ies) under the screen, 12 visible to a camera`,
  each `layers=[1] inherited_visible=true view_visible=true at Vec3(0,0,0)`.

`view_visible` is set by the visibility pass per camera, so the HUD camera's
frustum genuinely contains this geometry. No pipeline, shader or wgpu error is
logged at any level.

## ROOT CAUSE: the HUD camera's 3D pass is discarded by the shared view target

Two probes, on the window (the capture path routes cameras and confounds the
question — measure this one live):

1. **`clear_color: Custom(magenta)` on the HUD camera** → no magenta anywhere,
   while the viewer's own UI still draws. The UI is `bevy_ui` on the same
   camera (it carries `IsDefaultUiCamera`), so the camera is alive and its
   graph runs; it is the **3D pass** — its clear included — that never lands.
2. **`Msaa::Off` on the HUD camera** (everything else unchanged) → the whole
   window turns magenta, the world gone, the UI on top. Given its **own** view
   target the very same pass reaches the swapchain, and its blit (order 2, last)
   overwrites the world camera's.

So the pass is fine and the camera is fine. What loses it is the target the two
cameras **share**: `hud.rs` matches the world camera's `Msaa::Sample4` + `Hdr`
on purpose, so that the HUD composites over the frame the world camera left
(`ClearColorConfig::None`). But an HDR view target is a ping-pong **pair**, and
the world camera's post-processing chain (`SlTonemap`, the glow pass) swaps
which of the two is current. The HUD camera then draws into one texture while
the blit to the swapchain reads the other, and its pixels are written and
dropped.

The camera census (also under `SL_VIEWER_LOG_ATTACHMENT_BIND=1`) confirms there
is nothing else on the window to blame — exactly two window cameras, the world
at `order=0 msaa=4 hdr=true layers=(default)` and the HUD at
`order=2 msaa=4 hdr=true layers=[1]`; the other 38 are probe-capture cameras on
image targets.

## FIXED (2026-09-09): the blit blended by the glow mask

`bevy_core_pipeline::upscaling::prepare_view_upscaling_pipelines` picks the
blend state of a camera's blit to its target from that camera's **position** on
the target: `sorted_camera_index_for_target == 0` replaces, and every later
camera defaults to `BlendState::ALPHA_BLENDING` "so they don't accidentally
overwrite earlier cameras' output".

That default is right for a camera with its own target whose alpha means
coverage. It is wrong twice over here. The HUD camera **shares** the world
camera's main texture — that is the whole point of matching its sample count
and HDR-ness — so what it blits is already the finished frame with the HUD drawn
into it, and there is nothing to blend against. And a viewer frame's alpha is
the **glow mask** (memory `sl-client-transparent-overlay-glow-mask`), not
opacity, so blending by it composited every HUD pixel at roughly zero alpha.

The fix is to say so: the HUD camera (and the gizmo overlay camera, which had
the identical arrangement) now carries an explicit `output_mode` of
`Write { blend_state: Some(BlendState::REPLACE), clear_color: None }`.
Gizmos had survived the default only because their geometry writes a solid alpha
where it draws — an accident of what they draw, not a property of the setup.

This also explains why the magenta clear-colour probe showed nothing: MSAA
writeback runs for any non-first camera on a target, and it consumes the colour
attachment's first-call, so the main pass's `Clear` degrades to a `Load`.

**Verified live** on the local grid: a plain prim worn on HUD Top Left draws in
the corner of the window, the world intact behind it and the UI over it.

Pinned by `hud`'s `the_hud_camera_replaces_the_frame_it_composites_into`, which
asserts the whole arrangement — the sharing preconditions and the explicit
replace.

## Still worth doing

A **render tier** test. The HUD is otherwise pinned only by *picking* tests
(`world_test`'s HUD pie / occlusion cases), which is why a total loss of HUD
rendering went unnoticed; nothing in `render_readback` / `render_test` /
`full_stack_test` mentions the HUD.

## Reproducing

Wear a plain prim on a HUD point (`sl-repl`:
`rez_attachment <item> hudtopleft`), then run the viewer with
`SL_VIEWER_LOG_ATTACHMENT_BIND=1` and watch the arrival / routing / census
lines say everything is right while the corner of the frame stays empty.
