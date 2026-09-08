---
id: viewer-rlv-environment-commands
title: RLV — @setenv_*/@getenv_* environment control
topic: viewer
status: done
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-environment-fixed-editor, viewer-environment-personal-lighting]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

RLV lets a worn object drive the wearer's local environment:
`@setenv_<subkey>:<value>=force` sets roughly forty sky/water/day
parameters and `@getenv_<subkey>=<channel>` reads them back
(`rlvenvironment.cpp`). Subkeys include daytime, preset/asset/daycycle by
name or UUID, ambient, bluedensity, bluehorizon, densitymultiplier,
distancemultiplier, dropletradius, hazedensity, hazehorizon, icelevel,
maxaltitude, moisturelevel, scenegamma, cloudcolor, cloudcoverage,
clouddensity (plus legacy "cloud"), clouddetail, cloudscale, cloudscroll,
cloudtexture, cloudvariance, moonbrightness, moonscale, moontexture,
sunglowsize, sunglowfocus, sunlightcolor (plus legacy "sunmooncolor"),
sunscale, suntexture, starbrightness, sunazimuth, sunelevation,
moonazimuth, moonelevation, eastangle and sunmoonposition, with legacy
per-component r/g/b and x/y suffixes handled via `idxComponent` in
`RlvEnvironment::onHandleCommand`. The `@setenv=n` gate (a dictionary
restriction our parser already knows) forbids the user opening the
environment editors while scripts control the sky, and the
RestrainedLoveNoSetEnv setting opts out entirely.

These are extension-prefix commands outside the behaviour dictionary —
Firestorm dispatches them through the `RlvExtCommandHandler` fallback, and
our parser today faithfully yields `RlvBehaviour::Unknown` with the raw
keyword kept (`sl-rlv/src/behaviour.rs`). Implementing this means
recognising the `setenv_`/`getenv_` prefixes on Unknown keywords, mapping
each subkey onto our EEP-based environment override layer — the
[[viewer-environment-personal-lighting]] local override is the natural
write target, with [[viewer-environment-fixed-editor]] providing the
editors the `@setenv=n` gate must lock — and answering `@getenv_*` on the
requested chat channel.

Reference (Firestorm, read-only): `indra/newview/rlvenvironment.cpp`,
`indra/newview/rlvenvironment.h`.

## Done

A **seventh layer** in `sl-rlv`: `environment.rs`, the second registered
extension handler. It is a sibling of the debug window rather than a layer over
it — the reference registers `RlvEnvironment` and `RlvExtGetSet` as two
independent `RlvExtCommandHandler`s and offers an unknown keyword to each in
turn — so `RlvState::run_environment` sits beside `run_extension` and every
consumer chains the two in that order.

`RLV_ENV_SETTINGS` is the whole language: 39 rows, each with its name, the
legacy name it also answers to when a per-component suffix is stripped, the
spelling of its value, and whether a script may read or write it. The four
reference lookup maps (get, set, and a legacy pair) are the four flag
combinations of one row, so a row cannot be in three of them and not the fourth.
The scaling, the parsing, the `%f` formatting and the spherical maths behind the
six angle subkeys are all here and unit-tested; the consumer sees the sky in its
own units and never a scale factor.

Sun and moon are asked for as **directions** rather than rotations: the crate
has no quaternion type and wants none, and the image of `+X` under the rotation
is the whole of what `convert_azimuth_and_altitude_to_quat` encodes, so a source
hands over three floats and takes back an `(azimuth, elevation)` pair. The
viewer's half of that conversion already existed (`azimuth_altitude_to_rotation`
in `sl-proto`); its inverse is now `sl_viewer_kit::coords::sky_body_direction`,
which the scene dump had been open-coding.

Five reference quirks are kept, each pinned by a test: the legacy per-component
spellings are a *fallback*, so `@getenv_cloud` is not a command while
`@getenv_cloudr` is; the `i` component is the pre-EEP intensity slider, not a
fourth channel, and writing it rescales the whole colour (or flattens it, when
either end is zero); `@getenv_daytime` answers `-1` or `2` rather than a time,
`2` being a value `@setenv_daytime` itself rejects; an unparsable option is
`FAILED_PARAM` where an unusable one is `FAILED_OPTION`; and the east angle of a
sun due east really does come back as `-0.000000`, because negating a zero
azimuth leaves a signed zero that `%f` prints with its sign.

Viewer side, three seams:

