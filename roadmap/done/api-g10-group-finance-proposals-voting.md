---
id: api-g10
title: Group finance, proposals & voting
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G10 — Group finance, proposals & voting

`GroupAccountSummaryRequest`/`Reply`, `GroupAccountDetailsRequest`/`Reply`,
`GroupAccountTransactionsRequest`/`Reply`; `GroupActiveProposalsRequest`,
`StartGroupProposal`, `GroupProposalBallot`, `GroupVoteHistoryRequest` and their
replies. OpenSim-testable (Groups V2 + money module).

- [x] G10 group finance, proposals, voting. New types in
  `sl-proto/src/types/group.rs`: `GroupAccountSummary`, `GroupAccountDetails`
  (+ `GroupAccountDetailsEntry`), `GroupAccountTransactions` (+
  `GroupAccountTransaction`), `GroupActiveProposalItem`, `GroupVoteHistoryItem`
  (+ `GroupVote`). Commands `RequestGroupAccountSummary`/`Details`/
  `Transactions` (each takes a client-chosen `request_id` echoed in the reply +
  an `interval_days`/`current_interval` accounting window),
  `RequestGroupActiveProposals`, `RequestGroupVoteHistory` (each takes a
  client-chosen `transaction_id`), `StartGroupProposal`, `GroupProposalBallot`;
  circuit encoders + `Session` methods (`request_group_account_*`,
  `request_group_active_proposals`, `request_group_vote_history`,
  `start_group_proposal`, `cast_group_proposal_ballot`). Events
  `GroupAccountSummary`/`GroupAccountDetails`/`GroupAccountTransactions`
  (struct-wrapped) and `GroupActiveProposals`/`GroupVoteHistory` (inline fields
  carrying `group_id`/`transaction_id`/`total_num_items` + the per-item list),
  decoded in the dispatch path via per-type helpers in
  `session/conversions.rs`. Server: each of the 7 client requests surfaces as a
  matching `ServerEvent`, and `SimSession` gains the 5 reply encoders
  (`send_group_account_summary_reply`/`_details_reply`/`_transactions_reply`/
  `send_group_active_proposals_reply`/`send_group_vote_history_reply`). Both
  runtimes + REPL (7 commands) + format.rs event/command names. Tests: 2
  lifecycle client (commands encode, summary/active-proposals reply decode) + 1
  loopback round-trip (all 7 requests + all 5 replies) + 4 REPL registry. Book:
  extended `content/groups.md` with a "Finance, proposals & voting" section +
  "In this codebase". **Scope note:** `StartGroupProposal`/`GroupProposalBallot`
  are
  `UDPDeprecated` but still wrapped (maximal scope, following the G6
  `RezRestoreToWorld` precedent); the `*Reply` messages are `Trusted` but
  viewer-facing (dataserver→sim→viewer), so they ARE wrapped as `Event`s +
  `SimSession` encoders (G7 `ParcelInfoReply` precedent), unlike the
  sim↔dataserver backend-only `*Backend` messages; `TallyVotes` (Low 365,
  userserver→dataserver `Trusted`) is out of scope. OpenSim-testable (needs
  Groups V2 plus a money module) but NOT live-tested this session (loopback +
  lifecycle cover both directions).
