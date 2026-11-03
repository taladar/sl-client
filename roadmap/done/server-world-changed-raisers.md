---
id: server-world-changed-raisers
title: Raise changed() from every grid-side cause, with the right bit
topic: server
status: done
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

## From gridspec-lsl-events (2026-10-25)

`events/merging` and `events/order` (`book/src/gridspec/lsl.md` § Events),
changes made by a helper script while the probe slept:

- **Second Life merges, OpenSim never does** (both engines;
  `Coalescing::Never`): five colours in one handler are one `changed` on
  Second Life and five on OpenSim; three half a second apart, queued behind
  another event, one against three; colour, scale and texture in one
  handler one `changed` of 10 (no texture bit, as before) against three of
  2, 8 and 16 — OpenSim's `llSetTexture` raises `CHANGED_TEXTURE`.
- **A colour set to the one it has** raises `CHANGED_COLOR` on Second Life
  and nothing on OpenSim.
- **Where it lands in the queue**: on Second Life a `changed` raised while
  the script slept comes after every link message, line of chat, timer,
  sensor answer and touch raised in the same sleep, whenever it was raised;
  on OpenSim it keeps its place (first in, first out across every type).
- **OpenSim raises its own bit 32768** (`Changed.POSITION`, no LSL
  constant) on an attach from the ground (twice), a drop (once) and an
  `llAttachToAvatar` (twice); Second Life raises no `changed` for any of them
  (`events/attach`). An OpenSim flavour that is faithful raises it too.

## From server-world-sit-and-attach (2026-11-02)

Raised now: `CHANGED_LINK` in every script of an object on a sit and a stand
(`scripts::raise_changed`), and OpenSim's `changed(32768)` twice before an
`attach` on a wear and once on a drop (`scripts::raise_attach`). An
OpenSim-flavoured region's engine now uses `Coalescing::Never`.

## From server-world-touch-and-grab (2026-11-02)

Where a `changed` lands in the queue is done: under `Coalescing::Reference`
the queue is ranked (`Coalescing::rank`), a touch behind every other kind of
event and a `changed` behind the touches, whenever each was raised
(`second_life_runs_touches_after_the_rest_and_changed_last`). What is left
here is raising the changes; the `events/order` rows of
`book/src/gridspec/lsl.md` stay unasked offline until the colour change does.

## From server-world-link-sets (2026-11-03)

Raised now: `CHANGED_LINK` in every prim of the sets a viewer's `ObjectLink`
or `ObjectDelink` touched (`link_sets::announce_link` / `announce_delink`),
and for a script's `llCreateLink`, `llBreakLink` and `llBreakAllLinks` as
each flavour's grid tells it (`imitates::LinkEvents`), OpenSim's newcomer
first hearing its own 32768. `scripts::raise_changed_in` raises a bit in the
scripts of named prims alone, for the per-prim rows of the table above.

## Done (2026-11-03)

Measured the grid-side causes by four new probes, `events/changed-edits`,
`events/changed-contents`, `events/changed-owner` and `events/changed-worn`,
run on aditi and on the local OpenSim under YEngine and XEngine
(`book/src/gridspec/lsl.md` § `changed`: what a viewer's change raises). The
probe runner got three drives for them: `edit` (a build-floater change of a
prim's faces, size, place, shape, light, name, description, phantom flag or face
media), `contents` (a drop — by the other avatar too — a contents-tab rename, a
remove, a new script and a save) and `buy` (the owner sells prim A for L$ 0, the
other avatar buys it), and skips at teardown a prim the region already removed.

Raised now on the fake grid (`sl_fake_grid::changes`, the disagreements in
`ImitatedGrid::change_policy`): a face's colour or alpha `CHANGED_COLOR`, any
other face field `CHANGED_TEXTURE`, each only when it changed; a resize
`CHANGED_SCALE` every time, a child's heard by the root on Second Life and by
the child on OpenSim; a move or a turn OpenSim's 32768 (root moved: once in the
root, twice in each child; root turned: once in each child; a child moved: once
in it) and nothing on Second Life; a shape `CHANGED_SHAPE`; face media set or
cleared `CHANGED_MEDIA`; a drop, a contents-tab rename (which the fake grid now
takes), a remove, a new script and a save `CHANGED_INVENTORY` in the prim
written. A drop by an avatar who does not own the prim is refused unless the
prim allows drops — `llAllowInventoryDrop`, now in the runtime, sets
`FLAGS_ALLOW_INVENTORY_DROP` — and then raises `CHANGED_ALLOWED_DROP`; refused,
Second Life tells the dropper `Unable to edit this!` and OpenSim nothing. Held
by `tests/scripts.rs`'s `a_viewers_changes_raise_changed` on both flavours.

Not raised, each for want of the cause on the fake grid, and handed on with
its measurement: `CHANGED_OWNER` on a purchase
([[server-fake-grid-money-ledger]]: once on Second Life, three times on
OpenSim), `CHANGED_REGION | CHANGED_TELEPORT` of a worn object
([[fake-grid-own-attachments-and-region-moves]]), the scripted setters' rows
of `prim/changed` ([[server-lsl-lib-prim-state]]) and `llRemoveInventory` and
`llGiveInventory` into a prim ([[server-lsl-lib-task-inventory]]). Not
measured, each its own task: an owner change by a deed to a group (no group on
the aditi accounts; [[server-world-changed-owner-deed]]), and a crossing of an
unworn object, which the fake grid does not make either
([[server-world-changed-object-crossing]]).
