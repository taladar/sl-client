---
id: test-mute-list
title: mute / unmute and fetch the mute list
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 7 — Avatar profile & social `[both]`
---

Context: [context/test.md](../context/test.md).

`mute-list` — mute / unmute and fetch the mute list. `1av`.
The agent's own private block list: a full add → read-back → remove →
read-back round-trip over the per-account mute list the simulator keeps.
Reading is [`Command::RequestMuteList`] (`MuteListRequest` with a zero CRC,
forcing a fresh download); the simulator answers by uploading the list file
over the `Xfer` path behind a `MuteListUpdate` (surfaced, once downloaded and
parsed, as [`Event::MuteList`]) or — for an empty list — with the
`emptymutelist` `GenericMessage` (also [`Event::MuteList`]`([])`). Adding is
[`Command::Mute`] (`UpdateMuteListEntry`), removing is [`Command::Unmute`]
(`RemoveMuteListEntry`); neither carries an ack, so each edit is verified by
re-requesting the list until the change shows. The case mutes a **fixed
synthetic target** (a conformance-owned UUID + name, muted as a
[`MuteType::Agent`] with default mute-everything flags): nothing external is
touched — a mute is private block-list state, the target need not be a real
account, and the fixed id means a re-run edits the one marker rather than
piling up. Because the round-trip *is* add-then-remove it leaves the list as
it found it (marker absent) with no separate restore step, and an interrupted
run self-heals since the next run's remove sweeps a leftover marker. That
makes the case grid-agnostic and free of any `other_avatar` fixture or
cooldown concern (muting has no display-name-style change cooldown, so it is
safe to re-run freely). Unlike every prior Phase 7 case this one needed **no
new client code and no new runtime-crate re-exports** — the mute
Command/Event/Session surface and the `MuteEntry`/`MuteFlags`/`MuteType`
re-exports all already existed — so it is a pure new conformance case. Stock
OpenSim serves the whole round-trip once its `MuteListModule` (+
`MuteListService`) is enabled (both already on in this workspace's
`OpenSim.ini`); its SQLite mute-delete works cleanly (no data-layer bug,
unlike `picks-classifieds`' classified delete). With the module absent the
simulator's default handler answers a read with an empty list but drops the
write, so the entry never appears — the case detects that (the add never
surfaces), records the write as exercised, best-effort cleans up, and marks
`partial` rather than failing. Green on OpenSim: baseline 0 mutes, entry
muted (type agent, default flags) then unmuted, `mute_rtt` ≈ 1.0 s /
`unmute_rtt` ≈ 1.1 s (both poll-interval-bound, not the sub-poll server
latency). `[both]`; the aditi run — where Second Life serves it natively — is
deferred with the batch (no aditi record this session).
