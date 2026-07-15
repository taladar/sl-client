---
id: viewer-script-mirror-download
title: Scripts on disk — mirror grid scripts to a directory tree
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-script-external-workflow
refs: [viewer-lsl-lsp-server]
---

Context: [context/viewer.md](../context/viewer.md).

Mirror grid scripts to a **real directory on disk**, so nvim / VS Code / ripgrep
/ git / CI just work. Deliberately **not** blocked on the UI framework: it needs
inventory and the asset caps, both of which already exist, so it can land long
before the in-viewer editor — and for a power user it is probably worth more.
This task is the read-only half: enumerate agent and task inventory scripts and
write them out; the watch/upload/conflict half is
[[viewer-script-mirror-upload-watch]].

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

**Stay out of git's way.** Do not embed a git library — if the scripts are real
files, the user's git is better than anything we would wrap. What we uniquely
*can* report is **grid-vs-disk** drift (`sl-script status`), the state git
cannot see; leave disk-vs-HEAD to git.

Plan for **no-modify scripts** (mark read-only on disk so no one edits a file
that can never go back). The buffer→inventory mapping (agent inventory vs. a
script inside a prim, needing object id *and* item id) is **shared** with the
language server ([[viewer-lsl-lsp-server]]) — do not invent a second one.

Reference (Firestorm, read-only): `llpreviewscript` (`LLLiveLSLFile`,
`getTmpFileName`), `llexternaleditor`.
