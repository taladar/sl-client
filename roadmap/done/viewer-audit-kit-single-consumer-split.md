---
id: viewer-audit-kit-single-consumer-split
title: sl-viewer-kit is a grab-bag: 39% of it has exactly one consumer
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [build-split-ui-widgets-crate]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-kit/src/lib.rs:4` opened "This is a deliberately mixed bag, and the
mix is the point." Eight of its twenty modules had exactly **one** consumer
crate, and they have moved to it.

| module | lines | now in | chain above it, before → after |
| --- | ---: | --- | --- |
| `radar_model` | 1027 | `sl-viewer-people` | 157.6 s → 47.3 s |
| `shadow_visibility` | 740 | `sl-client-bevy-viewer` | 157.6 s → 19.5 s |
| `edit_math` | 650 | `sl-viewer-edit` | 157.6 s → 47.3 s |
| `appearance` | 632 | `sl-viewer-world-avatar` | 157.6 s → 117.5 s |
| `world_map_math` | 631 | `sl-viewer-map` | 157.6 s → 79.9 s |
| `ik` | 415 | `sl-viewer-world-avatar` | 157.6 s → 117.5 s |
| `sit_offset` | 305 | `sl-viewer-ui-context-menus` | 157.6 s → 23.2 s |
| `procedural` | 230 | `sl-viewer-world-avatar` | 157.6 s → 117.5 s |

**4,230 of 11,658 lines — 36%.** (The audit said 39% of 10,985; the crate has
grown since, and two modules it placed in the binary had already moved on to
real crates, which is a better home than the one it named — see below.)

That column is `scripts/build-critical-path.py`'s *chain above each rebuilt
unit* — what still has to compile once that crate starts, which is what an edit
to it costs — read off one baseline run so all sixteen numbers share a clock.
Three of the eight were then put on a stopwatch; see below.

## The rule this applied, written back into the crate doc

Leaf position is what makes a module *eligible* for `sl-viewer-kit`; it is not
what makes it belong. The test is **more than one consumer crate**, and the
crate doc now carries it along with the table above, so the next module to
acquire or lose a second caller has somewhere to read what to do.

## Two of the audit's homes had already improved

The audit put `appearance` (258 lines then) and `sit_offset` in "the binary".
Both now land in ordinary library crates instead — `sl-viewer-world-avatar` and
`sl-viewer-ui-context-menus` — because the binary extraction
([[viewer-audit-binary-module-extraction]]) moved their callers out first. That
matters for the reason [[build-split-ui-widgets-crate]] measured: the app crate
is the **terminal** unit of the build, so a line moved there lands on the
critical path at full price. Only `shadow_visibility` goes to the binary now,
and it goes there because the binary is genuinely its only caller.

## What it cost in visibility: one item

`sl-viewer-chat::emoji_complete`'s precedent again — the seven library
destinations declare the module `pub mod`, exactly as `sl-viewer-kit` did, so
not one item needed rewidening. The binary declares `mod shadow_visibility;`,
which made its single `pub struct ShadowVisibilityPlugin` an `unreachable_pub`;
it is `pub(crate)` now. That is the whole visibility diff.

Six of the eight files are **byte-identical** to their originals
(`diff <(git show HEAD:sl-viewer-kit/src/<m>.rs) <dest>/src/<m>.rs` is empty).
The two that are not carry only the edits described here.

## The sort loop the audit asked for

`radar_model` hand-wrote the multi-column sort that
[[viewer-audit-table-sort-consolidation]] replaced everywhere else, because
`sl-viewer-kit` cannot reach `ui_table::order_by_sort_keys` in
`sl-viewer-ui-widgets`. In `sl-viewer-people` it can, so `sort_rows` is now that
call with a local comparator and tie-break — the twelfth copy of the loop, and
the last one the consolidation could not reach.

## Two dependencies left the kit's manifest

Each was there for exactly one of the departed modules, and 20+ crates were
paying to resolve them:

- **`jiff`** — `radar_model`'s "born on" → age-in-days parse.
- **`sl-viewer-notifications`** — `appearance`'s "attempting to fix this" tip.
  It moves to `sl-viewer-world-avatar`, which did not have it; the crate is a
  `bevy_ecs`-only leaf, so no cycle and nothing new on the build path.

## What the build measurement actually says

Run the way the book's
[build-performance](../../book/src/tools/build-performance.md) page
prescribes: touch `sl-viewer-kit` before each run so the same set rebuilds,
`--timings` either side, compared with `--baseline` so compile noise is held
out.

**From-scratch: the critical path barely moves.** Over the segment both runs
share (`sl-viewer-kit` → the final link): **157.6 s → 154.8 s, −2.9 s**.

That is the honest and the *expected* reading, and it is the same result
[[build-split-ui-widgets-crate]] got: the kit was never **on** the critical path
— it ran beside `sl-viewer-platform → -settings → -ui-core` with about two
seconds of slack — so shrinking it (14.7 s → 12.3 s of self time) cannot shorten
a chain it was not in. **This task moved a unit's size, not the graph.**

**Incremental: that is where the 36% went.** Unique comment appended to one
file, `cargo build --release -p sl-client-bevy-viewer`, all four in the same
tree with a settling build between them — unique because a repeated identical
edit hits the `kache` wrapper and reports a fraction of the true time:

| edit to | crates rebuilt | wall clock | |
| --- | ---: | ---: | ---: |
| `minimap_math` — a module that **stayed** in the kit | 24 | **162 s** | *(what all eight used to cost)* |
| `world_map_math`, now in `sl-viewer-map` | 6 | **65 s** | **−60%** |
| `radar_model`, now in `sl-viewer-people` | 3 | **52 s** | **−68%** |
| `shadow_visibility`, now in `sl-client-bevy-viewer` | 1 | **30 s** | **−81%** |

The first row is the control and needs no "before" tree: every one of the moved
modules *was* a kit edit, so it is what each of the other three used to cost.
These are 2–3x effects, far outside the report's ±20% per-unit noise band, so
they need none of the `--baseline` treatment the from-scratch figure above does.

`radar_model` now rebuilds `people`, `ui-context-menus` and the app instead of
`world-api`, `world-objects`, `world-avatar`, `world-scene`, `world-view` and
the rest of the fan in order to reach the one crate that reads it.
`shadow_visibility` rebuilds the app and literally nothing else.

**A measurement trap worth recording:** reverting each probe with
`git checkout` leaves the file's mtime new, so the *next* probe starts with the
previous probe's crate already dirty and measures both. The first pass here
reported `radar_model` at 24 crates for exactly that reason. Settle with a
build after each revert, or revert before the next probe rather than after the
last one.

## Tests for the four zero-test files the audit named

The audit closed by naming four files in the crate with no tests at all. They
are still here — they have more than one consumer, so the split does not touch
them — and they now have 24 tests between them, written against what their own
docs promise:

- **`probe_layers`** (9 tests) — the module exists to keep reflection-probe
  capture cameras off the sun's shadow path, and every one of its guarantees is
  a statement about which layer sets intersect which. So: no probe camera meets
  the shadow-casting sun (the pipeline stall the scheme was written to remove,
  and invisible in a picture); the mirror sun lights every probe and never the
  main view; the mouselook head casts a shadow without being drawn;
  water-exclusion surfaces leak into no ordinary view; no two roles share a
  layer number.
- **`face_material`** (6 tests) — `has_water_fog` is what stops the per-frame
  water sweep rewriting every material and re-creating its bind group
  (`sl-client-facematerial-no-per-frame-mutation`), so it is tested per
  component and for the bit-exactness its doc claims (`-0.0` vs `0.0`). Plus the
  two write paths that share `uv_translations_a/b`: each must leave the other's
  half alone.
- **`avatar_assets`** (6 tests) — the base-part table and `BodyRegion`: distinct
  baked slots, every region covered by a part, the eyeballs the only rigid parts
  and pinned to different joints, the eyelashes hidden with the head.
- **`sky_presets`** (4 tests) — the menu's pin position cross-checked against
  the keyframe `sl_proto` schedules the same preset at. Those are two encodings
  of one fact living in two crates; drift would freeze **Sunset** at some other
  time of day while still calling itself Sunset.

Each was mutation-checked: the invariant was broken in the source, the expected
test failed, the break was reverted.

## Follow-ups

- `sl-viewer-kit`'s doc still opens on "a deliberately mixed bag". That is now
  true of a smaller, better-defended set — but the crate is still twelve modules
  with nothing in common but graph position, and the next honest question is
  whether `minimap_math` (1904 lines, three consumers) wants company or a crate.
- `sl-viewer-gallery` and `sl-viewer-kit` remain the two shared hubs the
  parallel plan marks read-mostly.
