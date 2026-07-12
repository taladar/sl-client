---
id: viewer-p12-5
title: Tests
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 12 — `sl-avatar`: skeleton & base body (pure crate)
---

Context: [context/viewer.md](../context/viewer.md).

**P12.5. Tests.** Skeleton hierarchy + attachment/HUD point maps; `.llm`
decode non-degenerate counts + weight normalization; param-table lookups and
byte→value dequantization. `cargo test -p sl-avatar`. **Done:** the P12.2–
P12.4 modules each already ship their own `#[cfg(test)]` unit tests over the
private surface; this adds `tests/avatar.rs`, an *integration* suite that
drives only the re-exported public API (`sl_avatar::*`) an external consumer
sees and asserts the structural invariants the three bullets call out rather
than fixed fixture values: the skeleton is a coherent tree (single parentless
root, every parent index precedes its child, each child listed once under its
parent) with round-tripping name/alias lookups; the attachment map, per-point
`is_hud`, `hud_points()`, and the wire enum's own `AttachmentPoint::is_hud`
all agree, and a shared joint (`mChest`) proves the cross-asset lad→skeleton
reference resolves; the base `.llm` has non-degenerate counts with every
per-vertex stream one-entry-per-vertex, all face / morph-delta / shared-vertex
indices in range, one skin weight per vertex whose joint indexes the mesh's
own joint table and whose blend is normalized to `[0, 1)` (the last joint
never blends past the table), and a reduced LOD whose `vertex_count` is
exactly its max referenced index + 1; the param table is strictly id-sorted
with id lookups round-tripping, `transmitted()` is exactly the wire-carrying
groups (length matching `transmitted_count()`, complement covering the rest),
and a full appearance vector dequantizes so that `AppearanceValues::weight`
matches each param's own `weight_from_byte` slot-for-slot and stays within the
param's min/max, with empty / short vectors falling back to defaults and
recording no raw byte. The `clippy::tests_outside_test_module` restriction
lint applies to `tests/` targets too, so the suite lives in a `#[cfg(test)]
mod tests`. 10 integration tests (21 unit + 10 = 31 total green).
