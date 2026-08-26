---
id: viewer-audit-name-tag-viewport-gate
title: The name-tag viewport-changed gate is exactly inverted
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-objects/src/name_tag_billboard.rs:530` —

```text
let viewport_size_changed = *last_logical_viewport_size == logical_viewport_size;
```

It computes *unchanged*. The value is then fed to
`computed.needs_rerender(viewport_size_changed, ...)`, which ANDs it with
`uses_viewport_sizes` (bevy_text 0.19 `text.rs:95`).

Net effect, wrong in both directions: in steady state every viewport-unit name
tag reshapes **every frame** (bounded only by `TagLayoutBudget`), and on the
actual resize frame it does **not** reshape.
