---
id: viewer-script-external-workflow
title: Scripts on disk — external editors, git and a headless uploader
topic: viewer
status: ideas
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Mirror grid scripts to a **real directory on disk**, so nvim / VS Code / ripgrep
/ git / CI just work. Deliberately **not** blocked on the UI framework: it needs
inventory and the upload caps, both of which already exist, so it can land long
before the in-viewer editor ([[viewer-lsl-script-editor]]) — and for a power
user it is probably worth more.

## What the reference actually does, and why it is not enough

Firestorm's external editor is **one ephemeral temp file per open floater**:
`$TMPDIR/sl_script_<name>_<md5(objectUUID_itemUUID)>.lsl`, watched by a **1 Hz
`stat()` poll** with second-granularity mtime, **deleted when you close the
floater**. The editor must be an absolute path to a binary with a `%s`
placeholder (so plain `nvim` on `$PATH` does not work). Compile errors never
reach the editor. There is **no directory sync**.

And, most importantly: **there is no conflict detection at all.** The dirty
model is a text-editor pristine flag; the script is fetched once on open and
every save uploads unconditionally. If a co-builder edits the in-world script
while your floater is open, your next save **silently clobbers them** and
neither of you is told.

The community works around all this today by pointing the "external editor"
setting at a *shell script* that rewrites the temp file from a build directory.
That is the state of the art, and it is a hack.

## The one hard fact the design must respect

**Upload *is* the compile. There is no dry run.** The cap body carries only
`{item_id, target}` (or `{task_id, item_id, is_script_running, target,
experience}`); OpenSim stores the asset *before* compiling it. So every
grid-side diagnostic costs an **asset write and a script restart**. A `:w` in
nvim must not silently restart a live vendor's payment script.

Consequences, baked into the design rather than discovered later: **upload on
save is opt-in**, local checking ([[viewer-lsl-parser]]) carries the fast
feedback loop, and grid diagnostics arrive only on an explicit push.

## Shape

A human- and git-legible tree, with the ugly identity mapping in a sidecar:

- `inventory/<folder path>/<Script Name>.lsl` — agent-inventory scripts,
  mirroring the user's own folder structure.
- `objects/<Object Name>-<key prefix>/<Script Name>.lsl` — task inventory.
  Object keys change when an object is taken and re-rezzed, so they must **not**
  be the path; keep the authoritative `(object_key, item_id, asset_id)` triple
  in a committable `manifest.toml`.
- `manifest.toml` is the interesting artefact: a diffable, committable record of
  **which asset id each file was last uploaded as** — which is exactly the
  "version the asset UUID alongside the source" feature, achieved with no git
  integration at all, because the user's own `git commit` picks it up.

**Sync as a three-way state** (`disk_hash`, `last_synced_hash`,
`grid_asset_id`): grid→disk overwrites only a *clean* file, and on a dirty one
writes the incoming version beside it and marks the script conflicted — the
thing no existing tool does. Suppress our own write echoes by
**content hash, not an ignore-next-update flag** (that flag is exactly what
produced Firestorm's "script must be saved twice" bug). And never delete the
user's files.

File watching has one classic trap: editors that save by **atomic rename**
(vim, VS Code) orphan an inode watch, which then goes permanently deaf. So
**watch the directory, not the file**, debounce (`notify-debouncer-full`), treat
every event as a *hint*, and re-read and hash rather than trusting the event
kind.

## Git: stay out of the way

**Do not embed a git library.** If the scripts are real files, the user's git is
better than anything we would wrap, and a viewer that auto-commits on every
upload produces garbage history and fights the staging area. What we uniquely
*can* report is **grid-vs-disk** drift (`sl-script status`) — the state git
cannot see. Leave disk-vs-HEAD to git.

## Two things nobody has ever shipped

- **A headless uploader.** No CLI anywhere uploads LSL to the grid — every
  existing tool needs a running viewer with the object rezzed and the floater
  open. We already have the session, the caps, inventory and structured compile
  errors, so `sl-repl`-driven `pull` / `push` / `status` / `watch` is mostly
  plumbing — and it makes **LSL in CI possible for the first time**.
- **Speaking Linden Lab's editor protocol.** LL shipped `sl-vscode-plugin` (MIT)
  in 2026: **JSON-RPC 2.0 over WebSocket, port 9020, with the *viewer* dialling
  out to the editor** — `session.handshake`, `script.list`, `script.compiled`
  (carrying row/column diagnostics), and `runtime.debug` / `runtime.error`, i.e.
  live `llOwnerSay` output and runtime stack traces streamed into the editor.
  Firestorm has not adopted it. Implementing the viewer half is cheap, it is a
  published spec, and it hands every existing VS Code user our viewer for free.
  Their protocol still routes edits through the viewer's temp dir — so our
  mirror *is* their temp dir. Interop, not competition.

Also plan for: **no-modify scripts** (mark read-only on disk and refuse to push,
rather than letting someone edit a file that can never go back), and
`is_script_running`, which round-trips through the upload body — a headless push
must carry the current run state or it will start or stop scripts as a side
effect.

Reference (Firestorm, read-only): `llexternaleditor`, `llpreviewscript`
(`LLLiveLSLFile`, `getTmpFileName`, `onExternalChange`), `lllivefile`,
`fslslpreproc` (the on-disk `#include` path — the one place power users already
hang git off today).

Refs: [[viewer-lsl-language-server]] (shares the file↔inventory mapping),
[[viewer-lsl-parser]] (the fast local feedback loop this design depends on).
