---
id: viewer-automation-offscreen-window-spike
title: Spike — how a windowless viewer renders and picks at the same time
topic: viewer
status: ready
origin: viewer automation design review (2026-09-28)
points: 3
refs: [viewer-automation-windowless-mode, viewer-ui-interaction-harness]
---

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
