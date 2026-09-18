---
id: viewer-audit-debug-cli-affordances
title: Ten debug affordances sit in the shipping viewer --help
topic: viewer
status: deferred
origin: static code audit (2026-08-26), split out of viewer-audit-binary-module-extraction on 2026-09-18
refs: [viewer-audit-binary-module-extraction]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy-viewer` takes 22 CLI options, **10 of which are debug
affordances** whose own doc comments say so:

- `--camera-position`, `--camera-look-at`, `--camera-spin`, `--camera-spin-axis`
- `--screenshot-dir`
- `--play-animation`, `--repeat-animation`
- `--replay`, `--replay-orbit-light`, `--replay-reflection-probe`

Under the project's CLI rule (a GUI-reachable feature belongs in preferences,
and a diagnostic belongs behind a diagnostic surface) none of these belongs in
the `--help` a resident reads. They are the render-diagnosis and capture-harness
controls.

## Why this is deferred rather than ready

Both plausible shapes cost more than they look like they cost, and neither is
worth paying while the module extraction is in flight.

**A `debug` subcommand.** `sl-client-bevy-viewer debug --camera-position …`
keeps every flag compiled in and only moves it out of the top-level help. But
these exact spellings are the documented interface of things outside this
crate: `sl-crosscheck` writes them when it drives both viewers, the patched
Firestorm harness deliberately *mirrors* them so one environment block
configures both sides, and `book/src/tools/` plus the personal `sl-client`
skill quote them verbatim. Changing the shape means changing all of that in the
same commit, or the cross-check silently stops framing the same scene.

**A `harness` cargo feature.** Gating them out of a release build is the
cleaner end state, but it **adds a feature to the heaviest crate in the
workspace**, and the ggh pre-commit hook runs cargo-hack's *feature powerset*.
The number of configurations that hook compiles is exponential in the feature
count, so one more feature on `sl-client-bevy-viewer` roughly **doubles** the
pre-commit build time for the crate whose single rustc has been measured near
16 GiB, which is why `roadmap/coord.sh heavy --exclusive` exists at all. Every
commit in the repository would pay it, for a tidier `--help`.

## What to do when it is picked up

Decide the shape first, with those two costs on the table — most likely the
subcommand, done as one commit that moves the flags and updates `sl-crosscheck`,
the Firestorm harness's mirrored names, `book/src/tools/*.md` and the skill
together, so no documented command line is ever stale. If instead the feature
gate wins, measure the pre-commit powerset time before and after and record the
number here, because that cost is the whole argument.
