---
id: viewer-settings-sanity-checks
title: Settings sanity-check warnings
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-preferences-debug-settings-editor, viewer-ui-settings-store,
       viewer-settings-file-resilience]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's sanity checker (`sanitycheck.cpp`, Zi Ree): each debug
setting can carry a sanity rule (min / max / equals plus warning text,
the SanityCheck… entries in `app_settings/settings.xml`); when a
control changes to an insane value the viewer shows a one-shot
notification ("bandwidth set above 1500 kbps causes packet loss…")
with an ignore option.

Ours has a typed settings store ([[viewer-ui-settings-store]]), so
type errors are impossible, and the curated preferences UI already
clamps ranges — which is why this sits in ideas: the value would
mainly be guarding the raw debug-settings editor
([[viewer-preferences-debug-settings-editor]] done,
`sl-client-bevy-viewer/src/debug_settings.rs`) against
legal-but-harmful values, a much smaller surface than in the
reference. If picked up, the natural shape is optional per-setting
validation metadata in the typed store plus a one-shot warning toast.

Reference (Firestorm, read-only): `indra/newview/sanitycheck.cpp`,
`indra/newview/app_settings/settings.xml` (SanityCheck… entries).
