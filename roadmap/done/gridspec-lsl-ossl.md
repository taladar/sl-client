---
id: gridspec-lsl-ossl
title: OSSL availability and denial on OpenSim
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-ossl, viewer-script-warning-window,
  viewer-lsl-editor-highlight, server-fake-grid-lsl-syntax]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`AllowOSFunctions = true`, threat level VeryLow in the local config.

## Discover

What is enabled and denied per engine, and the denial error text.

## Document

`book/src/gridspec/lsl.md` § OSSL.

## Fake grid

Large — [[server-lsl-lib-ossl]].

## Viewer

OSSL highlighting from the grid-served syntax; denial errors shown.

## Done (2026-10-26)

- **Discover.** Two probes, `sl-lsl-runtime/probes/ossl/`: `available`
  (functions allowed by an `Allow_ = true` line above the threat level, by a
  threat level under the configured one, by checking only that OSSL is on,
  and by checking nothing, with what each answers, `osGetAgentIP` behind its
  god check among them) and `errors` (one script per way a call is refused
  or fails — threat level, an `Allow_` list, `Allow_ = false`, a run-time
  error, a deprecated function, a shouted error — plus a LightShare and a
  MOD call, then every script's state, a ping, a reset and the call again,
  with `DEBUG_CHANNEL` listeners 10 m and 50 m away). Complete on both
  OpenSim engines; on aditi each stops at its compile error, which is the
  measurement there.
- **Corpus and runner.** A `library: opensim` header key lets the corpus
  test accept, in a script for OpenSim, the undefined names OpenSim's
  library adds (the `os*`, `ls*` and `mod*` families and their constants)
  and nothing else. The runner now notes the second avatar's
  `DEBUG_CHANNEL` chat too (`# debug-channel by=other:`), since who is told
  of an error was part of the question.
- **Found on the way.** Every OSSL permission or run-time error reaches
  every avatar near the object and no script; YEngine stops the script
  until a reset, XEngine runs on, each wrapping the text its own way; a
  deprecated function costs a one-second sleep and a shout scripts hear.
  OpenSim's served `LSLSyntax` document lists disabled and denied functions
  like any other and leaves out `osListenRegex` and every `mod*` function.
  Second Life reports an undefined constant ahead of an earlier undefined
  call.
- **Document.** `book/src/gridspec/lsl.md` § OSSL, linked from
  `content/scripts.md`; the `library` key and the new note in
  `book/src/tools/lsl-probes.md`.
- **Fake grid.** Large, as planned: the measured permission model and texts
  are noted on [[server-lsl-lib-ossl]], the served document's gaps on
  [[server-fake-grid-lsl-syntax]].
- **Viewer.** Not built here, as neither surface exists yet: denial errors
  are ordinary nearby-chat lines until [[viewer-script-warning-window]],
  which now notes that OpenSim sends them to every avatar, not the owner
  alone, and in which two layouts; OSSL highlighting waits on
  [[viewer-lsl-editor-highlight]], which now notes that nothing yet hands
  the decoded `LSLSyntax` table to the editor and that OpenSim's document
  does not say what the region allows.
