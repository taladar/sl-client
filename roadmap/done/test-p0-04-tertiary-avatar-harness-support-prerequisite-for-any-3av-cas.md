---
id: test-p0-04
title: Tertiary-avatar harness support** (prerequisite for any 3av case):
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 0 — Test utilities & helpers (do first)
---

Context: [context/test.md](../context/test.md).

**Tertiary-avatar harness support** (prerequisite for any `3av` case):
a `--tertiary` resolver mirroring `resolve_secondary`, a `ctx.tertiary()`
accessor, a third Aditi cooldown guard, and bumping `accounts()` handling to
accept `3`. Two-avatar plumbing already exists in
`sl-conformance/src/context.rs` (`accounts()` + `--secondary`); this extends
it. OpenSim `3av` cases can run as soon as this lands; Aditi `3av` waits on a
3rd Aditi avatar (Phase Z). (Resolver picks an avatar distinct from both
primary and secondary; conventional credentials label `tertiary`.)

---
