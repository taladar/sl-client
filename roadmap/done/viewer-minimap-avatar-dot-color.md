---
id: viewer-minimap-avatar-dot-color
title: Minimap other-avatar dots are red, not green like the reference
topic: viewer
status: done
origin: user report (2026-08-07)
refs: [viewer-minimap-avatar-dots, viewer-minimap, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

Other avatars render on the minimap as **red** dots. In the reference viewer
they are **green** (`MapAvatarColor`); red is reserved for the tracking beacon
(`MapTrackColor`), so today the two collide — a tracked location and a nearby
avatar are indistinguishable.

## Read the Vintage skin, not the default one (2026-09-11)

This viewer follows the reference's **Vintage** skin throughout (see the module
docs in `sl-viewer-people`, `sl-viewer-inventory`, `sl-viewer-audio`). Vintage's
`colors.xml` overrides exactly two of the map family, and everything else falls
through to the default skin it layers over:

| colour | where | resolves to |
| --- | --- | --- |
| `MapAvatarColor` | vintage, `reference="Green"` | `0 1 0 1` |
| `MapAvatarFriendColor` | vintage, `reference="Yellow"` | `1 1 0 1` |
| `MapTrackColor` | default, `reference="Red"` | `0.729 0 0.121 1` |
| `MapAvatarMutedColor` | default, literal | `0.4 0.4 0.4 1` |
| `MapAvatarSelfColor` | default, `reference="Yellow"` | `1 1 0 1` |
| `MapAvatarLindenColor` | default, `reference="Blue"` | `0 0 1 1` |

Reading the same names from the **default** skin instead is what produced the
bug: there `MapAvatarColor` *is* `Red`, byte-identical to `MapTrackColor`.
Vintage moving friends to yellow is also what answers this file's original
worry that a green base dot would make friends and non-friends identical.

The second trap is that a skin names a colour rather than giving its channels:
the named `Red` is a crimson `186 0 31`, not `255 0 0`, which is what
`COLOR_TRACK` had.

Three further gaps came out of reading the same reference code:

- `LGGContactSets::colorize` greys a **blocked** resident's dot
  (`MapAvatarMutedColor`) between the friend and Linden branches. We had no
  such branch — the code said so, as a follow-up waiting on a mute-list mirror
  the viewer now has (`MuteModel`).
- The reference also distinguishes the beacon from a dot by **shape**:
  `map_track_16.tga` is a hollow ring whose hole is half its outer radius
  (`LLWorldMapView::drawTrackingDot` → `drawDot(…, sTrackCircleImage)`), where
  an avatar is a solid disc. `draw_tracking` drew a disc.
- Our unknown-altitude glyph was a ring, which would then have been the
  beacon's shape. The reference draws `map_avatar_unknown.tga`: a full-height
  stem crossed by two full-width bars.

## Fix

Colours, in `minimap_math`:

- `COLOR_AVATAR` → `0 255 0`, `COLOR_AVATAR_FRIEND` → `255 255 0` (Vintage).
- `COLOR_TRACK` → `186 0 31`, the named `Red` rather than a pure red.
- New `COLOR_AVATAR_MUTED` (`102 102 102`), wired into `avatar_color` in the
  reference's branch order: mark, friend, blocked, Linden, base.

Shapes, so the marks stay apart whatever they are painted:

- `draw_tracking` draws the reference's ring (outer 8 px, hole 4 px) instead of
  a disc, on the minimap and the world map alike.
- `HeightGlyph::Unknown` draws the reference's crossed bars instead of a ring.

Skinnable, rather than six more constants nobody can retune:

- Six new tokens on the existing colour bridge
  ([[viewer-preferences-colors-skins-tab]]):
  `--minimap-avatar`, `--minimap-avatar-friend`, `--minimap-avatar-muted`,
  `--minimap-avatar-self`, `--minimap-avatar-linden`, `--minimap-track`, with
  the values above as the built-in fallbacks. Defined by both shipped skins,
  and exposed as a *Minimap colors* section in the preferences tab, so a user
  override sits above the skin exactly as the chat and name-tag colours do.
- The beacon token is grouped with the dots deliberately: it is only legible
  while it wears a colour none of them does.
- The resolved palette rides in `CompositeStamp`, so a skin switch, hot reload
  or override repaints the map instead of waiting for the camera to move. The
  friend- and mute-list revisions joined it for the same reason — both classify
  dots, and neither moved anything else in the stamp.

Tests: the ring's hollow centre against the dot's solid one; the crossed-bar
glyph against a ring; every constant against the Vintage value; no dot wearing
the beacon's colour, in the constants *and* in every shipped skin's real CSS;
the classification order including the new blocked branch; and the bridge's
fallbacks against the `minimap_math` constants.

Reference (read-only): `indra/newview/skins/vintage/colors.xml` and
`skins/default/colors.xml`, `skins/default/textures/map_track_16.tga` and
`map_avatar_unknown.tga`, `llnetmap.cpp`, `llworldmapview.cpp` (`drawDot`),
`lggcontactsets.cpp` (`colorize`).

## Live check outstanding

Screenshot the minimap with a beacon set (double-click-teleport) and a nearby
resident in frame, to confirm the ring reads as a beacon at the real dot sizes.
