---
id: viewer-settings-backup
title: Settings backup — export / import
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's settings-backup tab: export the viewer configuration (global
settings, per-account settings, chosen extras — toolbar layout, AO config,
contact sets — via checkboxes) to a directory, and restore from one, with
a preview of what a restore will overwrite. Our settings are already
tidy TOML files under XDG paths, so this is mostly a manifest-driven
copy with selection UI — but it earns its keep on migration between
machines and before risky experiments. Secrets (saved passwords via
keyring) are explicitly **not** exported.

Reference (Firestorm, read-only): `panel_preferences_backup.xml`,
`fsfloaterbackup`.

Deps: [[viewer-preferences-floater]] (it is a prefs tab).
