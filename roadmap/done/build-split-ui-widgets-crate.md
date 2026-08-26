---
id: build-split-ui-widgets-crate
title: Lift the single-consumer widgets out of sl-viewer-ui-widgets
topic: viewer
status: done
origin: critical-path analysis of the world avatar/object split (2026-08-26)
points: 3
refs: [build-split-viewer-crate, build-flatten-feature-tier,
  build-split-world-avatar-crate]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets` was 20.6k lines over thirteen modules and carried
**123.2 s of dependent crates above it** — the second most expensive place in
the workspace to touch a line. Eleven of its thirteen modules are named by five
to thirteen crates each and are a genuine shared vocabulary. Two were not, and
those two are gone:

- **`pie_menu` → `sl-viewer-ui-pie-menu`** (new crate). 3445 lines, named by
  exactly one crate, importing nothing from its twelve siblings.
- **`emoji_complete` → `sl_viewer_chat::emoji_complete`.** 554 lines, and the
  chat input is the only field that ever attaches one.

The rest of the crate was left alone, exactly as the note said to: any line
drawn through `floater` / `ui_text_input` / `ui_tab` / `ui_table` / `menu` is a
line most consumers end up on both sides of.

## Why `pie_menu` got a crate rather than a home in the app

The note offered both. The app crate is the wrong place, and the reason is the
same measurement the task was written from: **the app crate is the terminal unit
of the build**, so every line moved into it lands on the critical path at full
price. 3.4k lines there would have given back at the tail roughly what the move
saved at the head.

A separate crate does not, because `pie_menu` names nothing but `bevy` and
`sl-viewer-ui-core`. It therefore sits **beside** `ui-widgets` over `ui-core`
rather than above it, and starts the moment `ui-core` finishes — the same tier
as `world-objects`. It never joins the chain (see the table below: 124.0 s above
it, identical to `ui-widgets` and `world-objects`, which is what "starts at the
same moment" looks like in this report).

The manifest is the evidence that the cut was clean: the new crate needs
`bevy` + `sl-viewer-ui-core`, and `pretty_assertions` + `sl-viewer-testkit` for
its tests. Nothing else. `ui-widgets` in exchange **lost its `sl-emoji`
dependency** — `emoji_complete` was its only user — which is the manifest sweep
[[build-split-world-avatar-crate]] left as a follow-up, paid on one crate.

## What it cost

Nothing in visibility, unusually. Neither module had a single `crate::<sibling>`
reference, so no item had to be widened the way every earlier step of
[[build-split-viewer-crate]] had to widen them. The whole price was **four
intra-doc links**, and they are worth naming because they are a trap this
codebase has hit before (see the `sl-client-visibility-pass-breaks-doc-links`
note): `menu.rs` linked `[`crate::pie_menu`]`, `[`crate::pie_menu::PieAction`]`
and `[`crate::pie_menu::OpenPieMenu`]`, and `floater.rs` named `crate::pie_menu`
in prose. A *sibling* crate cannot be linked to at all without a dependency
edge that would defeat the point, so all four became plain code spans.
`RUSTDOCFLAGS="-D warnings" cargo doc` over the three crates is clean.

The app crate's own `crate::pie_menu::…` paths — four menu modules and their
committed address tests — did not change at all, because the alias line moved
rather than the paths: `pub(crate) use sl_viewer_ui_pie_menu::pie_menu;`.

## Measured outcome

| crate | before | after |
| --- | --- | --- |
| `sl-viewer-ui-widgets` | **20.6k** / 13 modules | **16.7k** / 11 modules |
| `sl-viewer-ui-pie-menu` | — | **3.5k** |
| `sl-viewer-chat` | 3.4k | 4.0k |

### Incremental rebuilds — the result that matters

Append a **unique** comment to one file, `cargo build --release -p
sl-client-bevy-viewer`, before and after, in the same tree. Unique because a
repeated identical edit hits the `kache` wrapper and reports a fraction of the
true time — trap 1 of [[build-split-viewer-crate]]'s four.

| file edited | crate after the move | before | after | |
| --- | --- | ---: | ---: | ---: |
| `pie_menu.rs` | `ui-pie-menu` | 130 s | **40 s** | **−69%** |
| `emoji_complete.rs` | `chat` | 118 s | **62 s** | **−48%** |

A pie-menu edit used to rebuild `ui-widgets` and the twelve crates stacked on
it. It now rebuilds a 3.5k-line crate and the app. That is the whole point of
the task and it is far outside any noise band.

The two figures differ for a structural reason worth keeping: `pie_menu`'s new
crate is a **leaf** below the app alone, while `emoji_complete` landed in
`chat`, which still has `people` and the app above it. Moving a module to its
consumer is worth less than moving it beside its consumer.

### The critical path did not move, and that is the honest reading

`--timings` again, read through `scripts/build-critical-path.py`:

- **before:** `ui-core:9.3 → ui-widgets:21.2 → world-scene:30.1 → map:14.9 →
  people:21.7 → app:28.1 → link:7.3` = **132.5 s**
- **after:** `ui-core:10.6 → ui-widgets:18.2 → world-scene:33.3 → map:15.6 →
  people:21.8 → app:28.2 → link:6.9` = **134.6 s**

The chain has the **same shape**: `ui-widgets` still gates `world-scene`, and
taking 3.4k lines out of it did not change which crate that is. Run the
comparison the way the tool is built for — `--baseline`, which re-solves the new
graph with the old durations so compile noise is held out — and the answer is
**+0.0 s**. That is not a disappointing result, it is the correct one: no *edge*
on the chain changed, so a tool that measures edge changes reports nothing. This
task moved a unit's **size**, not the graph.

The size did move, in the predicted direction: `ui-widgets` **21.2 s → 18.2 s**,
against the note's estimate of "~21.6 s → ~18 s". But the report's own noise
band is ±20% on a single unit, and −14% sits inside it — every untouched crate
in the same pair of runs drifted too (`ui-core` 9.3 → 10.6, `world-scene`
30.1 → 33.3).
**So: consistent with the prediction, not proof of it.** One sample cannot
separate a 3 s structural gain from a 3 s drift, and it would take repeated runs
to try. The incremental numbers above did not need that treatment because they
are 2–3× effects.

New crate placement, from the "chain above each rebuilt unit" ranking:

```text
124.0  self  18.2  sl-viewer-ui-widgets
124.0  self  16.1  sl-viewer-world-objects
124.0  self   6.0  sl-viewer-ui-pie-menu   <- same start, off the chain
```

## Where this leaves the tier

The structural lever is now genuinely spent for `ui-widgets`. What is left in it
is eleven modules that five to thirteen crates each name, and the measured rule
from [[build-split-viewer-crate]] — stop when the unit-sum stops falling —
already said not to cut them. `ui-widgets` remains on the critical path at
18.2 s and the honest way to shorten it further is to make those eleven modules
cheaper to compile, not to redistribute them.

## Follow-ups

- `emoji_complete` is a general widget living in a feature crate, which is a
  deliberate compromise recorded in `sl-viewer-chat`'s crate docs: a second
  consumer is the signal to move it back down into `ui-widgets`.
- [book/src/tools/build-performance.md](../../book/src/tools/build-performance.md)
  still opens with "`sl-client-bevy-viewer` alone is ~283k lines across 239
  files", now three splits out of date. Carried over from
  [[build-split-world-avatar-crate]] and still not done.
