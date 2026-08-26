---
id: build-split-world-avatar-crate
title: Separate the avatar layer from the object layer in the world split
topic: viewer
status: done
origin: crate-split work (2026-08) — the four-way world split reduced to three
points: 8
refs: [build-split-viewer-crate, viewer-ecs-idiom-audit,
  build-structural-encapsulation-audit]
---

Context: [context/viewer.md](../context/viewer.md).

The world was split three ways -- objects, scene, view -- and not the four the
plan drew, because objects and avatars would not come apart. This finishes the
job: `sl-viewer-world-avatar` now exists, and the object layer names nothing in
it.

## What the re-measure found, and why it changed the plan

The note said to re-measure first. The snapshot it was written from had the
four-way grouping at **five cyclic pairs and 57 cross-group references**, and
merging objects with avatar at **two cycles and 44**.

Re-measured against the tree as it stood, with the grouping corrected in one
place -- `ground.rs` is the *avatar* ground probe, not terrain, and had been
counted on the object side -- an avatar/object cut came out at **nine
object→avatar module edges over 15 references**, and every one of them was a
relocation rather than a redesign.

The reason the number collapsed is the reason the original plan looked so
expensive: **it assumed the two halves would be peers.** They are not. In
Second Life an avatar *is* an object, so the dependency has a direction: the
avatar layer sits **above** the object layer and calls down into it. Every one
of the plan's hard cases is a *downward* call once that is settled:

- **The asset managers** -- the plan's headline item, "the largest single
  piece". `TextureManager` and `MeshManager` stay with the objects and the
  avatar side calls them, which is now a call *down* the stack and needs no
  inversion at all. [[viewer-ecs-idiom-audit]] had already inverted the half
  that was genuinely backwards (the `F3` overlay's statistics read, which was
  the scene layer reaching *up*); what was left never pointed the wrong way.
  **The manager inversion this task was waiting on turned out not to be a
  prerequisite.**
- **Rigged attachments** -- `objects.rs` knowing about `AvatarBody`, the BoM
  face materials and the GPU skin binding. Those systems moved *up*.

So the answer to "is it worth it" changed with the direction. The note said to
do it only when the manager inversion was wanted anyway; the inversion was not
needed, and what remained was a day's relocation.

## What moved, and why it went where it did

Nine object→avatar edges, four fixes.

**The billboard renderer stayed in the object layer, and its vocabulary came
down to it.** `name_tag_billboard` is filed under avatars but draws an object's
`llSetText` too (`hover_text`), and `NameTagBillboardPlugin`'s single chain
deliberately interleaves the two so a settings change and a content change
reach the same frame's meshes. Splitting that chain across a crate boundary
would have meant a shared `SystemSet` in `world-api` and two plugins
contributing to it -- for no gain, since the renderer has to sit below both of
its users either way and the object layer *is* the lower one. So it stayed, and
the four items it was reaching upward for moved down into it: `NameTag` (the
marker its own placement system queries by), the two `ShowNameTags` /
`ShowOwnNameTag` settings (beside the fade and bubble settings already there),
and `TagContent` / `TagLine` / `TagLineSize` -- the line-and-tier format
*both* writers fill in, which had been filed with the avatar one.

**Derender moved whole.** The note asked where the avatar half belongs; the
answer is that the object half has no home requirement. `enforce_derender`
drains one pending list into both `ObjectState` and `AvatarState`, and both of
those live in `world-api` -- only `avatars::derender_agent` (the body → coarse
placeholder hand-off that keeps the radar from seeing a leave/enter) is avatar
code. Splitting the system in two would have put two `mem::take`s on one queue
for nothing.

**Asset-store statistics split in two.** `asset_stats` published five stores;
two of them (`AnimationManager`, `WearableAssetManager`) are the avatar
layer's. It is now three there and two in the avatar layer's
`avatar_asset_stats`, both publishing into the one `PipelineStats` resource
`world-api` owns. The `F3` overlay still reads a single resource and neither
half knows the other exists.

