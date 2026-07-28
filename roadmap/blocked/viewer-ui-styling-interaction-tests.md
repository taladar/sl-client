---
id: viewer-ui-styling-interaction-tests
title: bevy_flair state styling under synthetic hover and focus
topic: viewer
status: blocked
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 5
blocked_by: [viewer-ui-interaction-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The headless harness excludes styling entirely, so the skin's
`:hover`/`:focus`/`:focus-visible`/`:active` rules (`skin.rs` + the
`.css` assets) are untested. Add a `ViewerSkinPlugin` variant of
`InteractionTest`; synthetic pointer/focus drives the real
`HoverMap`/`InputFocus`; assert computed-style transitions — colors and
backgrounds change on hover and revert on out, focus rings appear on
`:focus-visible`, `:active` applies while pressed — across the `ELEMENTS`
registry.

Open question to establish first: whether bevy_flair 0.8's style
application runs without render-world extraction. If it resolves styles
into ECS components CPU-side (expected), this is straightforward; if not,
the task documents the boundary and moves what is testable.
