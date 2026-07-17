---
id: viewer-render-baselines
title: Render regression baselines — recorded geometry that may not drift by accident
topic: viewer
status: blocked
origin: the viewer-render-test-harness work (2026-07); the third tier that task identified but did not build
blocked_by: [viewer-render-test-harness, viewer-ui-baseline-regressions]
refs: [viewer-ui-baseline-regressions, viewer-render-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The third tier of the render check suite. [[viewer-render-test-harness]] built
two:

| Tier | Question | Who decides |
| --- | --- | --- |
| **Universal** | is this broken? | the harness, for every scene, no opt-in |
| **Declared** | does it match its stated intent? | the scene (`DeclaredBounds`, `SymmetricAbout`) |
| **Baseline** | *has this moved?* | **a committed recording — this task** |

The first two only catch what is **wrong**. The gap is the same one
[[viewer-ui-baseline-regressions]] describes, in 3D: properties that are not
wrong at any particular value but must not change *by accident*.

For geometry that means the vertex and triangle count a shape tessellates to at
each LOD, its bounding box, its centroid; an attachment point's offset; a
joint's rest pose; the camera's default framing. Nothing is incorrect if a box
comes out at 26 vertices instead of 24 — and if a refactor changes it, somebody
should have to say so.

## Share the UI tier's format — do not grow a second

[[viewer-ui-baseline-regressions]] says this explicitly and it is the whole
reason this task is blocked on it: "[[viewer-render-test-harness]] needs the
same tier for 3D and should share this one's format rather than growing a
second." Two baseline formats, two re-bless flows and two staleness reports for
one idea would be strictly worse than waiting.

`records/` + `sl-conformance-report` is the prior art for the *shape* (a
committed, git-stamped recording, compared against the current commit, refreshed
deliberately) and should not be reinvented either.

## The design constraints, inherited

- **Opt-in, not universal.** Baselining every scene at every LOD would make
  every deliberate tessellation change a noisy diff, and a noisy check gets
  ignored and then deleted.
- **Record derived intent, not raw dumps.** A vertex-position dump changes
  whenever a float does and teaches everyone to re-bless without reading. Record
  counts, extents, angles.
- **One canonical cell, not the matrix.** The harness sweeps every scene × every
  LOD × every timeline sample; baseline the resting cell and let the universal
  tier cover the rest.
- **The value is the review moment.** "This diff changed the box's vertex count"
  is a sentence somebody can object to. That is the entire point.
