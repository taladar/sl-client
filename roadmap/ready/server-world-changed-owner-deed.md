---
id: server-world-changed-owner-deed
title: Measure and raise CHANGED_OWNER when an object is deeded to a group
topic: server
status: ready
origin: server-world-changed-raisers (2026-11-03), left unmeasured
refs: [server-world-changed-raisers, server-fake-grid-edit-permission-enforcement,
  server-fake-grid-money-ledger]
blocked_by: [server-fake-grid-groups]
---

Context: [context/server.md](../context/server.md).

## What

[[server-world-changed-raisers]] measured `changed` for every viewer-side
cause it could reach, except an owner change by a deed to a group: the aditi
test accounts are in no group, so the probe could not deed. A purchase's
`CHANGED_OWNER` is measured (once on Second Life, three times on OpenSim) and
raised on the fake grid since [[server-fake-grid-money-ledger]] (2026-11-06).
A deed has not been measured.

## Scope

- **Measure.** Add a probe (for example `events/changed-deed`) and a runner
  drive that sets the object's group and deeds it (`ObjectGroup`, then
  `ObjectOwner` / the build floater's Deed, as a viewer sends them). The
  probe should record:
  - which bits the object's scripts hear, how many times, and in which prims
    of a linkset;
  - whether `llGetOwner()` is the group key straight after;
  - whether the scripts keep running, reset or lose their permissions.

  Run it on aditi (an avatar needs a group with the deed power; set one up
  and record the setup in the task), YEngine and XEngine (the local
  OpenSim's Groups V2 module).
- **Write it down** in `book/src/gridspec/lsl.md`, in the owner-changes table
  of § `changed`: what a viewer's change raises.
- **Raise it on the fake grid.** The fake grid holds groups
  (`sl-fake-grid/src/groups.rs`), but its object edits do not consult them
  yet. Deeding by a member with the power belongs to
  [[server-fake-grid-edit-permission-enforcement]]. This task builds on
  that, or does the deed leg itself if it gets here first. Raise
  `CHANGED_OWNER` as each grid does, kept in `ImitatedGrid`'s
  `ChangePolicy` where the grids disagree, and hold it with a
  `tests/scripts.rs` case on both flavours.
