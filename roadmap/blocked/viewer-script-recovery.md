---
id: viewer-script-recovery
title: Unsaved-script recovery
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-lsl-editor-widget]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's script recovery: the in-viewer LSL editor
([[viewer-lsl-editor-widget]]) continuously autosaves dirty buffers to a
per-account recovery directory; after a crash the next start offers a
"Recover unsaved scripts" dialog listing the orphaned buffers (script name,
source object/item, timestamp) to restore into editors or discard. Small
but loss-preventing; the autosave hook belongs in the editor widget's
buffer layer so notecard editing can reuse it later.

Reference (Firestorm, read-only): `fsscriptrecover` /
`floater_script_recover.xml`.

Deps: [[viewer-lsl-editor-widget]] (the buffer layer being autosaved).
