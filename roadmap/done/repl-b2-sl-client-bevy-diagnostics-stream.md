---
id: repl-b2
title: sl-client-bevy diagnostics stream
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase B — runtime wiring (keep tokio & bevy at parity)
---

Context: [context/repl.md](../context/repl.md).

**B2. sl-client-bevy diagnostics stream.** Registered
`SlDiagnostic(pub Diagnostic)` next to `SlEvent`, written from the running
system (drains `poll_diagnostic` plus the CAPS-failure sentinel). Same flag
via `SlClientPlugin::diagnostics` → `set_diagnostics`, and the mirrored
CAPS-failure surfacing in the blocking helpers. (Mirrors B1; parity.)
