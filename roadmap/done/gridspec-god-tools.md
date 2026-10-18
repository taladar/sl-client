---
id: gridspec-god-tools
title: God tools as OpenSim answers them
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-god-tools]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Protocol only; no god account on SL.

## Discover

OpenSim with a god-level account; SL from source only.

## Document

`book/src/gridspec/estate.md` § God tools.

## Fake grid

Large, low priority — [[server-fake-grid-god-tools]].

## Viewer

God UI gated on `GrantGodlikePowers`.

## Done (2026-10-18)

The `god-tools` census (3av, live grids) asks each avatar for god powers and
gives them up, has a resident freeze a god over UDP and over
`UntrustedSimulatorMessage` (once naming the god with its own session), post a
message the capability does not know, and send every god request — a
`GodlikeMessage`, `GodUpdateRegionInfo` with the region's own values,
`SimWideDeletes`, a forced parcel owner, a mark as content, a forced cube
owner — and on OpenSim has a grid god (a new user-level-250 account,
`grid-god` in the gitignored `credentials.toml`) act out of and in god mode
(`book/src/gridspec/estate.md` § God tools). Findings:

- Second Life refuses a resident's request for powers with an `AlertMessage`
  keyed `SpecialPowersRequestFailedLogged`, giving them up with nothing; its
  `UntrustedSimulatorMessage` answers any `GodKickUser` `200 undef` and an
  unknown message `404 Not Found`; every other god request is silent. No test
  account is a god there, so a god's answers are unmeasured.
- OpenSim grants a god account its user level and the estate owner 220, only
  when the level told changes; a resident is told "/Request for god powers
  denied". Its `Permissions.IsGod` is "an administrator" (user level ≥ 200 or
  the region's owner) whatever the god mode, so a grid god freezes and forces
  owners out of god mode; only mark-as-content looks at god mode. The cap
  answers `200` no body, `401` for a god session that is not the god's,
  `400` for an unknown message. A lesser god's kick of a greater one is
  refused in an alert to the *target*. `GodlikeMessage`, `GodUpdateRegionInfo`
  and `SimWideDeletes` have no handler.

Client: `Command::GodKickUser` takes a `GodKickTarget` (an agent or everyone)
and a `GodKickAction` (kick, freeze, unfreeze — it could only kick before) and
is posted to `UntrustedSimulatorMessage` where granted, as the reference routes
it, the UDP message otherwise (`CAP_UNTRUSTED_SIMULATOR_MESSAGE` requested;
codec `sl-wire/src/untrusted_message.rs`); `Command::ForceObjectOwner` (the
god's `ObjectOwner` with `Override`) is new; `GodRegionUpdate`'s redirect is the
signed pair the region states (Second Life's `-1,-1` could not be carried by the
unsigned `GridCoordinates`, and the server side clamped it to 0). Both runtimes
and sl-repl.

Fake grid: `AccountConfig::god_level`; a request for powers answered per
flavour (`imitates.rs` `god_powers`, `sl-fake-grid/src/god_powers.rs`);
`UntrustedSimulatorMessage` served, answered in each grid's words
(`untrusted_message_style`). The god's actions are
[[server-fake-grid-god-tools]], now ready.

Viewer: the agent's god level is kept (`AgentGodLevel`) and every grant raises
the reference's `EnteringGodMode` / `LeavingGodMode` tip; both grids' refusals
already showed. The floater is [[viewer-god-tools]].
