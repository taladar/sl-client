---
id: test-chat-invite-accept-decline-aditi-primary-not-a-member
title: chat-invite-accept-decline is always partial on aditi — the primary's group start is refused "unverified"
topic: test
status: done
origin: protocol-group-fetch-history-403-on-aditi live runs (2026-11-19)
refs: [protocol-group-fetch-history-403-on-aditi, test-chat-invite-accept-decline-aditi]
---

Context: [context/test.md](../context/test.md).

## Observation

Every aditi record of `chat-invite-accept-decline`
(`records/aditi/chat-invite-accept-decline.toml`, 2026-11-18 and four runs on
2026-11-19) is a pass with `completeness = "partial"`: "no group-session
invitation was delivered within the retry budget for the accept half". The
secondary never reaches the accept, so the case's Second Life half (the
`accept invitation` / `decline invitation` POSTs) is not exercised at all.

Logging added for [[protocol-group-fetch-history-403-on-aditi]] showed the
primary's `StartGroupSession` answered by a `ChatterBoxSessionStartReply` with
`success=false error=unverified` in every run. The gridspec measured
`unverified` as Second Life's refusal of an agent that is **not a member**
(`book/src/gridspec/chat.md`, group session table), and a non-member's lines
are refused too, which would explain why none of the six markers arrives.

## To do

- Find which group the case used (`support::membership_group`: a pre-made
  fixture group from `ctx.premade_group(index)`, or a fresh throwaway) and
  whether the primary is a member of it on aditi at that moment (its
  `AgentGroupDataUpdate`, or a group-members request).
- If the fixture groups have lost the primary, restore them; if a fresh group
  is not yet known to Second Life's chat service as the founder's, wait for
  the start to succeed (retry the start, not only the line) before sending.
- Have the case fail, or say so in its partial note, when the start is
  refused, rather than spending the retry budget on lines that cannot arrive.

## Resolution (2026-11-19)

The primary **was** a member: it founded the group about two seconds before
starting its session. Every aditi run founded a fresh group, because
`fixtures.aditi.toml` named no `premade_groups`, and Second Life refuses the
founder's start in a group that young with `unverified`, its refusal of a
non-member. A REPL probe started the session of an August group, of the census
group and of the group that morning's run founded (then 2.5 h old): all three
were taken. The founding also cost L$100 a run and left the primary in about
thirty throwaway groups.

- The case now waits for the start reply before it sends, and retries a
  refused start every 5 s for a minute (`START_BUDGET`). A start still refused
  is a partial on aditi naming the refusal and the fix (pre-made groups), and
  a failure on OpenSim. A line that never arrives after a start that was taken
  is a partial only on aditi, and only when the secondary joined during the
  run.
- On a pre-made group the secondary joins only when its membership list lacks
  the group, and it never leaves one on Second Life: an invitation reaches any
  member who has not opened the session, and a group left with its founder
  alone is purged. OpenSim keeps the leave, since a fresh join is what makes
  OpenSim route the first line as an invitation.
- The (gitignored) `fixtures.aditi.toml` now names two old groups both avatars
  are members of.
- The fixtures docs, the runner book page and the gridspec chat page say so.
  The always-`absent` `chat_session_request_cap` metric is gone. It was wrong
  on aditi, and the case checks no capability.

Live: complete on aditi with both answers posted (`Answer` → `200 {}`), each
start taken on its first try and each invitation in under 0.4 s; passes on
OpenSim with throwaway groups.
