---
id: viewer-perf-chat-transcript-virtual-list
title: Chat transcript via virtual list — stop re-shaping the whole history
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

**Updated at the 2026-10-15 audit** (the original text described a single
`Text` node; the transcript has since become clickable rows). A
conversation's transcript is now a column of **one linkified row per line**
(`sl-viewer-people/src/conversations.rs`, `ConversationView::transcript_column`)
— so speaker names and URLs / SLURLs are links — in the reference's three
bands (persisted recall, server-fetched history, live lines up to
`HISTORY_CAP`). But `refresh_conversations` still **rebuilds the whole
column** whenever a line lands (`rendered_revision != entry.revision`):
`despawn_related::<Children>()` on the column, then a fresh row spawned and
shaped for every recall, history and live line.

So appending one line still costs O(history): every row entity is despawned
and respawned and every line is re-shaped by `bevy_text` / parley (~200+
lines × ~50 chars ≈ 10 000 glyphs), plus the entity churn the old single
`Text` did not have. It is correctly revision-gated (not a 60 Hz storm),
but busy local chat or an active group easily produces several-to-dozens of
messages per second, and cost grows with history length.

## Proposed fix

Render the transcript through the existing virtual list
(`sl-viewer-ui-core/src/virtual_list.rs`) — a
correct windowed-recycling list, already O(visible) with bounded row
pools and guarded writes, whose own module comment names "chat history
at scale" as an intended client. One `Text` entity per line: a line is
shaped once when its row binds, appending shapes only the new line, and
scrolled-out history keeps its cached layout. Cost per new message drops
from O(history) to O(1) (+O(visible) on scroll).

Details to carry over: line wrapping (a virtual row's height depends on
wrap — the list already handles variable row heights via its measured
rows), selection/copy across lines, the transcript's URL/emoji styling
spans, and auto-scroll-to-bottom behaviour. The `recall` history then
also stops being a growth term entirely (off-window rows don't exist).

## Estimated impact

Medium on average, high in exactly the situations that hurt — crowded
regions with fast local chat while the conversations floater is open.
Removes an ms-scale main-thread stall per message; also caps transcript
memory-in-layout at the visible window. Measure text-shaping span time
per incoming message ([[viewer-profiling]]) with a scripted chat-flood
on the test grid.

Confidence: high that the whole transcript currently re-shapes (full
column rebuild verified 2026-10-15); medium on the exact ms until profiled.
