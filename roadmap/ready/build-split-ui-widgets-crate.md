---
id: build-split-ui-widgets-crate
title: Lift the single-consumer widgets out of sl-viewer-ui-widgets
topic: viewer
status: ready
origin: critical-path analysis of the world avatar/object split (2026-08-26)
points: 3
refs: [build-split-viewer-crate, build-flatten-feature-tier]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets` is 20.6k lines over thirteen modules, compiles in
**21.6 s**, and carries **140.4 s of dependent crates above it** — the second
most expensive place in the workspace to touch a line, behind only
`sl-viewer-ui-core`. It is also on the build's critical path (see
[[build-flatten-feature-tier]] for the measurement and its caveats).

It does not need to be one crate. Counting which crates name each module:

| module | lines | consumer crates |
| --- | ---: | ---: |
| `floater` | 1931 | 13 |
| `ui_text_input` | 1558 | 12 |
| `ui_tab` | 2571 | 9 |
| `ui_table` | 1721 | 6 |
| `ui_combo` | 750 | 6 |
| `menu` | 3934 | 6 |
| `floater_persist` | 627 | 7 |
| `ui_search` | 580 | 6 |
| `ui_color_picker` | 656 | 5 |
| `settings_binding` | 1594 | 5 |
| `ui_radio` | 678 | 4 |
| `emoji_complete` | 554 | **2** |
| `pie_menu` | 3445 | **1** |

## `pie_menu` is a free extraction

**3445 lines — the second-largest module in the crate — named by exactly one
crate (the app crate), and importing *nothing* from the other twelve.** Zero
`crate::<sibling>` references out of it. It is 17% of the crate that every
one of the thirteen consumers recompiles, for a widget only the composition
root ever spawns.

Move it to the app crate, or to its own crate above the world tier if the app
crate is already the wrong place for 3.4k lines. Either way the edit cost
inverts: touching a pie menu today rebuilds `ui-widgets` plus 140 s of
dependents; afterwards it rebuilds the app crate alone. Every pie menu ships a
committed address test (see the `sl-client-pie-menu-address-tests` note), so
this is a file set that gets edited.

Expected effect on the tier: `ui-widgets` 20.6k → 17.2k lines, ~21.6 s →
~18 s, and one fewer thing on the critical path for the other twelve
consumers.

`emoji_complete` (554 lines, `chat` + the app crate) is the same shape at a
tenth the size — worth taking in the same commit if it comes out as cleanly,
not worth its own.

## What not to do

**Do not split the crate down the middle.** `floater`, `ui_text_input`,
`ui_tab`, `ui_table`, `menu` and the rest are named by five to thirteen crates
each: any line drawn between them is a line most consumers end up on both
sides of, and the per-crate metadata and re-monomorphization tax would buy
nothing. The measured rule from [[build-split-viewer-crate]] still holds —
stop splitting when the unit-sum stops falling. Here that means taking the
one module that genuinely has a single consumer and leaving the shared
vocabulary alone.

Re-measure `ui-widgets`' unit time after the move rather than assuming the
line-count proportion holds; the original split's own figures showed
lines-recompiled models over-predicting every time.
