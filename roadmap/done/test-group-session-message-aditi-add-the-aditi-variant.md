---
id: test-group-session-message-aditi
title: Group session message — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `group-session-message` case
(`[[test-group-session-message]]`, already green `[opensim]`): **pass
(partial) on aditi live** (2026-08-12, Phase Z batch). Same Second Life
behaviour as [[test-group-notice-aditi]]: a message sent into a
freshly created group is dropped until the group service has propagated
the membership. The case retries delivery with a fresh marker (six
attempts) and records a partial when none lands. A pre-made propagated
fixture group would let the full UDP `IM_SESSION_SEND` /
`ChatterBoxInvitation` dual-path assertion run on SL — the remaining
follow-up. Shares the short-group-name and creation-retry fixes in
[[test-group-join-leave-aditi]].
