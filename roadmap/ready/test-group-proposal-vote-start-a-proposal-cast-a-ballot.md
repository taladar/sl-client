---
id: test-group-proposal-vote
title: start a proposal, cast a ballot
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

`group-proposal-vote` — start a proposal, cast a ballot. `2av`. **No live
variant on any grid — group proposals/voting is a removed feature.**
Investigated 2026-06-30 (and confirmed by the project owner): Second Life
**removed group voting entirely** ("vote removal", DEV-24856). The reference
viewer keeps only the power bits `GP_PROPOSAL_START`/`GP_PROPOSAL_VOTE`,
explicitly marked `_DEPRECATED` "as part of vote removal" in
`roles_constants.h`; there is no `panel_group_voting` UI, no send code, and
the four messages are `UDPDeprecated` in `message_template.msg` — so a modern
SL viewer never starts a proposal or casts a ballot, and there is nothing to
observe on SL. **OpenSim has no proposal/voting backend either** — more absent
than group-accounting: `StartGroupProposal` and `GroupProposalBallot` are not
even parsed by `LLClientView` (no `AddLocalPacketHandler`, no
`OnStartGroupProposal`/`OnGroupProposalBallotRequest` event), and
`GroupActiveProposalsRequest`/`GroupVoteHistoryRequest` *are* parsed and fire
their events but no region module subscribes (the `SendGroupActiveProposals`/
`SendGroupVoteHistory` methods are dead code, like the accounting ones), so
none of the four messages yields any reply. The genuine proposal/voting
behaviour is therefore unobservable on **both** grids and not deferrable to
Phase Z (no avatar count makes it work) — same document-and-skip outcome as
`conference-roster`. The client retains the command/event surface
(`Command::StartGroupProposal`/`RequestGroupActiveProposals`/
`GroupProposalBallot`/`RequestGroupVoteHistory`,
`Event::GroupActiveProposals`/`GroupVoteHistory`) for completeness and for the
server-side `SimSession` encoders; a transmit-only "the client still encodes
these legacy messages acceptably" check was prototyped but dropped as
low-value given the feature is dead everywhere.
