---
id: viewer-audit-account-dir-index-repoint
title: A skipped rename still repoints the UUID index, handing one avatar another's data
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-account-dirs/src/lib.rs:105-110` — on a discovered rename:

```text
if previous_dir.exists() && !name_dir.exists() {
    fs_err::rename(&previous_dir, &name_dir)?;
}
write_index(&index_entry, name)?;
```

`write_index` sits **outside** the guard. When `name_dir` already exists — i.e.
another avatar already has a directory under that name — the directory move is
skipped but the UUID index is repointed anyway.

The comment says "never clobber existing data". The files are not clobbered; the
**wrong files are handed to the wrong avatar** — settings, chat logs and the
inventory cache — and the original avatar's data is orphaned.

Fix: move `write_index` inside the guard, and surface the collision rather than
silently proceeding. That branch has no test.

## The fix (2026-09-14)

The rule is now **the index is repointed only when the avatar really ends up
under the new name**, and the guard is the one that matters: not "does the
destination directory exist" alone, but "is that name taken at all".

- A name is taken when a directory of that name exists **or** another UUID's
  reverse-index entry claims it (`name_claimed_by_other`, a scan of
  `.by-uuid/`). The second half is not redundant: an index entry outlives a
  hand-deleted directory, and renaming into that name would have handed *this*
  avatar's data to the claimant at its next login — the same bug one step
  later.
- On a collision nothing moves and nothing is repointed. The avatar keeps its
  own directory under the former name, so its settings, chat logs and inventory
  cache stay its own and no other avatar's are handed to it.
- The former directory being gone is no longer conflated with a collision: with
  the name free there is simply nothing to migrate, and only the index moves.

`reconcile_account_dir` now returns an `AccountDir { path, outcome }` rather
than a bare `PathBuf`, because a collision that only a human can resolve must
not be silent. `Reconciliation` is `Settled` / `Renamed { previous }` /
`NameTaken { previous }`, and `Reconciliation::collision_warning` words the
report once for all three callers (`sl-viewer-settings`, `sl-client-bevy`,
`sl-repl-tokio`), each prefixing it with its own subsystem.

## Verified

Three new unit tests in `sl-account-dirs`, all passing (8 total): a rename into
a name another avatar occupies keeps both avatars' files with their own owners;
a name claimed only by a surviving index entry is refused just the same; and a
rename whose former directory was deleted by hand still repoints the index.
