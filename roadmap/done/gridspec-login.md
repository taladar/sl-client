---
id: gridspec-login
title: Login response fields, the options list and get_grid_info on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-login-options-max-agent-groups-unrequested,
  server-login-service, viewer-login-screen]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-04)

Measured and written up in `book/src/gridspec/login.md`.

- **Discover.** `LoginSuccess::response_fields` (and `LoginAccount`'s copy)
  now lists every top-level field a response carried, sorted.
  `login-handshake` records it; the new `login-options` case logs one avatar in
  three times (default / no / every option) and records each set. Run on aditi
  (two accounts), OpenSim and both fake flavours. The per-option mapping was
  read from the none/every difference rather than one login per option: every
  gated field has the name of its option, so the difference is the mapping. The
  LLSD login and `get_grid_info` were probed with plain HTTP (a non-existent
  account, so nothing was touched).
- **Findings.** aditi gates 13 sections and sends `max-agent-groups` and
  `map-server-url` unasked; it never sent `home`, `voice-config`, `currency` or
  `buddy-list`, even when asked. OpenSim ignores the list. Both accept an LLSD
  login; aditi refuses `get_grid_info` (403 / 500).
- **Fake grid.** Per flavour: `home` and region size only on OpenSim, `look_at`,
  `max-agent-groups` (package limit / 42) and `inventory-lib-owner` on both, no
  `voice-config` or `currency` on either (`ImitatedGrid::login_fields`);
  `filter_options` stops gating `max-agent-groups` / `map-server-url`.
  `login-options` and `login-handshake` hold all four grids to the measured
  answers as `Measured` constants. The sections no viewer here reads are
  [[server-fake-grid-login-response-sections]].
- **Viewer.** The default options ask for `max-agent-groups`; the maturity
  fallback already existed; the upload-price source is recorded on
  [[viewer-image-upload]]. `e2e_login` logs in on both flavours (and loads the
  Library), through MFA, and past ToS / critical gates — which found and fixed
  two MFA bugs: the binary never answered a challenge (`ViewerApp::run` read
  the outcome after `App::run` emptied the App) and the bevy client raced the
  challenge against a spurious `ProtocolError` disconnect. `sl-e2e` gained
  `StageBuilder::mfa` and reconnects a process viewer that restarts its app to
  answer one.
- Closed on the way: [[protocol-login-options-max-agent-groups-unrequested]],
  [[viewer-login-voice-config-unread]].

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
