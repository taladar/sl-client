---
id: test-audit-conformance-runner-isolation
title: A panicking or hung conformance case aborts the run and strands the avatar
topic: test
status: done
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

## Resolution

**The body is isolated.** `sl-conformance/src/isolate.rs` polls the case future
inside `catch_unwind` and under an overall `tokio::time::timeout`, so a panic
becomes `TestFailure::Panic` and a hang becomes `TestFailure::Timeout` — an
ordinary failure the runner records, and logs the avatars out around, instead of
an abort that strands them. The budget is `GridTest::timeout()` (default 15 min,
far above the slowest case's own 4-minute internal wait) and `--timeout <secs>`
overrides it. The failure reason now also reaches the `FAIL:` line and the log,
not the committed record — a message can quote grid content.

**The parcel case cleans up on both kinds of exit.** The divide-verify-join flow
moved into `exercise()`, run under an awaited whole-region join covering every
path that *returns*, plus a `RestoreOnDrop` guard that covers the paths that
never return (a cancelled body, an unwind). `Drop` cannot await, so the guard
queues the join through a new `Session::commander()` handle — the run loop is
still up and transmits it ahead of the logout. Live-verified on OpenSim: a
deliberate `--timeout 7` cancels mid-flight, the guard logs and queues, and the
region DB reads back one 65536 m² parcel.

**Orphan groups are named and dropped.** `membership_group` only retries when a
reply is slow, so after a retry it now watches 10 s for the *late* reply, treats
any group other than the winner as an orphan, asks to leave it (a group its
founder has left drops to zero members and the grid purges it) and records it as
`orphan_group_count` / `orphan_groups` plus a warning naming the ids. The watch
consumes events, so it runs only on the retry path. This path needs a slow SL
reply to trigger and has not been observed live; the OpenSim group cases confirm
the non-retry path is unchanged.
