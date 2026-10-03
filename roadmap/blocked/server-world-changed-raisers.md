---
id: server-world-changed-raisers
title: Raise changed() from every grid-side cause, with the right bit
topic: server
status: blocked
origin: server-lsl-state-and-events review (2026-09-28)
points: 5
blocked_by: [server-fake-grid-script-engine-wiring, gridspec-lsl-events]
refs: [server-lsl-state-and-events, server-world-link-sets,
  server-lsl-lib-prim-state, server-lsl-lib-task-inventory]
---

Context: [context/lsl.md](../context/lsl.md).

[[server-lsl-state-and-events]] built the fan-in: `Engine::changed(id,
bits)` raises `changed(integer change)` in one script, merging by the
region's `Coalescing` policy (the `Reference` rule measured on aditi: one
tick's raises merge, and a later one joins the newest queued `changed`
unless that one is next in line). What it could not build is the raisers —
the dozen grid-side places a change happens — because the fake grid runs no
scripts yet. This task calls the fan-in from each, for every script in the
object, with the right `CHANGED_*` bit:

| Bit | Raised when |
| --- | --- |
| `CHANGED_INVENTORY` (1) | an item is added to, removed from or renamed in the task inventory (measure which of those raise it) |
| `CHANGED_COLOR` (2) | a face colour or alpha changes, by a script or an edit |
| `CHANGED_SHAPE` (4) | the prim type or its shape parameters change |
| `CHANGED_SCALE` (8) | the prim is resized |
| `CHANGED_TEXTURE` (16) | a texture or its offset/repeats/rotation changes — **not** a scripted `llSetTexture`, which raised nothing on aditi (2026-09-28); measure an edit-floater change |
| `CHANGED_LINK` (32) | a link, an unlink, an avatar sitting or standing |
| `CHANGED_ALLOWED_DROP` (64) | an item dropped in through `llAllowInventoryDrop` |
| `CHANGED_OWNER` (128) | a sale or a give changes the owner |
| `CHANGED_REGION` (256) | the object (or its wearer) crosses into another region |
| `CHANGED_TELEPORT` (512) | its wearer teleports |
| `CHANGED_REGION_START` (1024) | the region restarts |
| `CHANGED_MEDIA` (2048) | a face's shared media changes |

Measured on aditi for the colour and scale rows: `llSetColor`, `llSetAlpha`
and `llSetScale` each raise their bit, and changes made while the script is
idle arrive one event each (twenty colour changes a tenth of a second apart,
twenty events). Measure each other row the same way — a probe script making
the change and reporting its `changed` events — before trusting the table,
and record the answer beside the raiser.

Which scripts hear it matters too: a change to one prim of a link set
raises `changed` in that prim's scripts only (confirm), while
`CHANGED_LINK`, `CHANGED_OWNER`, `CHANGED_REGION` and `CHANGED_TELEPORT`
concern the whole object.

Acceptance: a scenario test drives each raiser on the fake grid and a
script in the object prints the bits it received; every row above either
matches its aditi measurement or is recorded as not raised, as
`llSetTexture` is.
