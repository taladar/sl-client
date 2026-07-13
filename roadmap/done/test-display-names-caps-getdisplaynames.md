---
id: test-display-names
title: CAPS GetDisplayNames
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 7 — Avatar profile & social `[both]`
---

Context: [context/test.md](../context/test.md).

`display-names` — CAPS `GetDisplayNames`. `1av`. Batch-resolve agent
ids to their mutable, user-chosen **display names** (layered over the legacy
`First Last` identity) over the `GetDisplayNames` HTTP capability: the case
drives [`Command::RequestDisplayNames`], batching the agent's own id with a
second known avatar into one GET, and asserts the reply
(`Event::DisplayNames`) resolves the agent's *own* id to a real,
non-`missing` record with a non-empty username, legacy name, and display
name — the observable effect of the cap. **The case only ever reads.** The
client has no set-display-name command at all (the display-name surface is
the `GetDisplayNames` lookup plus observing the
CAPS-pushed `DisplayNameUpdate` / `SetDisplayNameReply`), so it structurally
cannot touch Second Life's multi-day per-avatar display-name-*change* cooldown
and is safe to re-run freely. **Re-gated `[aditi]` → `[both]`:** the legend
lists Display Names under SL-only, but stock OpenSim *does* serve
`GetDisplayNames` whenever its user-management component is present
(`BunchOfCaps.cs`), returning the legacy name as a default
(`is_display_name_default = true`) display name — so the read round-trip is
assertable on both grids. The second avatar (the `other_avatar` fixture on SL,
the local secondary `avatar2` on OpenSim) is added only to exercise
multi-id batching; its resolution is best-effort (recorded, not asserted),
because OpenSim resolves only avatars its region user-management component
already knows — the logged-in agent always, a not-recently-seen fixed-UUID
account not necessarily — and returns unknown ids in `bad_ids` (or silently
omits them) rather than failing. Where a grid omits the capability entirely
the command is a silent no-op and the case records `partial` on the timeout.
Added a `DisplayName` / `DisplayNameUpdate` / `SetDisplayNameReply` re-export
to both runtime crates (the display-name event types were reachable via
`Event` but not nameable). Green on OpenSim: own id resolved, lookup RTT
≈ 70 ms loopback,
`is_display_name_default = true`, secondary unresolved (not region-known).
`[both]`; the aditi run — where custom display names distinct from the legacy
name appear — is deferred with the batch (no aditi record this session).
