---
id: idiomatic-p3-04
title: script-control take:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 3 — Intent enums replacing bool / magic-int params (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

script-control `take: bool` → `ScriptControlAction { Take, Release }`
(`types/script.rs:171`). The roadmap's proposed name
(`ScriptPermissionResponse { Granted, Denied }`) was a misnomer — the cited
field is `ScriptControl.take`, the `TakeControls` flag on a
`ScriptControlChange.Data` block (`llTakeControls`/`llReleaseControls`), which
is take/release of movement controls, *not* a permission grant/deny (the real
permission answer, `ScriptAnswerYes`, carries a granted-subset *mask*, no
bool). Renamed (user-approved) to a public `ScriptControlAction` enum in
`sl-proto/src/types/script.rs` (`Take`/`Release`,
`takes_controls`/`from_take_controls`) replacing the `take: bool` field on
`ScriptControl` (renamed `take` → `action`). Codec wraps at the boundary
(decode `from_take_controls`, encode `action.takes_controls()`) so the
`ScriptControlChange` `TakeControls` wire bool is byte-identical. Re-exported
through `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (added `ScriptControl`
itself to the tokio/bevy re-exports too — it was previously absent there yet
is needed to name the enum). The REPL only renders
`Event::ScriptControlChange` as a label (never touches the field), so no REPL
change. Book `content/appearance.md` updated. +1 unit test
(action↔take-controls-flag mapping + round-trip); lifecycle + `sim_session`
round-trip suites updated. NO sl-types touched (a client wire-protocol
concept).
