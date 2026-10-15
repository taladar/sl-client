---
id: viewer-ui-styling-interaction-tests
title: bevy_flair state styling under synthetic hover and focus
topic: viewer
status: ready
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 5
blocked_by: [viewer-ui-interaction-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The headless harness excludes styling entirely, so the skin's
`:hover`/`:focus`/`:focus-visible`/`:active` rules
(`sl-viewer-ui-core/src/skin.rs` + the `.css` assets) are untested. Add a
`ViewerSkinPlugin` variant of `InteractionTest`; synthetic pointer/focus drives
the real `HoverMap`/`InputFocus`; assert computed-style transitions — colors and
backgrounds change on hover and revert on out, focus rings appear on
`:focus-visible`, `:active` applies while pressed — across the `ELEMENTS`
registry.

Open question to establish first: whether bevy_flair's (the workspace's fork)
style application runs without render-world extraction. If it resolves styles
into ECS components CPU-side (expected), this is straightforward; if not, the
task documents the boundary and moves what is testable.

Audit 2026-10-15: a first slice exists in `sl-viewer-ui-core/src/skin.rs`
tests — `stamp_hover_state` hands `Hovered` to the hoverable classes
(`a_hoverable_node_is_given_the_hover_component`), and two lints hold
`common.css`'s `:hover` rules to `HOVER_CLASSES` and to a resting rule each.
Those check the rules exist and have something to match; the
`InteractionTest` (`sl-viewer-testkit/src/interact.rs`) still runs without
the skin, so the computed-style sweep over `ELEMENTS` under synthetic
hover / focus / press is what remains.
