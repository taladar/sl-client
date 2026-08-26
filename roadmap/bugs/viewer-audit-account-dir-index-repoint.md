---
id: viewer-audit-account-dir-index-repoint
title: A skipped rename still repoints the UUID index, handing one avatar another's data
topic: viewer
status: bugs
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
