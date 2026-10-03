---
id: gridspec-object-edit
title: Object edits on each grid: transforms, shape, flags, admin fields, undo, duplicate
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-build-phantom-flag-wrong-bit,
  viewer-edit-permission-gating, viewer-build-general-sale-clickaction]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

The fake grid applies every edit with no permission check; its undo
(depth 10, whole snapshots, properties too) diverges from OpenSim's
transform-only undo. `server-fake-grid-object-undo` predates the
implementation. Duplicate does not copy contents; `DuplicateObjectsOnRay`
is unhandled.

## Discover

Run `object-edit` on aditi; add legs for clamps (scale, position off-region),
non-owner refusal (second avatar), next-owner mask clamping, name / desc
truncation, deed without power, undo scope and depth, duplicate (contents,
ownership) and duplicate-on-ray.

## Document

`book/src/gridspec/building.md` § Editing.

## Fake grid

Small in this task: clamps, refusal silence, mask clamping, undo scope per
flavour, duplicate contents and on-ray. Permission enforcement:
[[server-fake-grid-edit-permission-enforcement]].

## Viewer

Read back clamped values; converge after a corrective update; `e2e` edits on
both flavours.
