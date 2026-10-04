# Login

What each grid's login service answers: which fields a successful response
carries, which of them the request's `options` list controls, the account
fields, the transports it accepts, and `get_grid_info`. The protocol itself is
described in [Login](../content/login.md).

Measured on 2026-10-04 on Second Life's beta grid (aditi, two `Base`
accounts) and the local OpenSim standalone, by the conformance cases
`login-options` (the same avatar logged in three times: with the client's
default `options`, with an empty list, and with every option the reference
viewer knows) and `login-handshake`, and by direct HTTP requests. Each case
holds both grids, and both fake-grid flavours, to these answers as
`Measured` constants, so a grid that changes, or a fake grid that drifts,
fails the case.

## The `options` list

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| honours the list | yes: an empty list gets 34 fields, every option 47 | no: the same 39 fields whatever was asked | `FakeSl` honours it, `FakeOpensim` ignores it |
| gated sections (sent only when asked) | `inventory-root`, `inventory-skeleton`, `inventory-lib-root`, `inventory-lib-owner`, `inventory-skel-lib`, `gestures`, `login-flags`, `global-textures`, `ui-config`, `event_categories`, `classified_categories`, `initial-outfit`, `tutorial_setting` | none | `FakeSl` gates the inventory and library sections (the only ones it fills) |
| `max-agent-groups` without its option | sent | sent | sent by both |
| `map-server-url` without its option | sent | sent | sent by both |
| options that got nothing back | `buddy-list`, `voice-config`, `newuser-config`, `event_notifications`, `display_names`, `adult_compliant`, `advanced-mode`, `currency`, `max_groups`, `search`, `destination_guide_url`, `avatar_picker_url` | — (the list is ignored) | — |

`buddy-list` came back on neither aditi account even when asked for; whether
either held a friend at the time was not checked, so "omitted when empty" and
"never sent" are not yet told apart. OpenSim sends an empty `buddy-list` array.

## Fields by grid

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `home` | **absent**, even with every option | sent | `FakeOpensim` only |
| `look_at` | sent | sent | both |
| `region_size_x` / `region_size_y` | absent | sent | `FakeOpensim` only |
| `inventory-lib-owner` | sent when asked | sent | both (gated on `FakeSl`) |
| `voice-config` | **absent**, even when asked | absent | neither |
| `currency` | **absent**, even when asked | absent (stock config) | neither |
| `max-agent-groups` | 50 for `Base` (its `group_membership_limit`) | 42 (the login service default) | the package's limit / 42 |
| `account_type`, `account_level_benefits`, `premium_packages`, `agent_region_access` | sent | absent | `FakeSl` only |
| `openid_url`, `openid_token` | sent | absent | not sent (no OpenID endpoint) |
| `agent_flags`, `cof_version`, `god_level`, `max_god_level`, `is_admin_login`, `Linden_Status_Code`, `udp_blacklist` | sent | absent | not sent |
| `http_port`, `real_id`, `event_notifications` | absent | sent | not sent |
| gated content sections (`gestures`, `login-flags`, `global-textures`, `ui-config`, the category lists, `initial-outfit`, `tutorial_setting`) | sent when asked | sent (those it has) | not sent |

The last four rows are fields no viewer this workspace builds reads. The
reference viewer reads none of the Second Life-only scalars in the second to
last row either. The fake grid does not reproduce them yet (roadmap
`server-fake-grid-login-response-sections`).

## Account fields

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `agent_access` / `agent_access_max` | `M` / `A` for both test accounts (per account) | `M` / `A` for every account | per account config / `M` / `A` |
| `agent_region_access` (the preference) | `A` | absent | `FakeSl` only |
| `account_type` | `Base` | absent | `FakeSl` only |

## Transport

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| XML-RPC login (`text/xml`) | accepted; the reply is labelled `application/llsd+xml` although it is XML-RPC | accepted, `text/xml` reply | accepted |
| LLSD login (`application/llsd+xml`) | accepted, LLSD reply | accepted, LLSD reply | accepted |
| a body sent as `application/xml` | `400`: parsed as XML-RPC and refused | not measured | — |

Measured with a login for an account that does not exist, so no account was
touched: both grids answered `reason: key` in the request's own encoding.

## `get_grid_info`

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `GET <login host>/get_grid_info` | `403 Forbidden` | `gridname`, `gridnick`, `platform: OpenSim`, `login` (served as `text/html`) | served, `platform: OpenSim` |
| the XML-RPC `get_grid_info` method | `500` ("failed to rez") | the same four keys | served |

The fake grid serves `get_grid_info` on both flavours on purpose: it is how
Firestorm's grid manager adds a grid it does not know, so a fake grid that
refused it could not be logged into at all. It says `platform: OpenSim` for
the same reason. Second Life's own grids are built into the viewers.

## What the viewer does with it

- It asks for every option it reads, `max-agent-groups` and `map-server-url`
  included as the reference does, although aditi sends both unasked.
- It falls back to `agent_access_max` when no `agent_region_access` arrives
  (OpenSim).
- No `home` on Second Life: the viewer must not rely on it.
- Upload prices come from the benefits package on Second Life and from
  `EconomyData` on OpenSim. No paid upload exists in the viewer yet; the
  requirement is recorded on `viewer-image-upload`.
- The e2e test `e2e_login` logs in on both fake flavours, through a second
  factor, and past a terms-of-service and a critical-message gate.
