---
id: server-fake-grid-attachment-resources
title: Fake grid — list what is worn in the attachment resources answer
topic: server
status: done
origin: fake-grid-own-attachments-and-region-moves (2026-11-07)
refs: [fake-grid-own-attachments-and-region-moves, protocol-avatar-render-info,
  server-lsl-lib-http-url]
---

Context: [context/server.md](../context/server.md).

The fake grid now wears attachments, but its `AttachmentResources` answer is
still what both grids answer an agent wearing nothing scripted:
`attachments: []`, `summary.used` zero. Build the answer measured in
`book/src/gridspec/avatars.md` § Attachment resources: the scripted
attachments by point, HUDs included, named by their LSL constant; each
object's `resources` — `memory` 65,536 per Mono script and `urls` on Second
Life, `urls` alone on OpenSim; `summary.used` as the sums (`memory` 0 on
OpenSim); a URL a prim held before it was worn still held; a taken-off
object gone from the next report.

## Done (2026-11-07)

`attachments::resources_report` builds the answer from what the agent wears,
refreshed at the end of every flush: each worn object holding a script, by
point under its LSL constant (`POINT_NAMES`, held to the runtime's constants
by a unit test), with its id, where it stands, its name and its owner, and
`memory` 65,536 per script on Second Life
(`AttachmentPolicy::script_memory`), none on OpenSim; `summary.used` sums it.
`tests/scripts.rs` holds a worn scripted box's report and its absence once
taken off, on both flavours. URLs are counted as none until a script can hold
one ([[server-lsl-lib-http-url]]), which is also what keeps
`avatar-render-info`'s scripted cube legs live only.
