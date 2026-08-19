---
id: viewer-settings-backup
title: Settings backup — export / import
topic: viewer
status: ready
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

## Parity-audit addendum (2026-08-19)

Parity-audit extension: a "Reset All Settings" factory-reset action
(Firestorm's advanced-preferences button) — wipe the settings store
back to defaults (global and per-account), with a confirm; the natural
inverse of backup/restore and the same code path as a restore of an
empty snapshot.
