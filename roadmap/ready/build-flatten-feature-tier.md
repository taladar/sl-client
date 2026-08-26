---
id: build-flatten-feature-tier
title: Flatten the feature tier — it is a chain where it should be a fan
topic: viewer
status: ready
origin: critical-path analysis of the world avatar/object split (2026-08-26)
points: 5
refs: [build-split-viewer-crate, build-split-world-avatar-crate]
---

Context: [context/viewer.md](../context/viewer.md).

The feature crates were drawn as siblings over the world tier. They are not:
five of them form a **chain**, and that chain is most of what is left of the
build's serial tail.

```text
world-view → inventory → people → preferences → app crate → link
   20.9s        10.2s     24.2s      8.9s         28.0s      10.0s
```

## How this was measured

`cargo build --release -p sl-client-bevy-viewer --timings`, then the
infinite-core longest path through cargo's own unit-unblock graph. The result:
**a 279.1 s critical path against a 279.7 s wall clock** on 24 cores. The build
is entirely chain-bound — at no point would more cores have helped.

**Caveat on the figures:** that run had a warm dependency tree, so most
third-party units report 0.0 s and the *dependency* half of the chain is
understated (`bevy_pbr`'s 85.3 s is real; the rest is not measured here). The
viewer half — 176.5 s over twelve crates — is fully real, and it is what this
task is about.

Ranked by the chain **above** each crate (what an edit to it costs, modulo
parallelism):

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

The world tier is **not** the problem any more —
[[build-split-world-avatar-crate]] took `world-objects` to 14.2 s and put
`world-avatar` off the path entirely, parallel with `world-scene`. Splitting
the world further would move nothing. The feature tier is where the remaining
serialization is.

## The edges, and what they actually carry

Counted as `crate::<module>::<item>` references, comments excluded:

| edge | refs | what they are |
| --- | ---: | --- |
| `preferences` → `people` | 28 | **28 `SETTING_*` name constants, no code** |
| `preferences` → `map` | 13 | **13 `SETTING_*` name constants, no code** |
| `preferences` → `audio` | 7 | 5 constants + 2 real calls |
| `people` → `inventory` | 8 | real: `InventoryModel`, drag target, two helpers |
| `people` → `map` | 7 | real: `minimap::{narrow, region_handle_at, menu_agent_labels}` |
| `people` → `media` | 4 | real: `browser_widget`, `MediaSurfaces` |
| `edit` → `inventory` | 5 | real: `InventoryModel`, drop target, editor open |

So the tier splits into two very different jobs.

### Step 1 — the constant edges (cheap, and most of the win per hour)

`preferences` names **41 setting-key `&'static str`s** belonging to `people`
and `map`, and **nothing else at all** from either crate. Two whole crate
dependencies — one of them the 24.2 s `people` — bought for forty-one string
constants that the preferences panels need in order to bind a checkbox to a
setting.

The fix is the one [[build-split-viewer-crate]] used for the `PipelineStats`
labels: **a constant that two layers agree on belongs in the layer beneath
both**, here `sl-viewer-settings` (345 lines, 0.9 s, already below everything).
The owning crate keeps registering the default and the doc string, reading the
key from the shared constant, so the two cannot drift.

That alone takes `preferences` off the tail behind `people` and `map`:
102.2 s → **93.3 s** for the tail after `world-view`.

### Step 2 — the real edges (the rest of it)

`people` → `inventory` / `map` / `media` is nineteen genuine code references,
and `edit` → `inventory` five more. Those are the "lift the shared piece down"
move that `879495e1` and `5a252282` did during the original split — most
likely `InventoryModel` and the minimap query helpers want to be below the
feature tier rather than inside one of its members.

With `people` off the chain too, the tail after `world-view` becomes a fan:
**83.1 s**, set by the slowest single feature crate rather than by four of
them in series. Total 102.2 s → 83.1 s, and the same saving on every
incremental rebuild that touches anything below the feature tier.

## Where this stops

`people` is itself the slowest feature crate at 24.2 s, so flattening bottoms
out there — a further win needs `people` to shrink, not the tier to be
re-drawn. Note that too, and stop rather than paying a redesign for a
line-count target: that is the lesson [[build-split-world-avatar-crate]]
recorded, and it applies here.
