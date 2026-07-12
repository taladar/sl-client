---
id: idiomatic-p6-08
title: Considered, not adopted:
topic: idiomatic
status: deferred
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Considered, not adopted: `chat::ChatVolume` (richer `ChatType` kept — see
    interop above), `search::SearchCategory` (Search-floater tab / search-URI
    concept with no LLUDP wire field; the raw `category` directory fields are
    the distinct classified-ad code set, now the local `ClassifiedCategory` —
    see the `SearchCategory` item above), `pathfinding::PathfindingType`,
    `viewer_uri::ViewerUri`,
    `radar::Area`, `map::Location` (integer-coord + mandatory-name shape
    matches no wire field — teleport positions are float region-local coords,
    map blocks carry grid coords), `map::ZoomLevel` (no map-zoom field in the
    LLUDP protocol) (no matching protocol field). `map::Distance`
    (`draw_distance`/`far`) was deferred, not rejected — it needed an
    `sl-types` constructor; that constructor was added and `Distance` adopted
    in the batched migration below.
