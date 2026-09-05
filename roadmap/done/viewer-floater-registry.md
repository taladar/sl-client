---
id: viewer-floater-registry
title: A FLOATERS registry, so floaters can be swept at all
topic: viewer
status: done
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 3
refs: [viewer-ui-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

Floaters are opened imperatively through `FloaterCommand`/`FloaterOp` and
`spawn_floater(FloaterSpec)` — there is no central list, so no sweep or
gallery can reach them. The `ELEMENTS` registry was built precisely to
prevent this gap for panels and widgets; floaters need the same treatment.

Add a `FLOATERS` const registry (mirror of `ELEMENTS`: id + spawn fn
returning a `FloaterHandle`, with stub content where the real content
needs a session), wire it into the gallery, and write the rule down beside
the elements rule: **every floater registers**.

Independent of the pointer harness — the layout matrix in `ui_test.rs` can
sweep registered floaters immediately (viewport/containment/clipping per
matrix cell), which is value on its own before any interaction tests
exist.
