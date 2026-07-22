---
id: viewer-perf-chat-transcript-virtual-list
title: Chat transcript via virtual list — stop re-shaping the whole history
topic: viewer
status: ideas
origin: performance survey of the implemented viewer (2026-07-22)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

A conversation's transcript is rendered as **one single `Text` node**:
`refresh_conversations` (`conversations.rs:1707-1717`) rebuilds the
whole string with `format_transcript` (`conversations.rs:656`) over all
lines — up to `HISTORY_CAP = 200` live lines (`conversations.rs:84`)
plus recalled history — and `set_text`s the one node whenever a new line
lands (`rendered_revision != entry.revision`).

Because it is a single multi-line `Text`, appending one line makes
`bevy_text`/parley re-shape **every line** of the transcript: ~200 lines
× ~50 chars ≈ 10 000 glyphs re-shaped, plus a ~10 KB string
reallocation, per incoming message. Shaping is the expensive stage —
ms-scale at that glyph count. It is correctly revision-gated (not a
60 Hz storm), but busy local chat or an active group easily produces
several-to-dozens of messages per second, and cost grows with history
length.

## Proposed fix

Render the transcript through the existing `virtual_list.rs` — a
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

Confidence: high that the whole transcript currently re-shapes (single
Text node verified); medium on the exact ms until profiled.
