---
id: viewer-ui-test-harness
title: UI test harness — headless layout assertions + an isolated gallery
topic: viewer
status: done
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

## Done

Built as **three tiers of check × a registry of elements × a matrix**, which is
a different shape from the two halves sketched above. The change came out of the
work, and each part of it earned its place:

- **The matrix moved out of the gallery.** As written, the matrix lived with the
  thing a human eyeballs. That reintroduces the combinatorial explosion this
  task exists to end: nobody walks 8 scripts × 3 sizes × 3 scales × 2 directions
  × every element by eye. Whether a layout is *correct* is machine-checkable, so
  a machine checks it — every cell, every run. The gallery keeps only what an
  eye can judge (is it ugly), and the discovery loop: a human spots something,
  and the fix is a **check**, which from then on runs against everything
  forever.
- **A registry (`ui_element.rs`), so checks and elements compound.** Elements
  spawned ad hoc inside tests never meet the checks written later. One list
  means a new check retroactively covers every element, and a new element
  inherits the whole suite. That is the property that makes the suite grow
  rather than rot.
- **Behaviour, not just the resting state.** Rendering checks cannot see a
  submenu that opens on hover. The harness drives activation and tab navigation
  through the real `bevy_ui_widgets` / `bevy_input_focus` paths and asserts on
  what happened.
- **Three tiers, because one rule does not fit:**

  | Tier | Question | Who decides |
  | --- | --- | --- |
  | Universal | is this broken? | the harness: content fits its box, box fits its parent, nothing off screen, no text sliced |
  | Declared | does it match its stated intent? | the element: `AlignmentGroup` (the build window's X/Y/Z columns must stay straight however the row labels translate), `TextMayClip` |
  | Baseline | has it moved? | [[viewer-ui-baseline-regressions]] — identified here, deliberately not built here |

  The *declared* tier exists because nothing in a tree says whether two boxes
  ought to line up, and guessing produces noise. The `TextMayClip` opt-out
  exists for the same reason in reverse: a single-line field scrolling past its
  end is *supposed* to be half-clipped, and a check that cries wolf gets
  deleted, taking its real findings with it.

### The task's one real unknown resolved the other way

The premise — `bevy_ui`'s headless layout is `pub(crate)`, so this may need
**upstreaming a harness to Bevy** — is **wrong for 0.19**. `ui_layout_system`,
`propagate_ui_target_cameras`, `UiSurface`, `measure_text_system` and the
`bevy_transform` systems are all `pub`. No fork, no `[patch.crates-io]`, no PR:
`ui_test.rs` is ordinary downstream code. (Bevy's own harness omits
`measure_text_system` because none of its fixtures carry text; ours cannot,
since text measurement is the thing most worth testing.)

### It found real bugs immediately, which is the point

- **`viewer-text-node-padding-measure` reproduces headlessly** in ~0.2 s, where
  diagnosing it by hand cost a login, a debug key and six rounds of a human
  reporting numbers. The test asserts the bug is *still present*, so it doubles
  as the canary for the upstream fix.
- **That bug is bigger than its title.** The matrix showed the measure loses
  *anything* narrowing a text node's width other than its parent's own padding —
  a container's border, a sibling — so the documented workaround is
  insufficient. It is ≈0.23 × font size, and it does **not** accumulate with
  nesting (ruling out pixel rounding). All written up in that bug's file; it is
  why `OVERFLOW_EPSILON` is 6 logical px and not 1.
- **The `label` element had no width bound**, so CJK and bidi ran 1101 and 1165
  px off the edge of the window. Invisible in English, which fits by luck.
- **The pseudolocaliser padded with an unbreakable run**, so its own output
  could not wrap — a false positive that would have trained everyone to ignore
  the real ones.

### Also landed

- The viewer crate is now a **library with two thin binaries** (`src/lib.rs`,
  `src/main.rs`, `src/bin/…-gallery.rs`). Two binaries cannot share a
  `pub(crate)` module tree; only a library can. `#[path]`-including the UI
  modules into a second binary would compile them twice and trip `dead_code` on
  everything either binary happened not to use.
- **`ui_pseudoloc.rs`** — accents, ~140% expansion, `⟦…⟧` fences so truncation
  is visible in a language you cannot read. [[viewer-i18n-fluent-scaffold]]
  should take it over as a pseudo-*locale* applied at the Fluent lookup, rather
  than per-call-site.
- The **no-wiring rule is mechanised, not just written down**: an element emits
  a `UiAction` message and never touches a `Session`. The viewer routes those to
  real handlers, the gallery routes them nowhere (so a click is inert
  *by construction*, not by stubbing), and a test reads the queue to assert what
  a click meant. That is what makes behaviour assertable at all.

[[viewer-render-test-harness]] is the 3D counterpart, and should follow this
shape rather than invent its own.
