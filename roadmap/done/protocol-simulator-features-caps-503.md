---
id: protocol-simfeatures-503
title: SimulatorFeatures capability GET returns 503 on the local OpenSim (one-shot fetch never retries)
topic: protocol
status: done
origin: observed while live-testing protocol-lsl-syntax (2026-07-15)
---

Context: [context/protocol.md](../context/protocol.md).

While live-testing [[protocol-lsl-syntax]] against the local `opensim.service`
grid, the automatic **`SimulatorFeatures`** capability GET failed on every run,
in **both** runtimes (`sl-client-tokio` and `sl-client-bevy`), logging:

```text
WARN sl_client_tokio: CAPS request failed; no reply surfaced capability="SimulatorFeatures"
```

A throwaway diagnostic in `get_llsd` showed the GET returns **HTTP 503 Service
Unavailable with an empty body** — so it is an OpenSim-side cap-serving fault,
not a client parse error (the body never reaches `parse_simulator_features`).

Crucially this is **specific to the `SimulatorFeatures` cap**, not a general
outage: at the *same* caps-arrival instant the `LSLSyntax` cap GET returned 200
and decoded fully (653 functions / 770 constants / 35 events), and the region
handshake, EEP environment, and texture caps all worked. So the 503 is not a
startup race that a blanket retry would paper over — the `SimulatorFeatures`
handler itself is answering 503 on this OpenSim build/config.

Two things to pin down:

- **Why does OpenSim's `SimulatorFeatures` cap answer 503** here when its
  sibling caps answer 200? (Check `SimulatorFeaturesModule` handler
  registration / method, and whether a config toggle or a poll-service wrapper
  is involved. The `[SimulatorFeatures]` block in `OpenSim.ini` is all
  commented, i.e. defaults.)
- **Client robustness:** the runtimes fetch `SimulatorFeatures` **once** at caps
  arrival with **no retry** (`spawn_simulator_features`), so any transient
  failure is never recovered — unlike Firestorm, which defers/retries. Even if
  the OpenSim 503 is fixed, a one-shot fetch is fragile.

Impact: [[protocol-lsl-syntax]] is implemented and verified end-to-end (the
`LSLSyntax` fetch + decode works against the real OpenSim document), but its
**automatic trigger** keys off the `lsl_syntax_id` carried in the
`SimulatorFeatures` reply — so while this 503 stands, the local grid never fires
the LSLSyntax fetch on its own. The feature works the moment `SimulatorFeatures`
decodes (as on Second Life / aditi, or once this is fixed).

## Done (2026-08-28)

**Root cause — the grid is behaving as designed, the client was not.**
`SimulatorFeaturesModule.HandleSimulatorFeaturesRequest` answers
`503 Service Unavailable` + `Retry-After: 5` whenever
`m_scene.GetScenePresence(agentID)` is null, i.e. for as long as the requesting
agent has no `ScenePresence` yet. Its sibling `HandleSyntaxRequest` — served by
the *same* module, on the same connection — has no such check, which is exactly
why `LSLSyntax` answered 200 at the same instant. So this was never a config
toggle or a poll-service wrapper.

That makes the failure **deterministic, not a race lost by bad luck**: both
runtimes fire the `SimulatorFeatures` GET the moment the capability map lands,
which is the same instant the deferred `CompleteAgentMovement` is released — so
the first ask always arrives before the presence exists. A one-shot fetch could
not win.

**Fix — retry the transient answer**, in the one place every one-shot capability
GET goes through: `get_llsd` (`sl-client-tokio`) and `blocking_get_llsd`
(`sl-client-bevy`) now loop over the shared `retry` policy already used by the
asset/texture fetchers (`is_transient_status` — `503`/`502`/`504` — with
exponential backoff, 8 retries). A status that is *not* transient (`404`,
`500`) still fails the fetch at once rather than spending the budget on an
answer that will not change, a transport error is retried like a transient
status, and every outcome is now logged with the capability name (never the
URL — it carries the per-session cap token). This is what the reference viewer
does: `LLViewerRegionImpl::requestSimulatorFeatureCoro` re-issues the GET on any
non-success status, up to 30 attempts. `Retry-After` is deliberately ignored,
as in the reference — honouring OpenSim's `5` would make the first retry 25×
slower than the presence actually takes to appear.

Unit tests (`sl-client-tokio/src/http.rs`) pin the three behaviours against a
loopback stub: a transient refusal is retried until the document arrives, a hard
rejection is not retried at all, and the retry is bounded (budget spent, then
one failure). They build their own no-proxy client — the crate's
`http_proxy::client_builder` reads a process-global that the sibling
`proxy_lifecycle` test installs.

Live-verified on the local `opensim.service` grid, **both runtimes**, each
logging the 503 on `attempt=0` and the document on the retry:

- `sl-repl-tokio`: `simulator_features(SimulatorFeatures { … lsl_syntax_id:
  Some(4b833b57-…) … })`, followed by the `lsl_syntax(…)` event.
- `sl-client-bevy-viewer`: `loaded grid LSL syntax definition symbols=1473
  functions=653 constants=770 events=35` — which only fires off the
  `lsl_syntax_id` *inside* the `SimulatorFeatures` document, so the whole
  chain this bug blocked now completes unaided.
