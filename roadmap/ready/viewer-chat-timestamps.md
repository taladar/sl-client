---
id: viewer-chat-timestamps
title: Timestamps in the chat display windows
topic: viewer
status: ready
origin: user request (2026-07-22) — the on-disk chat logs carry
  Firestorm-compatible timestamps, the display windows show none
refs: [chat-b9]
---

Context: [context/viewer.md](../context/viewer.md).

Per-line **timestamps in the chat displays**: the nearby-chat window /
toasts and every conversation transcript (IM, group, conference)
prefix each line with `[HH:MM:SS]` — **24-hour, seconds always on**
(the reference makes both optional; we deliberately skip the toggles
for now), and in **SLT** (the grid's Pacific time,
`America/Los_Angeles` with its DST), the clock the reference viewer
shows everywhere. The on-disk chat-log files keep their existing
Firestorm-compatible zone-less local stamps
([`LogTimestamp`](../../sl-proto/src/chat_log.rs)) — display and file
may therefore differ by the local↔SLT offset, exactly as in the
reference. History reloaded from the log files should surface its
stored stamps rather than the load time.

Reference (Firestorm, read-only): `llchathistory.cpp` /
`llfloaterimnearbychat.cpp` (`ChatTimestampFormat`, the
timestamp-prefix settings).
