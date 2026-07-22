---
id: viewer-script-mirror-upload-watch
title: Scripts on disk — file watch, headless upload, three-way sync
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-script-external-workflow
blocked_by: [viewer-script-mirror-download]
refs: [viewer-lsl-semantic-pass]
---

Context: [context/viewer.md](../context/viewer.md).

Close the loop on the on-disk script mirror ([[viewer-script-mirror-download]]):
watch the tree for edits, push changes back to the grid headlessly, and detect
conflicts — the thing no existing tool does.

**The one hard fact the design must respect: upload *is* the compile — there is
no dry run.** The cap body carries only `{item_id, target}` (or `{task_id,
item_id, is_script_running, target, experience}`); OpenSim stores the asset
*before* compiling it. So every grid-side diagnostic costs an **asset write and
a script restart** — a `:w` in nvim must not silently restart a live vendor's
payment script. Consequences baked into the design: **upload on save is
opt-in**, local checking ([[viewer-lsl-semantic-pass]]) carries the fast
feedback loop, grid diagnostics arrive only on an explicit push, and a headless
push must carry the current `is_script_running` state or it will start or stop
scripts as a side effect. (For version-control workflows the porcelain
`sl-script status` of [[viewer-script-mirror-download]] is the CI/hook
surface; pushes stay explicit.)

**Sync as a three-way state** (`disk_hash`, `last_synced_hash`,
`grid_asset_id`): grid→disk overwrites only a *clean* file, and on a dirty one
writes the incoming
version beside it and marks the script conflicted. Suppress our own write echoes
by **content hash, not an ignore-next-update flag** (that flag is exactly what
produced Firestorm's "script must be saved twice" bug). And **never delete the
user's files**.

**File watching has one classic trap:** editors that save by **atomic rename**
(vim, VS Code) orphan an inode watch, which then goes permanently deaf. So
**watch the directory, not the file**, debounce (`notify-debouncer-full`), treat
every event as a *hint*, and re-read and hash rather than trusting the event
kind.

**A headless uploader is genuinely new.** No CLI anywhere uploads LSL to the
grid — every existing tool needs a running viewer with the object rezzed and the
floater open. We already have the session, the caps, inventory and structured
compile errors, so an `sl-repl`-driven `pull` / `push` / `status` / `watch` is
mostly plumbing — and it makes **LSL in CI possible for the first time**. Refuse
to push **no-modify scripts** rather than letting an edit fail at the grid.

Optional interop: Linden Lab's `sl-vscode-plugin` (MIT, 2026) routes edits
through the viewer's temp dir over JSON-RPC — so our mirror *is* their temp dir.
Interop, not competition.

Reference (Firestorm, read-only): `llexternaleditor`, `llpreviewscript`
(`LLLiveLSLFile`, `onExternalChange`), `lllivefile`, `fslslpreproc` (the on-disk
`#include` path).
