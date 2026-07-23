---
id: viewer-minimap-avatar-dots
title: Minimap avatar dots — colours, height cues, hover, chat rings
topic: viewer
status: done
origin: user request (2026-07-22); split from viewer-minimap
blocked_by: [viewer-minimap]
refs: [viewer-avatar-radar, viewer-name-tags-display-names]
---

Context: [context/viewer.md](../context/viewer.md).

Avatar presence on the minimap: everyone as a coloured dot with a height
cue, yourself as a distinct marker, hover tooltips, and the chat-range
rings. Reference facts (Firestorm `llnetmap.cpp:697-867`,
`llworldmapview.cpp drawAvatar`, `llworld.cpp getAvatars`, researched
2026-07-22):

## Position source (two merged sets)

`getAvatars` merges: (1) full avatar objects first — precise positions,
needed so distances stay correct above 1020 m; then (2) each region's
coarse-location list, deduplicated by UUID. Ours is the coarse tracking
in `avatars.rs` (incl. neighbour regions via the `viewer-r24` fix)
merged with the precisely-known scene avatars — the same source
[[viewer-avatar-radar]] consumes; build one shared provider.

Coarse Z is a byte in 4 m steps (0–1020 m). A raw byte of **0 or 255**
means "altitude unknown" and is flagged with a sentinel: if the camera
is itself ≥ 1020 m the avatar draws the *unknown* glyph, otherwise it is
treated as "far above you".

## Dots & height cue

- Glyph by relative height: within ±7 m a level dot
  (`map_avatar_32`), above +7 m an up-chevron (`map_avatar_above_32`),
  below −7 m a down-chevron (`map_avatar_below_32`), plus the unknown
  glyph. This is the camera-relative depth cue (objects use
  water-relative colours instead — [[viewer-minimap-object-layer]]).
- Dot radius `max(0.75 × pixels_per_meter, 3.5 px)`.
- Colours: base `MapAvatarColor` (red); friends `MapAvatarFriendColor`
  (green); self `MapAvatarSelfColor` (yellow); Lindens
  `MapAvatarLindenColor` (blue); muted `MapAvatarMutedColor` (grey
  0.4). Firestorm layers contact-set colours and per-avatar "marks"
  (context-menu mark colours: red/green/blue/purple/light-yellow) on
  top; under RLV name-hiding everything falls back to the neutral base
  colour. Avatars selected in the people panel get a highlight ring.
- **Self**: a distinct you-are-here glyph (`map_avatar_you_32`) tinted
  yellow at the (pan-adjusted) centre; heading is conveyed by the
  camera frustum wedge from the base task, not a separate arrow.

## Hover

The closest dot within the pick radius (`dot_radius ×
FSMinimapPickScale`, default 3.0; a faint pick-radius circle in
`MapPickRadiusColor` follows the cursor) resolves to an avatar tooltip:
display name + distance in metres (avatars with unknown altitude show
a radar-derived or "> draw distance" range). With no avatar under the
cursor the tooltip shows region name and — only when property lines are
enabled — parcel name, owner, for-sale price and area, plus the
double-click hint matching the configured double-click action. Name
resolution comes from [[viewer-name-tags-display-names]] data.

## Chat rings (Firestorm)

Optional whisper/chat/shout range rings around the self marker
(master `MiniMapChatRing`, default off; per-ring
`FSMiniMapWhisperRing`/`FSMiniMapChatRing`/`FSMiniMapShoutRing` default
on), 2 px circles in `MapWhisperRingColor` (blue 0.3α),
`MapChatRingColor` (yellow 0.3α), `MapShoutRingColor` (red 0.3α) —
ranges from sim features where the grid overrides them (our
`SimulatorFeatures` handling has the values).

Reference (Firestorm, read-only): `llnetmap.cpp`, `llworldmapview.cpp`
(`drawAvatar`, the glyph images), `llworld.cpp` (`getAvatars`),
contact-sets (`lggcontactsets`).

Deps: [[viewer-minimap]] (surface/transforms). The context-menu actions
on a hovered avatar live in [[viewer-minimap-interactions]].

## Done (2026-07-23)

Shared provider `AvatarState::map_avatars()` (full-object avatars first,
coarse-only dots deduplicated after, each coarse entry carrying its raw
coarse altitude — `avatars.rs` now always records `coarse_pos`), the
same source a future [[viewer-avatar-radar]] consumes. Dots draw into
the composited surface: level disc / up chevron / down chevron by the
±7 m camera-relative band, a hollow ring for the unknown-altitude
sentinel (coarse z 0 or ≥1020 with the camera itself high; below that a
sentinel reads as "far above", per the reference). Radius
`max(0.75 × ppm, 3.5 px)`. Colours: base red, friends green (live
`FriendsModel`), self yellow (distinct outlined marker at the avatar
position), Lindens blue (legacy-name " Linden" suffix), context-menu
marks override. Hover: closest dot within `dot radius ×
MinimapPickScale` (3.0), faint pick-radius circle at the cursor,
tooltip = name + distance in metres ("> far clip" for unknown
altitude); with no dot, region name + (property lines on) parcel
name/owner/price/area + the double-click hint. Chat rings around the
self marker (`MiniMapChatRing` off; per-ring whisper/say/shout toggles
on) at the `SimulatorFeatures` extras ranges (defaults 10/20/100) via
the new `ChatRanges` resource.

Not carried over (each needs a feature we don't have yet): contact-set
colours (no contact sets), muted grey (no mute-list mirror), the
people-panel selection highlight ring (no selection channel), display
names in the tooltip (legacy names until
[[viewer-name-tags-display-names]]).
