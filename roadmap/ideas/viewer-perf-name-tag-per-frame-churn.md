---
id: viewer-perf-name-tag-per-frame-churn
title: Name-tag systems recompose / re-solve every frame (ungated)
topic: viewer
status: ideas
origin: per-frame redundant-work audit (2026-08-12)
refs: [viewer-perf-steady-state-46fps-ceiling, viewer-perf-frame-churn-cleanups]
---

Context: [context/viewer.md](../context/viewer.md).

The name-tag chain runs several ungated per-frame systems whose inputs change
only on avatar-list / chat / status events. Their **output writes are already
`!=`-guarded**, so the downstream renderer stays quiet — but the compose /
sort / spring-solve / allocation work upstream is pure waste on the ~99 % of
frames where nothing changed. Cost scales with crowd size; on aditi the
name-tag systems sit in the main-app `Update`/`PostUpdate` chains that
co-limit the frame ([[viewer-perf-steady-state-46fps-ceiling]]).

## Findings

### 1. `compose_name_tags` — full re-compose + per-avatar alloc/format

`name_tag_content.rs:492` (registered `lib.rs:1880`, Update, ordering only —
no `run_if`, no `Changed`). Every frame, for **each** avatar it builds a fresh
`Vec<TagLine>`, `states.join(", ")` (`:394`), `title.to_owned()`, and
`format!("{distance:.2} m")` (`:449`). Only the distance cache is throttled
(0.25 s, `:521`); the composition is not. The `if *content != composed`
compare-then-assign (`:575`) keeps the renderer quiet, so all the compose work
is wasted when names/titles/status are unchanged. Fix: dirty-flag / `run_if`
on avatar-list + status + distance-tick changes; recompose only flagged tags.

### 2. `solve_tag_overlap` — O(n²)×10 spring solve + Vec/sort alloc

`name_tag_billboard.rs:1581` (registered `lib.rs:2271`, PostUpdate, no
`Changed` gate). Each frame allocates `entries: Vec` (`:1615`), `sort_by_key`
(`:1635`), a second Vec + a `HashMap` (`:1637`/`:1640`), then runs
`solve_overlap_offsets` = O(n²) × `OVERLAP_ITERATIONS` (10) (`:1485`) from
zero offsets; it only freezes while the camera moves *fast* (`:1650`). Fix:
skip the solve when the input screen-rect set is unchanged since last frame
(hash the rects); make the four transient buffers `Local<>` scratch
(clear+reuse). Tag rects settle whenever camera + avatars are slow/still.

### 3. `follow_tag_anchors` — all-tags screen projection every frame

`name_tag_billboard.rs:1298` (`lib.rs:2270`), no `Changed` filter. Writes are
`set_if_neq`-guarded and smoothing snaps when settled, so the residual is only
the unconditional projection + all-tags scan. Minor; batch with #2.

## Note: the frame-churn item's `position_name_tags` reference is stale

[[viewer-perf-frame-churn-cleanups]] lists `position_name_tags`
(`avatars.rs:3788`) as the name-tag churn item — that function has been
**refactored away** into the `follow_tag_anchors` / `solve_tag_overlap` /
`compose_name_tags` chain above. This item supersedes that bullet.

## Related small per-frame churn (same audit, lower value)

- `beacons.rs:976` `update_beacon_overlay` — per-frame `format!("{}\n{:.0} m")`
  before the guarded assign; the string changes ~1 Hz. Format only on change.
- `minimap.rs:1727`/`:1790` `composite_minimap` — while the floater is open,
  an `avatars.map_avatars()` scan + a `dots` Vec alloc run each frame even when
  the quantised stamp is identical. Compute a stamp key before allocating.
- `minimap.rs:2376` `update_minimap_hover` — writes tooltip `Node.left/top`
  **unguarded** while hovering; add a `!=` guard (can dirty UI layout).
- `hover_text.rs:352` `follow_hover_text` — per-frame `env::var_os` read (same
  uncached-env pattern as [[viewer-perf-avatar-pose-extract-skins]] item 3).

## Confirmed clean (audited, no action)

`drive_sky` / `drive_water` / `drive_clouds` / `drive_stars` (compare-then-
`get_mut`, `set_if_neq`), all gizmo systems (`run_if(edit_tool_active)`),
`update_parcel_borders` (dirty + budgeted), `update_status_readouts` /
`update_parcel_icons` (`on_timer` / `run_if`), `report_camera_interest` /
`report_agent_viewport` (rate-limited + change-gated), `pose_attachment_nodes`
(per-joint `is_changed()`), the `skin.rs` systems
(`resource_changed`/`Added<T>`).
