---
id: viewer-audit-notification-store-overwrite
title: A malformed notification store is overwritten, destroying unanswered notices
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-settings-write-race]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-notices/src/notification_persist.rs:236` — `read_store` warns and
returns `Vec::new()` on a parse failure. `load_persisted` then sets
`store.path`, and the first later `flush_persistent_notifications` rewrites the
file from the now-empty map. A parse error is silently downgraded into **data
loss** of every unanswered notification.

Same file, `:265` — the store is written with a whole-file `fs_err::write` on
the **main thread inside `Update`** every time `dirty` flips. No `save_async`
equivalent, and the same no-read-modify-write clobber hazard as
[[viewer-audit-settings-write-race]].

Fix: on a parse failure, refuse to write rather than overwriting (rename the bad
file aside), and move the write off the frame thread with the atomic
temp-and-rename helper. The 2 existing tests cover record/forget and a JSON
round-trip; the reload path — a `PersistedKind::Catalogue` naming a template
that no longer exists, and whether surviving entries are still written back — is
the untested case.

## Done

`read_store` returns a `StoreFile` — `Absent` / `Loaded` / `Unreadable` — rather
than a `Vec`, so "there is no file" and "I could not parse it" can no longer be
confused. `load_persisted` sets `store.path` only for the first two; an
unreadable file goes through `rescue_unreadable_store`, which renames it aside
(timestamped, so a second corruption does not destroy the first's evidence) and
only then licenses writing. If even the rename fails the session gets **no
path**, so nothing can overwrite it — refusing to write is the last remaining
way not to lose the data.

The write is now `sl_settings::atomic_file::write_atomically` on the
`IoTaskPool`, with **at most one in flight**: a flush that finds the previous
write running returns and leaves `dirty` set, so writes serialize and a burst of
changes coalesces into one. That is deliberately not `save_async`'s
spawn-and-detach — two detached writes have no ordering guarantee, which is half
of what [[viewer-audit-settings-write-race]] records.

### The helper this needed

The sibling task asks for "an atomic temp-and-rename helper in `sl-settings`"
and notes there is none anywhere. It exists now, as
`sl-settings/src/atomic_file.rs`:

- `write_atomically` — sibling temp file, `sync_all`, rename over the target,
  plus a best-effort directory sync on Unix so the rename itself is durable. The
  temp is a *sibling* because a rename is only atomic within one filesystem, and
  the system temp dir is routinely a different mount. On any failure the temp is
  removed and the target is untouched, which is the whole point over
  `fs_err::write`'s `O_TRUNC`.
- `move_aside` — the other half: preserving a file that could not be parsed, so
  a later flush cannot reach it.

That is more than this task strictly needed, but the helper belongs in the crate
the sibling names, and putting it there means
[[viewer-audit-settings-write-race]] now has its first bullet already built
rather than a duplicate to reconcile. It
does **not** address that task's other two defects (unordered detached saves
across ten call sites, and the exit save happening at `quit_deadline` rather
than at exit), which stay open.

### Tests

`sl-settings`, 3 new: a write replaces the file and leaves no temporary; a
*failed* write leaves the original intact (driven by renaming over a non-empty
directory, so the failure is guaranteed and lands after the temp is written);
and moving aside twice preserves both copies.

`sl-viewer-notices`, 3 new: every `read_store` outcome including both legitimate
empties (`Absent`, and a file holding `[]`) so a future simplification back to a
bare `Vec` fails rather than silently restoring the data loss; both outcomes of
the rescue, asserted as the presence or absence of the path every later flush
consults; and the reload path, where a `Catalogue` naming a template that no
longer exists is dropped **without** taking the entries after it with it.
