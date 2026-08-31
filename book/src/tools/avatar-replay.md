# Avatar state capture & replay

Live avatar-render bugs — a protruding mesh-head tongue, a brow spike, missing
hair, a blown-out facelight, misapplied clothing alpha — are hard to reproduce:
the offending avatar can log out or change outfit, and the exact combination of
shape, attachments, bakes, materials and playing animations is gone. The
**capture / replay** tool records one moment of a nearby avatar's complete
render *inputs* into a local bundle, so the viewer can rebuild and
**render it offline**, with no grid, any time later.

The point is that replay drives the viewer's *live* render pipeline over the
captured inputs — it stores inputs, not a screenshot. So the loop is: capture a
buggy avatar → edit the rendering code → rebuild → `--replay` → see whether the
fix took, repeatably, after that avatar is long gone.

> **Bundles are strictly local and must never be committed or shared.** They
> contain other residents' actual mesh/texture assets (creator permissions:
> no-transfer, etc.) and appearance (privacy). The capture has no in-repo
> default output path, and `.gitignore` guards `/avatar-dumps/` and
> `*.avatardump/`. A committed regression fixture must be synthetic, never
> derived from a real capture.

## Capturing

Run the viewer logged into a grid with capture enabled:

```console
SL_VIEWER_DUMP_DIR=avatar-dumps \
  cargo run --release -p sl-client-bevy-viewer -- --grid <grid> --avatar <name>
```

(The vendored `viewer-assets/character` directory is the viewer's default
for the avatar assets; set `SL_VIEWER_ASSETS` only to point at different
ones.)

With `SL_VIEWER_DUMP_DIR` set, pressing **Ctrl+Alt+D** writes a bundle for every
nearby avatar. Capture is opt-in: with the variable unset the capture systems
are not even added, so a normal session pays nothing.

Each capture writes, per avatar, a `<agent>.json` manifest holding the
*raw session events* needed to rebuild it — the avatar object and its whole
attachment tree (verbatim wire `Object`s, so transforms, per-face
`TextureEntry`, `ExtraParams`/light and mesh ids are all carried), the decoded
`AvatarAppearance` (visual params + baked-texture entry), the playing-animation
set, and any legacy `LLMaterial`s its faces reference. Alongside sits a shared
`cache/` laid out as a **drop-in asset cache**
(`cache/<kind>/<first-char>/<uuid>.<ext>`) with the referenced meshes,
animations and PBR material assets copied verbatim out of the viewer's live
caches.

**Textures are fetched, not copied.** The local cache usually holds only the
low-LOD prefix the viewer happened to load for a given texture; a copy of that
would be an incomplete codestream. So on **Ctrl+Alt+D** the capture fetches each
referenced texture at *full resolution* from the live session's capabilities
(regular textures from `GetTexture`, baked body textures from the appearance
service) and writes the complete codestreams into the bundle. This runs on a
worker thread but the capture **blocks until it finishes** — a detached fetch
would be killed when you close the viewer, leaving the bundle incomplete — so
the viewer pauses for a few seconds. Wait for the `capture complete` log line
before closing.

No display names are stored; a bundle is keyed only by agent UUID.

## Replaying

Point the viewer at a bundle directory:

```console
cargo run --release -p sl-client-bevy-viewer -- --replay avatar-dumps
```

There is no login. `--replay` loads every `<agent>.json` in the directory and
renders all of them at their captured region-local positions (point it at a
directory with a single manifest to replay just one avatar). `--viewer-assets`
(the Firestorm `character/` dir) is required — a body needs the system skeleton
and base meshes, or avatars fall back to placeholder spheres.

Replay composes with the debug-camera / screenshot harness (`--camera-position`,
`--camera-look-at`, `--camera-spin`, `--screenshot-dir`); by default it frames
the first captured avatar.

### How it works

The viewer runs its normal `App` with `SlClientPlugin` in **offline** mode: the
whole event/resource substrate is registered but no login or circuit is opened.
A one-shot injector then feeds the session synthetic `SlEvent`s from the bundle
— a placeholder `SlCapabilities` (which opens the cap-gated asset managers so
they serve from the bundle's `cache/`), the avatar objects and their attachment
trees, each `AvatarAppearance`, each animation set, and the legacy materials —
and the normal render systems derive bakes, part-visibility, attachments and
pose exactly as a live login would. A process-global cache-root override points
every asset store at the bundle's `cache/`.

### Test rig

A bare void does not exercise every material path, so two optional extras can be
placed around the avatar:

| Flag | What it adds |
| --- | --- |
| `--replay-orbit-light` | A local light orbiting the avatar — sweeps specular highlights across a surface |
| `--replay-reflection-probe` | A local reflection probe on the avatar, feeding image-based lighting |

(The global reflection probe is active in either case.)

## Headless geometry analyzer

For a geometry-only diagnosis without a window, an example reconstructs the
posed skeleton from a bundle and prints each mouth/brow bone's distance from
`mHead`, at the deformed rest and under the captured animation pose:

```console
cargo run --release -p sl-client-bevy --example avatar_replay -- avatar-dumps/<agent>.json
```

`SL_REPLAY_TIME` (seconds, default `1.0`) picks the animation sample time.

## Limitations

- A texture that the `GetTexture` / appearance service will not serve at capture
  time is simply absent from the bundle (and renders missing on replay).
- The reflection-probe test rig spawns a local probe best-effort; the global
  probe always provides image-based lighting regardless.
