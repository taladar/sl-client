---
id: viewer-settings-prose-description-keys
title: Four settings register prose where their description key belongs
topic: viewer
status: done
origin: found while running viewer-automation-ground-aim's e2e tests (2026-10-01)
---

## Done (2026-10-01)

Each of the four now registers a `setting-desc-<name>` key
(`setting-desc-RenderResolutionDivisor`, `setting-desc-panorama_face_size`,
`…_output_width`, `…_format`), with its text in the English bundle. The guard
is `settings_golden`'s `every_setting_description_is_an_english_key`: every
declared setting's description key must be a message the English bundle
defines.

Context: [context/viewer.md](../context/viewer.md).

`register_in`'s last argument is the Fluent key of the setting's description
(`setting-desc-<Name>`). Four registrations pass English prose instead, so
every viewer start logs a warning for each one:
`no bundle in the active locale chain defines the string key `Divisor for
rendering the 3D scene at reduced resolution (1 = full)``. The debug-settings
editor then has no translated description for them.

- `sl-viewer-world-scene/src/resolution_divisor.rs`, the render resolution
  divisor (its test copy in `sl-viewer-world-api/src/rlv.rs` repeats it);
- `sl-viewer-world-view/src/panorama.rs`, the 360 capture's face size,
  output width and output format.

Fix: a `setting-desc-<Name>` key for each, with the prose moved into
`assets/locales/en/main.ftl` beside the others. Then add a guard that every
registered description is a key the English bundle defines, so the next one
fails a test rather than logging a warning nobody reads.
