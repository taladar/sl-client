---
id: viewer-audit-object-children-index
title: ObjectState has no children index, so every linkset query full-scans the region
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-world-api-query-tests]
---

Context: [context/viewer.md](../context/viewer.md).

`TrackedObject` (`sl-viewer-world-api/src/lib.rs:4612`) carries `parent` but no
`children`, so finding an object's children means scanning the whole map:

- `linkset_members` (`:4992`) is `self.objects.iter().filter(...)` over every
  tracked object;
- `tracked_descendants` (`:4874`) rescans per frontier node, making
  `remove_object` on a D-prim linkset O(D x N).

The compound case is avatar complexity
(`sl-viewer-world-avatar/src/avatar_complexity.rs:1330`, `:1345`): per rescore
pass, one full-map `attachment_roots_by_wearer()` scan allocating a
`HashMap<_, Vec<_>>`, then for up to `RESCORE_BUDGET = 4` agents,
`linkset_members` is called **once per worn attachment root** — one full N-scan
plus a sort plus a `Vec` allocation each. Four avatars with 30 attachments over
a 20k-object region is roughly 2.4M map iterations per frame while anything is
dirty, and `sweep_jellied` (`:1505`) repeats it crowd-wide at 2 Hz.

Scope: add `children: HashMap<ScopedObjectId, Vec<ScopedObjectId>>` to
`ObjectState`, maintained alongside `parent`. That fixes the worst hot path and
the `KillObject` quadratic together. All the affected query methods are pure
over the map and currently untested — see
[[viewer-audit-world-api-query-tests]].
