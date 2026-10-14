---
id: protocol-cap-group-bans
title: Group ban lists over the GroupAPIv1 capability
topic: protocol
status: done
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-social-group-extras]
blocked_by: [gridspec-groups]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants `GroupAPIv1`; Firestorm uses it for group bans only. GET
`{cap}?group_id=<id>` → `{group_id, ban_list:{<agent>:{ban_date}}}`; POST
`{cap}?group_id=<id>` with `{ban_action: 1 create | 2 delete, ban_ids:[uuid]}`,
re-fetching afterwards. There is no UDP equivalent, and OpenSim does not serve
it.

Add commands to read, ban and unban, an event for the list, and the fake grid's
Second Life flavour serving it; the roles-panel UI is
[[viewer-social-group-extras]].

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-groups]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.

## Done (2026-10-14, with gridspec-groups)

`CAP_GROUP_API_V1` is requested; `Command::RequestGroupBans`,
`BanGroupMembers`, `UnbanGroupMembers` and `Event::GroupBans` in both
runtimes and the REPL (a change POSTs, then the list is read again, as the
reference does); the decoder reads Second Life's `group_id`, which is a
string. Measured on aditi: a ban answers `201` "Created", an unban `200`
"OK" (neither LLSD), a banned join is refused with an alert and an
invitation to a banned avatar is not sent (`book/src/gridspec/groups.md`
§ Bans). The fake grid has no groups to ban from, so serving the capability
for its Second Life flavour moved to [[server-fake-grid-groups]]; the panel
is [[viewer-social-group-extras]].
