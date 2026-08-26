---
id: protocol-audit-asset-store-duplication
title: Three hand-rolled copies of the same asset store
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/protocol.md](../context/protocol.md).

`sl-asset`, `sl-texture` and `sl-mesh` each ship `store.rs` + `disk.rs` +
`entry.rs` + `fetcher.rs` + `progress.rs` (334/299/102/73/32,
987/636/289/154/252 and 920/464/245/37/252 lines respectively), implementing the
same weak-reference `DashMap<_, Weak<Entry>>` cache, single-flight fetch,
Firestorm-shaped on-disk cache with LRU purge, and progress enum.

The variation between them is real — LOD in two of the three, a progressive
codestream in one — but the purge / index / single-flight halves are
near-identical. `sl-asset-sched` already exists as the shared scheduling piece;
the store/disk/LRU skeleton is the obvious next extraction.

Note `sl-asset` has **3 tests for ~840 lines** of store plus disk cache plus LRU
purge. Nothing covers the disk-cache header/entry byte-layout round trip, LRU
eviction ordering, or single-flight de-duplication under concurrency — all three
are pure in-memory tests (`pollster` is already a dev-dependency), and all three
become one test suite once the skeleton is shared.

`sl-asset-sched/src/gate.rs` has the same shape of gap: its three headline
guarantees are untested. `set_priority` (`:95`) and `remove` (`:106`) have zero
coverage anywhere; `gate_admits_up_to_capacity_then_serialises` (`:186`) drops
the first permit before acquiring the second, so it never exercises contention,
never reaches the `listener.await` path (`:90`) and never checks that the
highest-priority waiter wins; and `WaiterCleanup` (`:162-177`) — the "a
cancelled waiter never starves another" promise from the module doc — has no
test that drops an unresolved `acquire` future.
