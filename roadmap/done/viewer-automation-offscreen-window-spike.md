---
id: viewer-automation-offscreen-window-spike
title: Spike — how a windowless viewer renders and picks at the same time
topic: viewer
status: done
origin: viewer automation design review (2026-09-28)
points: 3
refs: [viewer-automation-windowless-mode, viewer-ui-interaction-harness]
---

## Done (2026-09-28)

**Decision: option 1, a surfaceless window in the Bevy fork** — written up,
with the rejected options and why, under "Rendering and picking without a
window" in [context/automation.md](../context/automation.md);
[[viewer-automation-windowless-mode]] now says how to build it into the
viewer.

- Fork: `bevy::window::OffscreenWindow`. `bevy_winit` creates no platform
  window for it; `bevy_render` extracts it with no `RawHandleWrapper`, gives
  it an `Rgba8UnormSrgb` texture of its physical size as its swap chain
  (`TEXTURE_BINDING | COPY_SRC`, never presented, no surface configured) and
  `Screenshot::primary_window()` reads it back.
- Prototype: `full_stack_test`'s `HarnessOptions::in_offscreen_window` spawns
  the primary window with the marker (scale factor 1), installs
  `SyntheticInputPlugin` and screenshots the window every frame. The test
  `an_offscreen_window_renders_the_ui_and_takes_clicks_and_picks` logs into
  the fake grid in a 1280×720 off-screen window, renders the world with the
  full chrome over it, right-clicks the stock box (GPU ID-buffer pick → the
  box → its object pie), and clicks the toolbar's Inventory button where the
  locator engine aims (its "receives events" check is `bevy_picking` against
  the window's UI camera). Every pixel of the floater's laid-out box changes
  between the frames, and almost nothing outside it; hiding the floater makes
  it fail.
- Found on the way: `Escape` never closed a pie, although `pie_menu.rs`
  promised it (only focus loss did) — fixed (`abort_pie_on_escape`, live
  pies only) with `escape_closes_a_pinned_pie`.

Context: [context/automation.md](../context/automation.md).

The existing tiers split for a reason nobody wrote down: `InteractionTest`
picks but never renders (its `Window` has no surface), and the screenshot
harness renders UI into an `Image` but never clicks. A windowless automated
viewer needs both at once, and the obvious combination does not work:

- `bevy_ui`'s picking backend (`picking_backend.rs`) and
  `bevy_picking::pointer` only hit cameras whose normalized render target
  equals the pointer's target. The mouse pointer targets the window, so UI
  cameras rendering into an `Image` get **no hits**;
- about 25 viewer files read `Window::cursor_position()` through
  `PrimaryWindow` (GPU pick, camera, pie menu, floater drag, inventory drag,
  edit create, inspector popup, …);
- a `Window` entity with no `RawHandleWrapper` has no surface, so cameras
  aimed at it render nothing.

## Decide between

1. **A surfaceless window in the Bevy fork**: `bevy_render` gives a window
   with no raw handle an off-screen texture of its resolution as its
   "swapchain", readable like a screenshot. Cameras keep
   `WindowRef::Primary`, so picking and every cursor reader work unchanged.
   The reviewer's recommendation.
2. **Custom pointers on the image**: a `PointerId` whose location targets
   the image, plus reworking the ~25 `PrimaryWindow` cursor readers to ask
   "the pointer" instead of "the window".
3. **An invisible winit window**: unreliable on Wayland (a compositor may
   not map, size or render an unmapped window), and still needs a
   compositor.

Prototype the chosen option far enough to render one frame with UI and
land one synthetic click on a UI button and one GPU pick on a mesh, and
record the decision (and the rejected options with their reasons) in
[context/automation.md](../context/automation.md).

Acceptance: the decision and the prototype's result are written down; the
windowless-mode task's text is updated to the chosen design.
