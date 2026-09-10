---
id: viewer-asset-root-needs-an-env-var
title: The viewer binary finds no icons, skin or locales unless BEVY_ASSET_ROOT is set
topic: viewer
status: bugs
origin: hit again during the hold-to-fly live test (2026-09-10); recurring since
  the skin work
---

Context: [context/viewer.md](../context/viewer.md).

Run the compiled viewer directly — `./target/release/sl-client-bevy-viewer` —
and it starts, logs in, renders the world, and quietly ships without its own
assets:

```text
ERROR bevy_asset::server: Failed to load folder. Path not found: locales
ERROR bevy_asset::server: Path not found: …/target/release/assets/skins/graphite/skin.css
ERROR bevy_asset::server: Path not found: …/target/release/assets/icons/parcel/fly.png
```

Bevy resolves its asset root relative to the **executable's** directory, so it
looks in `target/release/assets/`, where nothing is. The real tree is
`sl-client-bevy-viewer/assets/` (`icons/`, `locales/`, `skins/`). Running from
the crate directory does **not** help — the cwd is not consulted.

The workaround is `BEVY_ASSET_ROOT=<repo>/sl-client-bevy-viewer`, or launching
through `cargo run` (which sets `CARGO_MANIFEST_DIR`). Neither is discoverable
from the failure: the viewer comes up looking *almost* right, so a UI check can
be run and believed against a build with no skin and no icons. The same trap
catches the gallery binary, where a missing skin means no focus ring at all.

## Why it deserves a fix rather than a habit

It recurs constantly, it is silent in the sense that matters (the window opens),
and it degrades exactly the runs where appearance is the thing being judged.
It also cannot survive installation: an installed viewer has no crate directory
to point at, so whatever answer this gets has to work for a shipped build too.

## Shape of a fix

Have the binary resolve its own root instead of inheriting Bevy's default, with
`BEVY_ASSET_ROOT` still winning when set (so an override stays possible):

- an `assets/` beside the executable — the installed layout, and what Bevy
  already assumes; then
- `CARGO_MANIFEST_DIR`'s `assets/` baked in at compile time — the development
  layout, which makes the raw `target/…` binary behave like `cargo run`.

Either way the missing-asset case should also be **loud**: the skin stylesheet
and the locales folder failing to load are not cosmetic, and a viewer that
cannot find them should say so once, plainly, rather than emitting a scatter of
per-file `bevy_asset` errors that read like ordinary noise.

## Verify

`./target/release/sl-client-bevy-viewer` from the repo root, with no environment
set: the skin, toolbar icons and Fluent labels all load, and the log carries no
`Path not found` under `assets/`. Same for
`sl-client-bevy-viewer-gallery`, whose focus ring is the tell.
