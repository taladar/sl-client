---
id: build-flatten-feature-tier
title: Flatten the feature tier — it is a chain where it should be a fan
topic: viewer
status: done
origin: critical-path analysis of the world avatar/object split (2026-08-26)
points: 5
refs: [build-split-viewer-crate, build-split-world-avatar-crate]
---

Context: [context/viewer.md](../context/viewer.md).

The feature crates were drawn as siblings over the world tier. They were not:
five of them formed a **chain**, and that chain was most of what was left of
the build's serial tail.

```text
world-view → inventory → people → preferences → app crate → link
   20.9s        10.2s     24.2s      8.9s         28.0s      10.0s
```

They are siblings now. After `world-view` finishes, `people`, `edit`,
`preferences`, `map`, `places` and `search` all start together, and the tail is
set by the slowest one alone.

## How this was measured

`cargo build --release -p sl-client-bevy-viewer --timings`, then the
infinite-core longest path through cargo's own unit-unblock graph
(`unblocked_units` / `unblocked_rmeta_units` in the report's `UNIT_DATA`) —
`scripts/build-critical-path.py`, which was written for this task and does both
the single-report solve and the noise-controlled before/after below. The
"before" run: **a 279.1 s critical path against a 279.7 s wall clock** on 24
cores. The build was entirely chain-bound — at no point would more cores have
helped.

**Caveat on the figures:** that run had a warm dependency tree, so most
third-party units report 0.0 s and the *dependency* half of the chain is
understated (`bevy_pbr`'s 85.3 s is real; the rest is not measured here). The
viewer half — 176.5 s over twelve crates — is fully real, and it is what this
task was about.

Ranked by the chain **above** each crate (what an edit to it costs, modulo
parallelism), before:

| crate | self | chain above it |
| --- | ---: | ---: |
| `sl-viewer-ui-core` | 12.4 | 152.9 |
| `sl-viewer-ui-widgets` | 21.6 | 140.4 |
| `sl-viewer-world-api` | 4.4 | 137.4 |
| `sl-viewer-world-objects` | 14.2 | 133.1 |
| `sl-viewer-world-scene` | 26.5 | 118.8 |
| `sl-viewer-world-avatar` | 21.4 | 113.7 |
| `sl-viewer-world-view` | 20.9 | 92.3 |
| `sl-viewer-people` | 24.2 | 61.1 |
| `sl-client-bevy-viewer` | 28.0 | 38.0 |

The world tier was **not** the problem any more —
[[build-split-world-avatar-crate]] took `world-objects` to 14.2 s and put
`world-avatar` off the path entirely, parallel with `world-scene`. Splitting
the world further would have moved nothing.

## The result

Measured the same way, on the same durations. The second `--timings` run
rebuilt from `sl-viewer-platform` up, so the comparable segment is *platform →
link*; to keep run-to-run compile noise out of the comparison the new graph was
re-solved with the **first run's** per-unit durations:

| | platform → link |
| --- | ---: |
| before | 166.7 s |
| after | **147.6 s** |

**19.1 s off the critical path**, 6.9 % of the whole warm-dependency build, and
the same saving on every incremental rebuild that touches anything below the
feature tier. The tail after `world-view` went from 102.3 s to 83.1 s, which
was the number this task set out to reach.

The new critical path, and the shape it settles into:

```text
platform → settings → ui-core → ui-widgets → world-scene → world-view
   3.0        0.9       12.4       21.6         26.5         20.9
                                            → people → app crate → link
                                               24.2      28.0      10.0
```

`people` is now the *only* feature crate on the path, and it is there because
it is the slowest, not because anything waits on it.

## What the plan got wrong, and what actually worked

The plan named two jobs: move the shared setting-key constants down (step 1),
then lift `InventoryModel` and the minimap query helpers out of the feature
tier so `people` and `edit` stop depending on `inventory` (step 2). Step 1 was
right and was done as written. **Step 2 was the wrong lever**, and doing it
would have been both much more work and less effective.

### Step 1 — the constant edges (as planned)

`preferences` named **41 setting-key `&'static str`s** belonging to `people`
and `map`, and **nothing else at all** from either crate. Two whole crate
dependencies — one of them the 24.2 s `people` — bought for forty-one string
constants that the preferences panels need in order to bind a checkbox to a
setting.

The fix is the one [[build-split-viewer-crate]] used for the `PipelineStats`
labels: **a constant that two layers agree on belongs in the layer beneath
both**, here `sl-viewer-settings`, in a new `keys` module. The owning module
re-exports its keys from there and keeps registering the default, the section
and the description, so every existing `radar::SETTING_AGE_DAYS` call site
still resolves and the two halves cannot drift.

### Step 2 — not the edges the plan named

The plan read the reference counts (`people` → `inventory` 8, → `map` 7, →
`media` 4; `edit` → `inventory` 5) as the thing to remove. Re-solved against
the actual unblock graph, three of those four edges **cost nothing**:

- `map` finishes at 190.0 s and `media` at ~154 s, both *before* `world-view`
  at 197.7 s. `people` depends on `world-view` directly, so removing either
  edge would not have moved its start by a millisecond.
- `edit` was gated not by its own `inventory` edge but by `pickers`, which has
  one too — removing `edit` → `inventory` alone would have changed nothing.

What every one of those edges actually had in common was **`inventory`
finishing late**, and `inventory` finished late for exactly one reason:

```text
sl-viewer-inventory → sl-viewer-world-view   (one import, in one module)
```

`inventory_drag` asked `world-view`'s `gpu_pick` what the cursor was over so a
row dragged out of the panel could land on an avatar, an object or the ground.
That single import held the whole inventory crate — and therefore `pickers`,
`edit`, `places`, `asset-editors` and `people` — behind the world view.

**Inverting it is what flattened the tier.** The request and the answer now
live in `sl-viewer-world-api`, beneath both: `DragPickActive` (a drag is in
flight) and `DragWorldPick` / `DragPickHit` (what the pick found), with a new
`WorldPhase::DragPickResolved` for the ordering. The two driver systems moved
into `gpu_pick`, where the picker already is; the panel sets the flag and reads
the answer. This is the same seam `DragHoverHighlight` already used for the
other half of the same gesture, so it cost one resource pair and no redesign.

`sl-viewer-inventory` now starts at 150.2 s instead of 197.7 s, and `people`,
`edit`, `pickers`, `places` and `asset-editors` all come off the chain with it.

A second one-liner of the same shape: `sl-viewer-chat` depended on `world-view`
for `input_context::world_has_keyboard`, which `input_context` **re-exports
from `world_api`**. Pointing the import at the definition dropped the edge
outright and took `chat` off `people`'s critical path.

**The lesson worth keeping:** a reference count says how *entangled* two crates
are; it says nothing about what an edge costs. Rank edges by the finish time of
the crate they point at, not by how many names cross them. The 19 references
the plan led with were free, and the one import it never mentioned was the
whole cost.

**Live-verified** on the local OpenSim grid (2026-08-26), since the inversion
is a real runtime change and the unit tests only cover the pure drop
arithmetic: drag-rez onto terrain, drop into an object's contents with the
green hover outline appearing and clearing on release, the Ctrl gate on an
object item, and the self-drop wear all behave as before. The self-drop turned
up a *separate* pre-existing bug — a plain prim attachment reads `(worn)` but
never renders, identically from the context menu's Attach — filed as
[[viewer-prim-attachment-worn-but-not-rendered]].

`InventoryModel` did **not** need to move, and the plan's instinct to move it
was expensive: half of its 788-line `impl` is `DisplayRow` construction, which
is panel work, so splitting the type would have meant either splitting the impl
across crates (turning a dozen private accessors public, straight against
[[build-structural-encapsulation-audit]]) or moving the panel down with it.

## Where this stops

`people` is the slowest feature crate at 24.2 s and is now the *whole* of the
tier's contribution to the tail, so flattening has bottomed out: a further win
needs `people` to shrink, not the tier to be re-drawn. Do not pay a redesign
for a line-count target — that is the lesson [[build-split-world-avatar-crate]]
recorded, and it still applies. The remaining path above it is
`ui-core → ui-widgets → world-scene → world-view`, which is the world split's
territory, not this one's.
