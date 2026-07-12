---
id: protocol-45
title: ChatterBoxInvitation session type & bucket (extends #28, Tier A)
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**45. `ChatterBoxInvitation` session type & bucket (extends #28, Tier A). ✅
Done.** `chatterbox_invitation_from_llsd` (`session.rs`) read only
`id`/`from_id`/`from_name`/`message` from `message_params`, dropping `type` (the
session kind — group vs. ad-hoc conference vs. P2P) and `binary_bucket` (which
for a group IM carries the group/session name used to label the session), plus
`from_group`/`region_id`/`position`/`timestamp` — so a client could surface that
*an* invitation arrived but not classify or name the session it was being asked
to join. Added the full surface to `Event::ConferenceInvited`: **`dialog`** (a
typed `ImDialog` from the `type` byte — `SessionGroupStart` vs.
`SessionConferenceStart` vs. a plain add), **`from_group`** (group IM vs. ad-hoc
conference; for a group IM the `session_id` is the group id), **`session_name`**
(the human-readable label, taken from the event body's top-level `session_name`
that OpenSim supplies), **`binary_bucket`** (the dialog-dependent payload — for
a group IM the group/session name — read from the
`message_params.data.binary_bucket` nesting both OpenSim and the reference
viewer use, with a fallback to a flat `binary_bucket`), and the source
`region_id`/`position`/`parent_estate_id`/`timestamp`. The cross-checks:
OpenSim's `EventQueueGetHandlers.InstantMessageBody`
(field names, the `data.binary_bucket` nesting, `type`/`from_group`) and the
viewer's `LLViewerChatterBoxInvitation::post` (which reads the same
`message_params["data"]["binary_bucket"]`, `region_id`, `position`,
`parent_estate_id`, `timestamp`). A new `llsd_position` helper reads the
`[x, y, z]` real array. The event flows unchanged through both runtimes (every
consumer binds it with `{ .. }`; the `tokio_login_hold_logout` example now logs
the session name, dialog, and `from_group`). Covered by the extended
`chatterbox_invitation_surfaces_conference_invited` `sl-proto` lifecycle test (a
group-start invite with the `data`-nested bucket → `dialog=SessionGroupStart`,
`from_group=true`, `session_name="My Group"`, the decoded bucket, region id,
`position=(1.5, 2.5, 3.5)`, estate id, and timestamp all round-trip).
*Unit-tested only: stock OpenSim emits no CAPS `ChatterBoxInvitation` (its group
IM uses the UDP `ImprovedInstantMessage` path #7 already covers), so the CAPS
delivery is exercised by the deterministic lifecycle test rather than the local
grid. Test: SL grid.*
