---
id: viewer-audit-ui-core-sound-coupling
title: Three lines in ui_sounds.rs put the protocol stack behind 22 crates
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`ui_sounds` is `sl-viewer-ui-sounds` now, a sibling crate over
`sl-viewer-ui-core` exactly as `sl-viewer-ui-pie-menu` was made one. The module
moved verbatim: the only edits to the file are the `UiRoot` import path and the
`-sk-uisnd-*` CSS registration, which moved from `ViewerSkinPlugin::build` into
`UiSoundsPlugin::build` (safe in either order — `bevy_flair` only snapshots the
property registry into its CSS loader in `Plugin::finish`, after every `build`
has run). `UiSoundsPlugin` is therefore added *after* the skin plugin, and a new
test asserts every one of the fifteen properties lands on the registry, because
an unregistered one is not a compile error but a skin rule that parses to
nothing.

The trivial companion is done too: `ui_pseudoloc` is `pub(crate)`, and the three
doc links that pointed at it from public documentation are plain code spans.

## The note's premise was right and its conclusion was wrong

Those three lines really were the only reason ui-core *directly* depended on
`sl-audio`, `sl-client-bevy` and `sl-viewer-platform`, and they are gone. But
removing them changed **nothing** about the build: `cargo tree -p
sl-viewer-ui-core` still held 559 packages, `sl-proto`, `sl-wire`, `sl-asset`,
`sl-bake`, `sl-mesh`, `reqwest` and `tokio` among them, because

```text
sl-client-bevy ← sl-viewer-platform ← sl-viewer-settings ← sl-viewer-ui-core
```

carried every one of them back in. Same critical path, same rebuild blast radius
for all 27 dependents. The audit measured direct edges; the build measures the
transitive closure.

## So `sl-viewer-settings` was cut loose as well

It held the whole runtime up on **two lines**, and its other three dependencies
(`sl-account-dirs`, `sl-settings`, `sl-l10n`) are leaves — so those two lines
were the entire remaining distance:

- `ViewerSettings::load_with` called `sl_viewer_platform::paths::
  global_settings_file()`. It takes the path as an argument now; the three call
  sites are all in the binary, which is the crate that should know where a file
  lives anyway.
- `load_account_settings` read `sl_client_bevy::SlIdentity` for one `Uuid`.
  Moving the *system* out was not an option — three crates order against it with
  `.after(load_account_settings)`, so it has to stay at the bottom. It reads a
  settings-owned `SettingsAgent(Option<Uuid>)` instead, which the composition
  root fills from `SlIdentity` (`mirror_agent_id`, ordered before the loader).
  The mirror being a frame behind costs nothing: the loader already runs every
  frame until it succeeds, and a new test pins both states of it — nothing loads
  while the mirror is empty, the account scope resolves under the accounts root
  once it fills.

`sl-viewer-settings` now names **no** runtime at all, which is the property
worth keeping: nearly every crate in the viewer reads a setting, so it is a
floor the whole build stands on.

## Result

| | before | after |
| --- | --- | --- |
| packages under `sl-viewer-ui-core` | 559 | **420** |
| workspace crates under it | 27 | **4** (`sl-account-dirs`, `sl-l10n`, `sl-settings`, `sl-viewer-settings`) |

`sl-proto`, `sl-wire`, `sl-asset`, `sl-bake`, `sl-mesh`, `sl-audio`,
`sl-client-bevy`, `reqwest`, `tokio` and `wgpu-types` are all out from under the
UI vocabulary — and out from under `sl-viewer-settings`, so a protocol edit no
longer rebuilds the crates that only wanted `ui::column()` or a setting.
