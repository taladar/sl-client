---
id: test-audit-conformance-runner-isolation
title: A panicking or hung conformance case aborts the run and strands the avatar
topic: test
status: bugs
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/test.md](../context/test.md).

`sl-conformance/src/bin/sl-conformance.rs:331` — `test.run(&mut ctx).await` has
**no `catch_unwind` and no overall timeout**.

A panic in a case aborts the process before `write_record` (`:350`) and before
the three `logout()` calls (`:336-348`), so no record is written **and** the
avatar is left logged in — which is exactly the stale presence
`context.rs:534-548` then has to retry around on the next run. A hung case hangs
the runner forever.

Two related state-leak issues:

- `src/cases/parcel_divide_join.rs:230-240` — the region is divided at step 2
  and only rejoined at step 3. Every `check_eq` / `?` between them returns
  early, leaving the region **permanently divided**, so the next run's
  initial-area assertion fails against a region the previous run broke. No
  `Drop` guard, no cleanup path.
- `src/support.rs:252-293` — `membership_group` retries `CreateGroup` up to
  three times with a suffixed name, and the doc comment itself admits *"A retry
  after a merely-slow first reply can leave an orphan single-member group"*. On
  aditi that is L$100 per orphan plus founder group-slot churn, and the case
  never deletes them.

Scope: wrap the case body so a panic or timeout fails **one** case, still writes
its record and still logs out; and give a case that mutates grid state a cleanup
guard that runs on the failure path.
