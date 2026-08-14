---
id: test-agent-alert
title: observe AgentAlertMessage / AlertMessage
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 19 — Error handling & recovery `[both]`
---

Context: [context/test.md](../context/test.md).

`agent-alert` — observe `AgentAlertMessage` / `AlertMessage`. `1av`.
**Green (complete) on OpenSim as the estate-owner avatar; pass (partial) on
aditi.** Implemented 2026-08-12, two deterministic provocations — one per
notice channel:

1. **Set-Home** ([`Command::SetStartLocation`], Home slot): every outcome
   is answered with a notice — success or the not-allowed refusal — so the
   reply is deterministic regardless of land ownership. The case accepts
   either channel and asserts the decoded notice is non-empty.
2. **Estate map regeneration** (`EstateOwnerMessage` /
   `refreshmapvisibility`, hand-built via `Command::Send` — deliberately no
   typed command for this admin nudge): with estate rights every branch of
   OpenSim's handler replies with a plain `AlertMessage` ("Terrain map
   generated", the 2-minute cool-down notice, or generator-unavailable), so
   any reply exercises the broadcast channel. Without estate rights the
   request is silently refused (same estate gate as `kick-user`), so the
   OpenSim run uses `--avatar estate-owner`.

Observed live:

- **OpenSim** answers Set-Home with an **`AgentAlertMessage`**
  ("Home position set.", modal false, addressed to self, ~57 ms) and the
  map poke with an **`AlertMessage`** ("Terrain map generated", no
  `AlertInfo`, ~152 ms) — both channels decoded, run complete.
- **aditi** answers the Set-Home refusal as a plain **`AlertMessage`**
  ("You can only set your 'Home Location' on your land or at a mainland
  Infohub.", ~200 ms) — the opposite channel from OpenSim's, a nice
  cross-grid decode of the same provocation. The map poke is silently
  refused (no estate rights), recorded partial.

Records per notice: channel kind, message text (or the first keyed
`AlertInfo` id when the plain text is empty), the modal flag /
addressed-to-self (agent channel), the `AlertInfo` count (broadcast
channel), and per-notice latency.

**New client code:** `StartLocationSlot` was `sl_proto`-only and is now on
both runtime re-export lists (same gap as earlier cases); everything else
(`Command::SetStartLocation`, both alert events, the raw `Command::Send`
path) pre-existed.
