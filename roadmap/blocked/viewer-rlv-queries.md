---
id: viewer-rlv-queries
title: RLV — answer @get* queries via chat reply
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Answer the RLV **queries** — the commands whose param is a number, meaning
"chat the answer back on this channel". The parser
([[viewer-rlv-command-parser]]) already recognises the query form and its
channel; this task gathers the answer from viewer / session state (and the
restriction state, [[viewer-rlv-restriction-state]]) and replies:

- `@version*` — the version handshake (reports 3.4.3 with a 2.9.28
  compatibility floor);
- `@getoutfit`, `@getattach` — worn wearables / attachment points;
- `@getstatus` — the current restriction set for an object;
- `@getinv` / `@getinvworn` — inventory listing / worn-folder state;
- `@getsitid` — the object currently sat on;
- `@getcam_*` — current camera parameters.

Replies go back with `RlvUtil::sendChatReply` on the given channel, **split
across multiple lines** when the answer is long. Some queries (`@version*`,
`@getstatus`) need no viewer state and can be answered from `sl-rlv` directly;
the rest read the relevant viewer/session snapshot.

Reference (Firestorm, read-only): `rlvhandler.cpp` (query dispatch),
`rlvcommon.cpp` (`RlvUtil::sendChatReply`).
