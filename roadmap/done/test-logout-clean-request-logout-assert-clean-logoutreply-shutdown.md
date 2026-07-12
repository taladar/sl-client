---
id: test-logout-clean
title: request logout, assert clean LogoutReply / shutdown
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 1 — Session lifecycle & circuit `[both] 1av`
---

Context: [context/test.md](../context/test.md).

`logout-clean` — request logout, assert clean `LogoutReply` / shutdown.
SL replies cleanly (`complete`); OpenSim never transmits the reply (queued
then dropped by an unimplemented `LLUDPServer.Flush` + outbox-clearing
`Shutdown`), so it logs out via the 5 s timeout fallback and is recorded
`partial`. Our client is conformant on both.
