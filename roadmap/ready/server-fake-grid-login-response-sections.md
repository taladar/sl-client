---
id: server-fake-grid-login-response-sections
title: The fake grid's login response lacks the sections and scalars the live grids send
topic: server
status: ready
origin: gridspec-login (2026-10-04)
refs: [gridspec-login, server-login-service]
---

Context: [context/server.md](../context/server.md).

[[gridspec-login]] measured, field by field, what aditi and the local OpenSim
put in a successful login response (`book/src/gridspec/login.md`), and made
the fake grid match on every field a viewer here reads: `home`, `look_at`,
`max-agent-groups`, `map-server-url`, `inventory-lib-owner`, the region size,
and no `voice-config` or `currency` on either flavour. What it left is the
rest, which no viewer in this workspace reads yet:

- **The gated content sections** — `gestures`, `login-flags`,
  `global-textures`, `ui-config`, `event_categories`,
  `classified_categories`, `initial-outfit`, `tutorial_setting` — which aditi
  sends when asked and OpenSim always sends. The fake grid sends none. Their
  *values* were not recorded (the case records field names), so a capture of
  each grid's values comes first: extend `login-options` to record each
  section's contents, or dump one response per grid with `sl-repl`.
- **OpenSim-only fields**: `http_port`, `real_id`, `event_notifications` (an
  empty array), and an empty `buddy-list` array where OpenSim sends one for an
  account with no friends. `sl-wire` drops empty arrays on the way out, so this
  needs the encoder to tell "empty" from "absent".
- **Second Life-only scalars**: `agent_flags`, `cof_version`, `god_level`,
  `max_god_level`, `is_admin_login`, `Linden_Status_Code`, `udp_blacklist`,
  plus `openid_url` / `openid_token`. `LoginSuccess` has no field for the first
  six (they arrive only in `response_fields`), and the OpenID pair needs an
  endpoint the fake grid does not serve.

Done when each is a row in `login-options`' `PRESENCE` table held on both
fake flavours, or is written down in the book as deliberately not imitated
and why.
