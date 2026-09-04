---
id: viewer-audit-name-tag-viewport-gate
title: The name-tag viewport-changed gate is exactly inverted
topic: viewer
status: done
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

## Outcome (2026-09-04): the gate is a named function, and it is latent

The comparison is now `!=`, split out of the system as
`viewport_size_changed(last, current)` — small enough to test without standing
up the text pipeline, and a place to record *why* it diverges from the port's
source. Pinned by `viewport_gate_reports_change_not_sameness`, which fails
against the old expression in both directions (steady state and the resize
frame).

The inversion is upstream's, not ours: bevy_sprite 0.19 `text2d.rs:198` reads
`*last_logical_viewport_size == logical_viewport_size` and feeds it to a
parameter named `is_viewport_size_changed`. Nothing else in the workspace uses
`Text2d`, so the fork is left alone.

One correction to the finding as filed: the per-frame reshape it predicts
cannot happen **today**. `needs_rerender` ANDs the flag with
`uses_viewport_sizes`, which `TextPipeline::update_buffer` sets only for a
`FontSize::Vw`/`Vh`/`VMin`/`VMax` span, and every tag span is built by
`UiFont::at`, which is `FontSize::Px`. So the defect was latent — armed for
whoever first writes a viewport-relative tag size — and there is no measurable
frame-time change to show for the fix.
