---
id: test-chat-invite-accept-decline-aditi-primary-not-a-member
title: chat-invite-accept-decline is always partial on aditi — the primary's group start is refused "unverified"
topic: test
status: bugs
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
