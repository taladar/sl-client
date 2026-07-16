# sl-client-bevy-viewer

A **Bevy visual viewer** on top of the `sl-client` stack. It logs in via the
shared `credentials.toml` mechanism (`sl-repl::auth`) and renders a region —
terrain, prims (full Linden path/profile tessellation via `sl-prim`), meshes
(`sl-mesh`), sculpt-texture prims (`sl-sculpt`), avatars, sky, water and an
on-screen chat overlay — with a debug fly-camera.

The viewer is a thin rendering application over `sl-client-bevy`'s
`SlClientPlugin`: it consumes only `SlEvent` / `SlCommand` (never touching
`Session` accessors directly) and builds its own ECS scene mirror from the
event stream. Geometry arrives in Second Life's right-handed **Z-up** space; a
single `sl_to_bevy` conversion is applied at the entity `Transform` / camera
boundary to Bevy's **Y-up**.

See the `viewer` topic in the workspace `roadmap/` tree for the staged plan
(`roadmap/context/viewer.md` plus the `viewer-*` task files).

## Two binaries, one library

The crate is a **library** with two thin binaries over it. Both need the UI
modules, and two binaries cannot share a `pub(crate)` module tree — only a
library can give them one.

| Binary | What it does |
| --- | --- |
| `sl-client-bevy-viewer` | the viewer: logs in and renders a region |
| `sl-client-bevy-viewer-gallery` | the **UI gallery**: every UI element on its own, with no login and no world |

```sh
cargo run --release --bin sl-client-bevy-viewer-gallery
```

The gallery needs no credentials and touches no grid. `Tab` walks the widgets,
`Enter` activates one (inertly — see below), `D` flips the layout direction,
`L` cycles the strings through pseudolocalisation and each script, `S` cycles
the UI font size, `Escape` quits.

## Testing the UI (`viewer-ui-test-harness`)

UI bugs are the ones that only appear in a particular font, script, translation
or UI scale — a combinatorial space no human walks. So `cargo test` runs the
whole grid headlessly: real `bevy_ui` layout with real fonts, no window, no GPU,
no login. Every registered element (`src/ui_element.rs`) is checked in every
script, both directions, several UI scales and font sizes, and under
pseudolocalisation, against every check in `src/ui_test.rs`.

Two obligations on a new panel or widget, both cheap and both load-bearing:

- **Register it in `ui_element::ELEMENTS`.** That is the whole opt-in, and it
  buys the element every check that exists — and every check added later.
- **Make it constructible without its wiring.** An element emits a `UiAction`
  rather than calling into a `Session`, so the viewer can route it to a real
  handler while the gallery routes it nowhere and a test reads it off a queue.

The gallery is where bugs get *found*; the harness is where they stay found. A
human spots something, and the fix is a **check** — which from then on runs
against every element forever.

## Non-goals

Voice audio transport (signalling only — see the workspace docs) and sound.