**Rigged attachments became their own module, in the avatar crate.**
`rigged_attachments` takes ~890 lines out of `objects.rs`:
`adopt_pending_attachments` / `route_hud_attachment` (parenting a worn object to
its wearer's attachment-point node, or routing a HUD one onto the screen-space
layer), `apply_rigged_attachments` and `build_rigged_submeshes` (binding a worn
rigged mesh to the wearer's skeleton instance), and
`spawn_animesh_control_avatars` / `prune_control_avatars`. `animesh_root` -- the
linkset walk the note listed among the "small pure items" -- moved up too, into
`animesh`, which is where both its callers now are, and
`joint_overrides_enabled` with it.

## What it cost the object layer's encapsulation

Moving those systems out means the object layer has to *say* what a deferred
build is, where it used to keep it private. **Seven private items turned
`pub`** -- `PendingGeometry` with its three payload structs, and three
`PendingBuilds` methods -- plus a new `PendingBuilds::pending` accessor, added
so `ObjectBuilds`' own fields could stay private. **Fourteen more were already
`pub(crate)`** and widened: `ObjectBuilds`,
`MeshManager::{skin, header, lod_change_inflight}`,
`TextureManager::{forget, request_server_bake, native_dimensions}`,
`textures::{tint_color, face_material}`, `MeshDecoded`'s field,
`TextureApplyBudget::take_image`, `name_tag_render_bundle`, and the two fade
defaults.

That is the honest price, and it is the same one every step of
[[build-split-viewer-crate]] paid: a crate boundary makes a module's
collaborators name what they use. It runs against
[[build-structural-encapsulation-audit]]'s direction, and the trade is
deliberate -- the audit was about items `pub` for *no* reader, and each of these
has exactly one.

## Measured outcome

The world tier, before and after:

| crate | before | after |
| --- | --- | --- |
| `sl-viewer-world-api` | 6.8k | 6.8k |
| `sl-viewer-world-objects` | **44.8k** | **16.3k** |
| `sl-viewer-world-avatar` | — | **28.8k** |
| `sl-viewer-world-scene` | 18.6k | 18.6k |
| `sl-viewer-world-view` | 11.9k | 11.9k |

And the shape is not a chain: nothing in `world-scene` names an avatar module,
so `world-avatar` and `world-scene` are **siblings** over `world-objects` and
compile in parallel. `world-view` is the join.

The two most-edited files in the workspace, `objects.rs` (82 commits) and
`avatars.rs` (79), are now in different crates -- which is the whole point.
`sl-viewer-people` dropped its dependency on the object layer entirely.

### Incremental rebuilds

Edit one file, `cargo build --release -p sl-client-bevy-viewer`, measured
back-to-back in two trees (the "before" one a `git worktree` at the parent
commit). Each probe appends a **unique** comment, because a repeated identical
edit hits the `kache` wrapper and reports a fraction of the true time — trap 1
of [[build-split-viewer-crate]]'s four.

| file edited | crate after the split | before | after | |
| --- | --- | ---: | ---: | ---: |
| `avatars.rs` | `world-avatar` | 144 s | **110 s** | −24% |
| `animations.rs` | `world-avatar` | 140 s | **101 s** | −28% |
| `objects.rs` | `world-objects` | 139 s | **115 s** | −17% |
| `textures.rs` | `world-objects` | 139 s | **116 s** | −17% |
| `name_tag_billboard.rs` | `world-objects` | 145 s | **116 s** | −20% |
| *mean* | | 141 s | **112 s** | **−21%** |

Both sides got faster, not just the half that moved: an edit to `objects.rs`
still cascades into the avatar *and* scene layers, but the crate it recompiles
first is now a third of the size.

### The critical path

`--timings` again, but read as cargo's own unit-unblock graph rather than as a
wall clock: the longest dependency chain, on infinite cores.

Restricted to workspace crates (the dependency halves of the two runs are
**not** comparable — the before tree was a fresh `git worktree`, so its chain
was dominated by a 112 s `cef-dll-sys` download while its Bevy tree came from
cache):

- **before:** `… → world-objects:42.3 → world-scene:22.5 → world-view:16.6 → …`
- **after:** `… → ui-widgets:21.6 → world-scene:26.5 → world-view:20.9 → …`

`world-objects` was the largest workspace unit **on** the critical path at
42.3 s. It is now 14.2 s and **off the path entirely** — `ui-widgets` overtook
it as what gates `world-scene`, and `world-avatar` (21.4 s) never joins the
chain at all because it runs beside `world-scene` rather than before it.

**Noise band:** unchanged crates drifted up to ~20% between the two runs
(`world-view` 16.6 → 20.9 s, the app crate 26.8 → 28.0 s), so single unit times
are worth ±20% and no more. The 3× drop in `world-objects` and the incremental
deltas are well outside that; nothing else here should be read finer.

### What the analysis turned up next

The same critical path says the build is now **entirely chain-bound** — a
279.1 s modelled chain against a 279.7 s wall clock on 24 cores — and that the
world tier is no longer where the chain is. Two follow-ups came out of it,
both `ready`:

- [[build-flatten-feature-tier]] — `world-view → inventory → people →
  preferences → app` is a chain where it should be a fan, and 41 of the edges
  holding it together are `SETTING_*` string constants.
- [[build-split-ui-widgets-crate]] — `pie_menu` is 3.4k lines with one
  consumer and no coupling to its twelve siblings, inside the crate that now
  carries 140 s of dependents.

## Follow-ups

- `world-objects`' manifest lost nine dependencies that were only ever the
  avatar modules' (`image`, `jiff`, `reqwest`, `serde`, `serde_json`, `sl-anim`,
  `sl-avatar`, `sl-texture`, `fs-err`). Worth a sweep of the other crates for
  the same thing.
- [book/src/tools/build-performance.md](../../book/src/tools/build-performance.md)
  still opens with "`sl-client-bevy-viewer` alone is ~283k lines across 239
  files". That went stale when [[build-split-viewer-crate]] landed, not here,
  but it is now two splits out of date.
