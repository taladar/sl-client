---
id: server-fake-grid-god-tools
title: Fake grid — god tools as OpenSim answers them
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-god-tools]
---

Context: [context/server.md](../context/server.md).

## What

Every god `ServerEvent` is unmatched. Implement what OpenSim does for a
god-level account; low priority.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured by gridspec-god-tools (2026-10-18)

`book/src/gridspec/estate.md` § God tools has the answers. Already done: the
request for powers per flavour and the `UntrustedSimulatorMessage` statuses
(`sl-fake-grid/src/god_powers.rs`). Left, OpenSim's (Second Life's god side is
unmeasured): the god kick, freeze and unfreeze over both transports — alerts
"/User Frozen", "/User Unfrozen" to the god and the reason, "/"-prefixed, to
the target; a kick's `KickUser` with the reason; the all-agents id kicking
everyone but the god; a lesser god's kick refused in an alert to the target —
all for an administrator (user level ≥ 200 or the region's owner) whether or
not in god mode; `ParcelGodForceOwner` (the parcel's properties to the god);
`ParcelGodMarkAsContent` (the "check ini files" alert without a configured
owner, "Request denied. You're not priviliged." to a non-god); `ObjectOwner`
with override. Then add `god-tools` to the offline list for both flavours.
