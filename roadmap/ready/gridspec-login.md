---
id: gridspec-login
title: Login response fields, the options list and get_grid_info on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-login-options-max-agent-groups-unrequested,
  server-login-service, viewer-login-screen]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

SL honours the `options` list, OpenSim sends every field
(`ImitatedGrid::honors_login_options`); SL sends `account_type`,
`account_level_benefits`, `premium_packages`, `agent_region_access`, OpenSim
none and hard-codes `agent_access=M` / `agent_access_max=A`. Aditi sends
`agent_access="M"` even with both ceiling and preference "A" (unexplained,
`login_handshake.rs`). Whether SL gates `max-agent-groups` behind its option
is open ([[protocol-login-options-max-agent-groups-unrequested]]). The fake
grid's `get_grid_info` always says `platform: OpenSim`; what aditi's login
host answers is unmeasured.

## Discover

- Extend `login-handshake` to record every top-level field present, per
  option requested; run on aditi and OpenSim (one avatar). `sl-repl --script`
  dumps the raw response.
- Request each option alone and in combination on aditi to map option →
  fields; check the LLSD login is accepted on SL.
- `curl` both login hosts for `get_grid_info` (XML and the XML-RPC method).

## Document

`book/src/gridspec/login.md`; cross-link from `content/login.md`.

## Fake grid

Small — in this task: field-by-field presence per flavour in
`filter_options` / `prepare_session`, the measured access strings, the
`get_grid_info` answer per flavour.

## Viewer

Request every option the viewer reads (the `max-agent-groups` bug); tolerate
absent benefits / preference on OpenSim (fall back to `EconomyData`).
`e2e` login on both fake flavours.