- **`RlvEnvironmentSlot`** (`sl-viewer-world-api::rlv`) is the meeting point,
  the same shape `@setrot` uses to reach the movement driver: the scene
  publishes what it renders, the RLV engine edits a clone of it, and the scene
  takes the edit on its next frame. A write clones the rendered sky into the
  local layer first, exactly as `RlvEnvironment::getTargetSky(true)` does, so a
  script never edits the region's own settings and two components of one colour
  written in a single owner-say line see each other.
- **`EnvironmentState` grew the local layer** the reference calls `ENV_LOCAL`,
  as an `EnvironmentAsset` — so a sky, a water frame or a whole day cycle each
  override exactly the track they are and everything else keeps following the
  grid. It is the **same slot** the World ▸ Environment menu writes: the
  reference has one, so the last writer wins in both directions and "Use Shared
  Environment" empties it whichever put it there.
- **The `@setenv=n` gate** disables every World ▸ Environment entry while an
  object holds the restriction (`can_change_environment`), and the submenu shows
  no check mark at all while a script owns the layer — none of the entries
  describes what is being rendered.

## Not done — and why

- **A library preset by *name* is refused.** `@setenv_preset:Sunrise=force`
  resolves against the inventory Library's `Environments` folder in the
  reference; nothing here indexes that folder, so a name that is not an asset id
  is answered `RLV_RET_FAILED_OPTION` — which is also what the reference answers
  for a name it cannot find. The **id** forms of `@setenv_asset`,
  `@setenv_preset` and `@setenv_daycycle` all work, and they are the same
  request: the reference tries the text as an id first and applies it exactly as
  `@setenv_asset` would. The index is its own task,
  [[viewer-environment-settings-index]] — the quick-preferences sky / water /
  day-cycle combos want the same lookup, so it belongs to neither caller.
- **There is one local environment layer, not two.** The reference writes into
  `ENV_LOCAL` normally and `ENV_EDIT` while an object holds `@setenv`, then
  *selects* whichever it wrote — the two differ in bookkeeping, not in what the
  wearer sees, so one layer reproduces the behaviour.
- **`RestrainedLoveNoSetEnv` still only reads back.** The task text has it
  opting out of the family entirely; the reference does something narrower and
  stranger — it marks the `@setenv` *restriction* `BHVR_BLOCKED`, so an object
  cannot take the sky away from the user, while `@setenv_*` force commands keep
  working. Reproducing that needs the blocked-behaviour flag `sl-rlv` has no
  model for at all, so it is a task of its own —
  [[viewer-rlv-blocked-behaviours]] — rather than a loose end of this one; the
  setting is registered and readable through `@getdebug_*` today.
- **The family reaches no water setting**, because the reference registers none.
  Every subkey is a sky row; a script that wants the water changed hands over a
  whole settings asset. Noted because the task text says otherwise.
- **A per-value edit *after* a whole-environment change in the same line is
  built on the sky from before it.** The reference applies each command as it
  reads it; here a whole-environment change is queued for the scene, so an edit
  that follows it in the same owner-say line clones the sky that was rendering a
  frame ago. The other order is exact — a whole-environment change drops a
  waiting edit, which is where the reference lands too — and so is either order
  across two lines. Closing the gap means running the environment pipeline
  synchronously inside the command path, which an asynchronous asset fetch
  cannot do anyway.

## Verified

`cargo test --release` over the touched crates and `cargo clippy --release
--all-targets` clean over the same set. The new tests pin the dispatch
conditions (head, param kind and the three-character minimum), every scale
factor on the row that carries it, both legacy component directions including
the intensity arithmetic, the pair that has no blue and no intensity, the angle
round trip and the moon following the sun, `daytime` both ways, the asset
refusals, the `@setenv` ownership gate both ways, the unusable reply channel,
the read with no sky at all, and — in the viewer — the clone-before-write, the
null-texture round trip, and the one local slot the menu and a script share.

One bug the tests caught rather than the eye, and the fake-grid tier is what
caught it: the intake system took the new seam as a plain `ResMut`, but the
resource was declared by the *UI* plugins — so a host that runs
`RlvIntakePlugin` without the RLVa windows (the fake-grid smoke tier, and
anything else that wants the intake alone) panicked on the first owner-say line
with "Resource does not exist". It is declared by the intake plugin now, for the
same reason that plugin already declares its own notification message.

Not verified live: no grid session drove the family. The console path is the
interactive check to make — type `@getenv_ambient=2222` at the RLVa console and
read the answer, then `@setenv_ambient:1/0/0=force` and watch the sky go red,
then World ▸ Environment ▸ Use Shared Environment to take it back.
