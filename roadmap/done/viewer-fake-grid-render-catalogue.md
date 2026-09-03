---
id: viewer-fake-grid-render-catalogue
title: The full-stack test catalogue — NPCs, attachments, teleport, crossing, parcels, environment
topic: viewer
status: done
origin: test-harness plan (2026-08-30)
points: 8
refs: [viewer-fake-grid-render-harness, test-audit-fake-grid-conformance-grid]
blocked_by: []
---

Context: [context/testing.md](../context/testing.md).

Everything the first four full-stack tests did not cover, each with the
oracle it uses. Done 2026-09-03; the tier now runs **18** checks.

The nine subjects that landed, in
`sl-client-bevy-viewer/src/full_stack_test.rs`:

- `an_npc_arrives_with_its_bakes_and_plays_its_animation` — the blue bake
  on the chest, and half the chest twist's period apart the body's own
  disc changes while a ground patch beside it does not.
- `an_attachment_follows_its_avatar_across_a_scripted_move` — the grid
  moves the *body* and never resends the worn box; the box's rendered
  entity moves the same metre and its checker is on the new disc and gone
  from the old one.
- `a_teleport_keeps_the_subject_where_it_is` — the same framing over two
  catalogue regions ten apart puts the subject's centroid within two
  pixels, with the source circuit's objects asserted purged.
- `a_teleport_leaks_nothing_between_regions` — taken mid-fetch: no object
  from a pre-teleport circuit, no asset work owed, and the destination's
  prim wearing its own blue solid and neither of the checker's colours.
- `a_border_framing_shows_both_regions_ground` — one frame holding the
  terrain either side of the border, each region's ground painted its own
  marker colour. (The other two crossing subjects went with
  [[test-fake-grid-neighbours-crossing]].)
- `a_parcel_split_draws_its_property_line_and_a_join_removes_it` — both
  directions, against a control patch of ground the split does not touch.
- `an_environment_change_to_night_darkens_the_sky` — the sky band's
  luminance halves; the one harness in the tier that does **not** pin the
  day, because a pinned day replaces the region's cycle with a synthesised
  one and the grid's own sky would never reach the picture.
- `a_prims_floating_text_is_drawn_above_it` and
  `a_name_tag_is_drawn_over_an_npcs_head` — the shared world-text
  billboard read from either end, as a setting toggle plus
  `changed_centroid`: what changed is real, is centred above its subject,
  and left the subject itself alone.
- `a_media_face_fetches_its_object_media_and_keeps_its_texture` — the
  whole MOAP hand-shake but the browser: the update's `MediaURL` version,
  the viewer's own `RequestObjectMedia`, the capability's reply, and the
  face-0 entry landing in `MediaData` with the URL the region published —
  plus, in pixels, the media flag not having blanked the face.

## Three bugs the pictures found

- **The tier was rendering at dawn.** `DAY_POSITION` was `0.25`, commented
  as "the middle of the day track" — on the synthesised preset cycle that
  is *sunrise*. The scene came out in dim pink light: a blue-baked avatar
  classified as no marker colour at all and the ground under a grazing
  camera read as black. Now `0.5`, midday.
- **The tier had no avatar assets, so every avatar was a placeholder
  sphere.** `load_avatar_library` is called by `run_session`, not by any of
  the six plugin groups, so the harness never had it; the same is true of
  `register_ui_fonts`, without which no world-space text lays out at all.
  Both are now loaded by `build_viewer_app`.
- **The catalogue's floating text was invisible.** `PrimFixture::hover_text`
  wrote its alpha byte straight onto the wire, where `TextColor` transmits
  `255 - opacity` — so the catalogue asked for opaque white and got the
  scripter's invisible-text trick. The builder now inverts, as its own doc
  always claimed it did.

## What was left out

- **The media *placeholder*.** The task asked for "the media face fetches
  its `ObjectMedia` and shows the placeholder". `MediaPrimPlugin` and
  `MediaEnginePlugin` are added by `run()` alone, like the avatar library
  and the fonts, and the harness now adds them too — with both engines
  `enabled: false`, which still registers `MediaEngine` / `MediaSurfaces`
  and the `Pump` set `MediaPrimPlugin` schedules against but starts no
  browser. So the fetch half is asserted and the *placeholder* half is not:
  a surface's first paint needs a Chromium process, and a test binary has
  no `sl-cef-helper` beside it to start one with. That belongs to a rig
  that has one.
- **The four `sl-conformance` cases** (`region-crossing`,
  `neighbour-child-circuits`, `terrain-layerdata`,
  `avatar-appearance-npc`) moved to
  [[test-audit-fake-grid-conformance-grid]], which is what adds the
  `Grid::Fake` variant they would run on — `sl-conformance/src/grid.rs`
  still has exactly two variants and no dependency on `sl-fake-grid`.

## New harness and fixture surface

- `ViewerHarness::{start_in_with, hold_clock, capture_after, set_setting}`
  and `HarnessOptions` — a test whose subject *is* time needs a gap it
  chose rather than however long a settle took (a two-second loop sampled a
  whole period apart is the same pose twice), and every difference check in
  the tier holds the clock so a drifting cloud layer and a re-capturing
  reflection probe are not a second reason two frames differ.
- `pixel_oracle::{luminance, changed_centroid}`, both with teeth.
- `sl_test_assets::environment::{single_sky_environment, noon_environment,
  night_environment}` — the *typed* `ExtEnvironment` value beside the
  settings-asset bytes already there.
- `sl_fake_grid::world::rect_parcel`, `fixtures::arrival`,
  `fixtures::border::{border_on_painted_ground, BorderSide::ground_texture,
  BorderSide::ground_color}`.
