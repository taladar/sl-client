---
id: viewer-perf-session-network-thread
title: Run the Bevy session on a dedicated network thread
topic: viewer
status: done
origin: unbounded-frame-work survey (2026-08-09, performance branch)
refs: []
---

Context: [context/viewer.md](../context/viewer.md).

The Bevy driver's `drive` system did all protocol work on the frame thread,
once per frame: drain the whole UDP socket (full LLUDP parse + session state
machine + ACK bookkeeping per datagram), ingest the CAPS event-queue
payloads, run timers, apply commands, and write the chat-log / inventory
disk caches. A login/teleport backlog decoded in one frame, and ACKs /
retransmits were quantised to the frame rate. (CAPS transport + LLSD decode
were already on a background worker; only the semantic ingestion was
main-thread.)

Now the whole session lives on one dedicated `sl-session-net` thread
(sl-client-bevy):

- `start_login` spawns it; the thread performs the blocking XML-RPC login
  (`login_phase`) and then ticks `advance_running` continuously. Each tick
  blocks in `recv_from` under a 15 ms read timeout — a datagram is parsed
  (and ACKed) the moment it arrives, and an idle tick still services
  timers, CAPS payloads, and queued commands. After the first datagram the
  rest of the backlog drains non-blocking, so the ACK flush is never
  delayed by a second timeout wait.
- `drive` is now a thin pump: it forwards the frame's `SlCommand`s over a
  channel (cloned — `sl-proto`'s `Command` gained `Clone` — so in-process
  observers like the mutes tap still read the originals) and drains a
  `NetOutbound` channel into the Bevy messages and resources (`SlEvent`,
  `SlDiagnostic`, `SlCapabilities`, `SlIdentity`, `SlAgentParcel`, MFA /
  rejection). The agent-parcel mirror is compared in-thread and crosses
  the channel only on change.
- The chat-log and inventory-cache disk writes moved off the frame thread
  for free. Offline/replay mode simply spawns no thread. A thread that
  dies without a clean `LoggedOut` / `Disconnected` surfaces a synthetic
  disconnect instead of hanging silently; app teardown closes the command
  channel, which the thread notices within one tick and exits on.

No parity concern: `sl-client-tokio` already ran the session on its own
runtime.

Live-verified on the local OpenSim grid via `sl-repl-bevy --smoke` plus a
scripted chat round-trip (sent and echoed back by the sim), `query_friends`
(the in-thread local-query path), and `logout`. The `LogoutReply` timeout
warning seen there also occurs identically with `sl-repl-tokio` (untouched
by this change) — pre-existing grid behaviour, not a regression.
