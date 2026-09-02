# sl-crosscheck

Runs this workspace's viewer and Firestorm against **one** fake grid and
collects what each of them drew, so a person can put the two side by side.

A rendering question that neither viewer can answer alone — *is our sky too
dark, or is Firestorm's too bright; is that prim at the wrong height, or is
its texture at the wrong scale* — is answered by photographing the same
scene with both viewers. This crate produces the photographs. Comparing
them (a contact sheet, an image diff, a scene-dump diff) is a separate
step, deliberately: keeping the collection honest is easier when it has no
opinion about what the frames should look like.

## Running one

```sh
cargo build --release -p sl-client-bevy-viewer
cargo run --release -p sl-crosscheck -- \
  --scenario catalogue \
  --look-at mesh-cube \
  --day-position 0.25 \
  --firestorm "${FIRESTORM_BUILD}/newview/packaged/firestorm"
```

`--firestorm` points at the launcher in a Firestorm build tree — wherever
that tree is on the machine; nothing here assumes a location, and
`SL_CROSSCHECK_FIRESTORM` sets it once for a shell. Without it only this
viewer runs, and the report says the other half was *skipped* rather than
implying it failed. `--only sl-client` / `--only firestorm` runs one on
purpose.

The camera is aimed at a **landmark of the chosen scene** — `mesh-cube`
rather than a position nobody can check — and the runner logs every
landmark the scene has on startup. `--camera-position` / `--camera-look-at`
take explicit `x,y,z` region coordinates instead.

## What a run leaves behind

```text
<run>/
  run.json                  what was asked for, and what each viewer did
  config/                   the credentials + grid files both viewers read
  sl-client/
    frame_000.png …         the numbered capture sequence
    scene.json              the structured scene dump, when the viewer writes one
    harness-status.json     whether the run happened at all
    viewer.log              that viewer's own output
  firestorm/
    …the same four
  sl-client-state/          that viewer's per-run settings / cache / logs
  firestorm-state/          FIRESTORM_X64_USER_DIR for the run
```

## The three things that decide whether a run is usable

**A viewer is asked to quit, never killed.** A session the simulator still
believes is logged in makes the *next* run fail to log in, with a failure
that looks exactly like a viewer bug. The escalation is `SIGTERM` — which
both viewers turn into a logout — then the logout grace, and only then
`SIGKILL`. Firestorm's own `--quitafter` is unusable here for the same
reason: it calls `forceQuit()`, which sends no `LogoutRequest`.

**The status file, not the exit code, says whether a run happened.** A
viewer that never got in world still writes a full set of frames, black and
on schedule, and neither viewer's shutdown carries a status out reliably.
Both write `harness-status.json` before they log out; no file means the run
never reached that point. "The viewers differ" and "the run did not happen"
are never reported the same way.

**Each viewer is confined to the run directory.** `FIRESTORM_X64_USER_DIR`
and this viewer's `XDG_*` roots point inside it, so a run cannot rewrite the
operator's own settings, cannot inherit last run's texture cache — which is
how a fixture whose pixels changed under a stable id goes unnoticed — and
two runs cannot fight over the same files.

## Why the grid is in-process

A readiness probe against a port proves the *port* answers, not that the
grid you started did: `scripts/fake-grid.sh` grew a check for exactly that
after cheerfully reporting a leftover grid from an earlier run as ready.
Binding the port in the runner makes that class of mistake impossible — an
address already in use is an immediate, honest error, and the grid is ready
when `start()` returns rather than when a poll says so.

One grid serves both viewers, one after the other: they log in as the same
avatar, and two GPU-bound viewers photographing a scene at once are two
viewers photographing a machine under load.
