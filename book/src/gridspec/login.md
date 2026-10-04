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
| gated sections (sent only when asked) | `inventory-root`, `inventory-skeleton`, `inventory-lib-root`, `inventory-lib-owner`, `inventory-skel-lib`, `gestures`, `login-flags`, `global-textures`, `ui-config`, `event_categories`, `classified_categories`, `initial-outfit`, `tutorial_setting` | none | `FakeSl` gates the same set |
| `max-agent-groups` without its option | sent | sent | sent by both |
| `map-server-url` without its option | sent | sent | sent by both |
| options that got nothing back | `buddy-list`, `voice-config`, `newuser-config`, `event_notifications`, `display_names`, `adult_compliant`, `advanced-mode`, `currency`, `max_groups`, `search`, `destination_guide_url`, `avatar_picker_url` | — (the list is ignored) | — |

`buddy-list` came back on neither aditi account when asked for, because
neither had a friend: `login-buddy-list` made the two accounts friends, logged
one in again, and got a one-entry `buddy-list` on both grids. So Second Life
**leaves out an empty list**, where OpenSim sends it as an empty array — as it
does `gestures`, `event_categories` and `event_notifications`
(`LoginSuccess::empty_lists` keeps the difference).

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
| `openid_url`, `openid_token` | sent (`openid_url` is the string `None` on one of the three accounts) | absent | **not sent** — see below |
| `agent_flags`, `cof_version`, `god_level`, `max_god_level`, `is_admin_login`, `Linden_Status_Code`, `udp_blacklist` | sent | absent | `FakeSl` |
| `http_port`, `real_id` | absent | sent (`0`, nil) | `FakeOpensim` |
| `event_notifications` | absent | sent, empty | `FakeOpensim`, empty |
| `gestures` | the account's active gestures | sent, empty for an account with none | the account's (none), empty on `FakeOpensim` |

The fake grid does not send `openid_url` / `openid_token`: a viewer POSTs the
token to that URL at login to mint the web session cookie, and the fake grid
serves no OpenID endpoint for it to reach. The reference viewer reads none of
the Second Life-only scalars in the second row.

## Content sections

What each section held, as `login-options` recorded it from the every-option
login and holds both fake flavours to:

| section | Second Life | OpenSim |
| --- | --- | --- |
| `classified_categories` | the nine (1 Shopping … 9 Personal) | the same nine |
| `event_categories` | thirteen (18 Discussion … 31 Spirituality) | an empty array |
| `global-textures` | sun `cce0f112…`, moon `d07f6eed…`, cloud `fc4b9f0b…` | sun `cce0f112…`, moon `ec4b9f0b…`, cloud `dc4b9f0b…` |
| `login-flags` | `ever_logged_in` Y, `gendered` Y, `stipend_since_login` N, `daylight_savings` Y | the same |
| `ui-config` | `allow_first_life` Y | the same |
| `initial-outfit` | an empty struct | `Nightclub Female`, `female` |
| `tutorial_setting` | two entries: `tutorial_url` = the orientation page, then `use_tutorial` = empty | absent |
| `udp_blacklist` | `EnableSimulator,TeleportFinish,CrossedRegion,OpenCircuit` | absent |
| `cof_version` | the Current Outfit Folder's version (1) | absent |

`daylight_savings` is Pacific daylight-saving time, which the fake grid works
out from its clock rather than fixing at the measured `Y`.

Second Life types its integers `<int>`, OpenSim `<i4>`; both are the same
XML-RPC type.

## Account fields

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `agent_access` / `agent_access_max` | per account: `M` / `A` on two test accounts, `A` / `A` on the third | `M` / `A` for every account | per account config / `M` / `A` |
| `agent_region_access` (the preference) | `A` on two accounts, `M` on the third | absent | `FakeSl` only |
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
