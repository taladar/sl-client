---
id: test-group-notice
title: send and receive a group notice
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

`group-notice` — send and receive a group notice. `2av`. The
group's **one-shot announcement** path, complementing
`group-session-message`'s live
conversation and `group-join-leave`'s membership churn: the primary owns the
group and posts a notice (`Command::SendGroupNotice`, an `IM_GROUP_NOTICE`
whose subject and body are joined with a `|` on the wire), while the
**secondary** — having joined the open-enrollment group — is the member that
receives it. `2av` is intrinsic: send-and-receive needs a receiver distinct
from the poster (the grid also relays the notice back to the founder, but
proving delivery to *another* avatar is the point). A freshly joined member
accepts notices by default (`AcceptNotices = "1"`), so no accept-notices
toggle is needed. The relayed IM is attributed *from the group* —
`from_group` set, `from_agent_id` the group id, not the posting avatar — so
the receive predicate keys on the `GroupNotice` dialog and the exact
`subject|body` rather than a sender id; its session id (`InstantMessage::id`)
is the new notice's id. After
observing the live delivery the case cross-checks persistence: the primary
fetches the notice history (`RequestGroupNotices` → `Event::GroupNotices`) and
asserts the just-posted notice is present with the same id and subject and no
attachment — proving the notice was stored, not merely echoed, exercising the
`GroupNoticesListReply` read path alongside IM delivery. The group comes from
`support::membership_group` (index 0): a throwaway created per run on OpenSim,
or a reused pre-made group on Second Life; the secondary leaves any group it
joined, restoring a reused fixture to its founder-only state. Green on OpenSim
(local secondary `Friend Tester`, Groups V2 enabled): create ≈ 0.32 s, join
≈ 0.06 s, notice deliver ≈ 44 ms, history fetch ≈ 16 ms loopback (1 listed
notice). `[both]`; the Aditi run is deferred with the batch (needs a second
Aditi avatar and a configured pre-made group; no aditi record this session).
