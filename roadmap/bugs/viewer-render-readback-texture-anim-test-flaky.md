---
id: viewer-render-readback-texture-anim-test-flaky
title: render_readback texture-animation test is flaky under load
topic: viewer
status: bugs
origin: observed while testing viewer-preferences-general-tab (2026-08-04)
---

Context: [context/viewer.md](../context/viewer.md).

`render_readback::tests::a_texture_animation_actually_moves_on_screen` is
**load-sensitive**: during the general-tab work it failed twice inside full
`cargo test -p sl-client-bevy-viewer --lib` runs and once standalone on a
tree with concurrent builds running — including on a clean checkout of
`0508804c`, so it is not tied to any particular change — yet passes when run
alone on an idle machine (9.5 s).

The test drives a real GPU readback and asserts the animated texture moved
between frames; under CPU/GPU contention the sampled frames apparently show
no movement yet. Likely fix directions: sample more frames / wait on a
readback fence rather than a frame count, or assert over a longer window.

Until fixed, a full-suite failure of only this test on a loaded machine is
suspect — re-run it standalone before treating it as a regression.
