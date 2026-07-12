---
id: api-g3
title: Display names (CAPS, SL-priority)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G3 — Display names (CAPS, SL-priority)

`GetDisplayNames` capability → display-name resolution (`DisplayName` type:
username, display name, legacy first/last, expiry), complementing the existing
legacy-name `RequestAvatarNames`. Command `RequestDisplayNames`, Event
`DisplayNames`; server encodes the LLSD reply. SL-only (guard on the cap).

- [x] G3 display-name resolution. New `sl-wire/src/display_name.rs`:
  `DisplayName` (id, username/SLID, mutable display name, legacy first/last,
  `is_display_name_default`, expiry/next-update timestamps, `missing`) +
  `display_names_query` / `parse_display_names` (client) and
  `parse_display_names_query` / `build_display_names_response` (server, the
  inverse). Cap `GetDisplayNames` (`CAP_GET_DISPLAY_NAMES`, added to
  `REQUESTED_CAPABILITIES`); command `RequestDisplayNames(Vec<Uuid>)`; event
  `DisplayNames(Vec<DisplayName>)` decoded in `handle_caps_event`. Both runtimes
  (GET via the existing caps pipeline) + REPL (`request_display_names`) + tests
  (2 wire-level decode/round-trip + 1 proto `handle_caps_event` decode) + book
  (`region.md` "Display names" alongside the legacy-name resolution).
  OpenSim serves it only with its user-management component present, so the
  command is a no-op when the seed omits the cap.
