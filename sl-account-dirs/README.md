# sl-account-dirs

Per-avatar on-disk directory layout for a Second Life / OpenSim client.

An avatar identity is **(grid, name)** — the same avatar name on SL's Agni, the
Aditi beta grid, and an OpenSim grid are three different avatars, and Aditi is
periodically cloned from Agni so the agent UUID alone is not grid-unique. The
grid must therefore always appear in the path.

The per-avatar directory is keyed by **name** (readable, and known before
login), with the agent **UUID** recorded as a reverse-index symlink so a paid
Linden name change is *discovered* and the readable directory renamed, rather
than the old data orphaned:

```text
<base>/<grid>/<name>/                    the per-avatar directory (canonical)
<base>/<grid>/.by-uuid/<uuid> -> <name>  reverse index, for rename discovery
```

A name change is only carried out when the new name is free. If a directory of
that name already exists, or another avatar's index entry claims it, the rename
is abandoned: the avatar keeps its own data under its former name and the
returned `Reconciliation::NameTaken` tells the caller to report the collision.
Repointing the index anyway would hand this avatar the *other* avatar's
settings, chat logs and inventory cache.

The crate is I/O only (path derivation + one idempotent
`reconcile_account_dir`); it does not choose the base directory — the host
application supplies that (e.g. an XDG data dir via the `directories` crate).
