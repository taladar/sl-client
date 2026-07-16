---
id: viewer-ui-test-harness
title: UI test harness — headless layout assertions + an isolated gallery
topic: viewer
status: ready
origin: the viewer-ui-widget-scaffold review (2026-07), where finding one layout bug cost a live login and six rounds of a human pressing a debug key
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-text-node-padding-measure, viewer-i18n-fluent-scaffold, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

A way to exercise UI elements **without a viewer, a login, or a world** —
because the bugs this cluster will actually ship are the ones that only appear
in a particular font, script, translation or UI scale, and that combinatorial
space cannot be walked by a human logging into a grid.

This is not speculative. Diagnosing [[viewer-text-node-padding-measure]] — a
text node laid out one line shorter than the text it drew — took a login to
OpenSim, a temporary debug key wired into the demo panel, and six rounds of a
human pressing it and reporting numbers back. The whole thing is a pure function
of a font, a string, and an available width. It should have been a `cargo test`.

Two halves, both needed, useful independently:

## 1. Headless layout assertions (`cargo test`)

Stand up enough of `bevy_ui` in a test to spawn a fixture, run the layout, and
assert on the resulting `ComputedNode`s — no window, no renderer, no session.

The invariant that would have caught the bug above, and that generalises to the
whole cluster:

- **No node's `content_size` may exceed its `size`.** Content spilling out of
  its own box is never intentional in this UI, and it is exactly what a wrongly
  measured text node looks like. Assert it over the *whole* fixture tree, so one
  check covers every widget.

Others worth having: no panel exceeds the viewport; nothing overlaps that should
not; a text node's height is a whole number of its own line heights.

**Known obstacle, hit while trying this by hand:** `bevy_ui`'s own layout tests
(`bevy_ui-0.19.0/src/layout/mod.rs`, `setup_ui_test_app`) drive layout
headlessly, but through `pub(crate)` internals — `propagate_ui_target_cameras`,
`ComputedCameraValues`, `RenderTargetInfo`, `mark_dirty_trees` — none of which
are reachable from a downstream crate, and the harness omits
`measure_text_system` anyway. So the first job is to find what *is* reachable
(possibly the whole of `UiPlugin` against a dummy render target), and if nothing
is, to **upstream a public headless-layout test harness to Bevy** — see the
standing preference in `sl-client-fork-upstream-for-upstream-bugs`. Establish
this before assuming the approach; it is the task's one real unknown.

## 2. An isolated UI gallery (a binary)

A "storybook": a binary that renders widgets and panels **in isolation**, with
no login and no world, so a human can look at one thing and click it.

The requirement that makes it more than a convenience — and the reason it
belongs here rather than being bolted on later:

- **A UI element must be constructible without its wiring.** In the gallery a
  button must be clickable *without* firing whatever it does in the live viewer:
  no teleport, no object edit, no L$ spent. So a panel's **construction** has to
  be separable from its **actions**, with the actions injectable — inert stubs
  in the gallery, real handlers in the viewer.

  This is a **constraint on every downstream UI task**, not just on this one. A
  panel that can only be spawned by reaching for a live `Session` is a panel
  that can never be tested, and retrofitting that separation is exactly the kind
  of late rework [[viewer-ui-widget-scaffold]] exists to prevent. Write the rule
  down in the scaffold's conventions once this task settles its shape.

The gallery is also where the **matrix** lives, since it is the thing a human
eyeballs and the screenshot harness can capture:

- **Scripts**: Latin, CJK, Cyrillic, Arabic / Hebrew (RTL), Devanagari, emoji.
- **Direction**: LTR and RTL, via the scaffold's `UiDirection`.
- **Scale**: several UI font sizes and `UiScale` / window scale factors — the
  bug above surfaced at scale 1.5.
- **Translation length**: German and Finnish run long, CJK runs short but tall.
  Before real translations exist, **pseudolocalisation** covers this — expand
  each string ~40%, add diacritics, bracket it — which catches overflow with no
  translator involved. Fits [[viewer-i18n-fluent-scaffold]].

Reference: none — the reference viewer has no UI test harness, which is part of
why its skins break every release. This one is ours to design.
