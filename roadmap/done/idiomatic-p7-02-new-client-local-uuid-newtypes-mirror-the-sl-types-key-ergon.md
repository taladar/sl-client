---
id: idiomatic-p7-02
title: New client-local UUID newtypes (mirror the sl-types key ergonomics Fro
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

New client-local UUID newtypes (mirror the `sl-types` key ergonomics
    `From<Uuid>`/`uuid()`/`Display`): **`PickKey`** (avatar_profile.rs — the
    picks-side parallel of `ClassifiedKey`;
    `AvatarPick`/`PickInfo`/`PickUpdate` `pick_id`, the
    `RequestPickInfo`/`DeletePick`/`GodDeletePick` commands + methods),
    **`GroupNoticeKey`** (`GroupNotice.notice_id` + `RequestGroupNotice`),
    **`ProposalVoteId`** (`GroupActiveProposalItem`/`GroupVoteHistoryItem`
    `vote_id`, the ballot `proposal_id`), **`ProposalCandidateId`**
    (`GroupVote.candidate_id`, a distinct type from `ProposalVoteId`).
    Re-exported through both runtimes; REPL parses raw `Uuid` then wraps; +3
    unit tests. **Left raw (deliberately):** the
    `*_request_id`/`query_id`/`transaction_id` correlation ids and session
    tokens (no entity identity). (commit "Phase 7 A2")

**B — in-band sentinel → `Option` (maximal; IN PROGRESS):**
