---
id: test-inventory-cache-skip
title: refetch with matching version is skipped
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 5 — Inventory (deep) `[both]`
---

Context: [context/test.md](../context/test.md).

`inventory-cache-skip` — refetch with matching version is skipped. `1av`.
    Where the other Phase 5 cases prove the inventory tree can be *fetched*
    and *mutated*, this proves it need not be fetched **again**: the runtime's
    inventory disk cache lets a relogin restore version-unchanged folders
    straight from `<agent-uuid>.inv.llsd.gz` instead of refetching them. The
    runtime loads the cache before the login skeleton and reconciles it
    (Firestorm's `loadSkeleton`): a cached folder whose version equals the
    skeleton's keeps its loaded contents (`FolderState::Loaded`); a mismatch
    is invalidated and requeued. The case drives the cache directory through
    new harness support (a cleared per-case, per-grid dir under the gitignored
    `.sl-conformance/`, opted into by a `GridTest::inventory_cache()` hook, so
    the first login is genuinely cold) and the mid-run
    `Session::disconnect`/`relogin` cycle `offline-msg-fetch` introduced
    (disconnect writes the cache on logout; relogin reads it back). It asserts
    the version-matching skip directly from the held model via
    `Command::QueryInventoryFolder`: the agent root's child folders are
    **`Unknown`** before the crawl (cold — nothing loaded) and
    **`Loaded` at the same version** after the relogin (warm), with no refetch
    issued that session. The cache load/merge keys only on the login skeleton
    (which both grids send), so it is a single `[both]` path; only the
    underlying per-folder crawl picks CAPS vs UDP per region. Green on
    OpenSim: all 24 agent-root child folders went cold-`Unknown` →
    warm-`Loaded` at the identical version (24/24 version-matched, 29 folders
    cached), crawl ≈ 2.9 s, relogin ≈ 5.1 s loopback. `[both]`; the aditi run
    is deferred with the batch (no aditi record this session).
