---
id: viewer-asset-root-needs-an-env-var
title: The viewer binary finds no icons, skin or locales unless BEVY_ASSET_ROOT is set
topic: viewer
status: done
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

## What landed

`sl-client-bevy-viewer/src/asset_root.rs`: the three-step resolution above,
pinned onto `AssetPlugin::file_path` as an **absolute** path (an absolute
`file_path` replaces Bevy's base entirely, since that base is only `join`ed
onto it). All three binaries take it — the viewer, the UI gallery
(`gallery.rs`) and the render gallery (`render_gallery.rs`), the last of which
was not setting `AssetPlugin` at all.

Start-up now says which layout it found in one `viewer assets` line, and an
incomplete tree is a single `error!` naming the directory, what it lacks of
`skins` / `locales` / `icons`, and the override — instead of a scatter of
per-file `bevy_asset` failures.

`BEVY_ASSET_ROOT` is obeyed **whether or not it exists**: a wrong override must
fail at the path the operator named rather than fall through to a tree that
happens to be there and quietly render a different skin.

Ten unit tests cover the order (override wins, installed beats development, the
`target/release` case, neither present, no executable path) and the
missing-entry report, with one that asserts the shipped crate tree really does
hold every required entry — the compile-time fallback the whole thing leans on.

## Verify

`./target/release/sl-client-bevy-viewer` from the repo root, with no environment
set: the skin, toolbar icons and Fluent labels all load, and the log carries no
`Path not found` under `assets/`. Same for
`sl-client-bevy-viewer-gallery`, whose focus ring is the tell.

**Verified (2026-09-10, release builds, `BEVY_ASSET_ROOT` explicitly unset).**

- `sl-client-bevy-viewer-gallery` from the repo root: one
  `INFO … viewer assets path=…/sl-client-bevy-viewer/assets source="the viewer
  crate's source tree"`, and **zero** `ERROR` lines in the whole run — against a
  `target/release/` that holds no `assets/` at all, which is the reported case.
- `sl-client-bevy-viewer` on the local OpenSim grid, same bare invocation: the
  same resolution line, zero `ERROR` lines, no `Path not found`, and the UI
  confirmed skinned by eye over a five-minute session to a clean `session
  ended`.
- The loud path, `BEVY_ASSET_ROOT=/nonexistent-tree`: one
  `ERROR … the viewer's asset tree is incomplete … path=/nonexistent-tree/assets
  source="BEVY_ASSET_ROOT" missing="skins, locales, icons"` **ahead of** the
  per-file `bevy_asset` scatter, so a mistyped override names itself instead of
  hiding in the noise.
