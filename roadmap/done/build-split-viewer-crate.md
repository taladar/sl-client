---
id: build-split-viewer-crate
title: Split the viewer crate to regain cross-crate build parallelism
topic: viewer
status: done
origin: build-performance work (2026-08) — profile/linker tuning landed first
points: 13
---

`sl-client-bevy-viewer` is one crate of ~283k lines across 239 files with 228
private `mod` declarations. Consequences for build time:

- **No cross-crate parallelism.** The whole thing is a single `rustc`
  invocation, so 23 of this machine's 24 cores idle through the frontend.
- **No incremental boundary.** A one-line edit in any of the 239 files
  recompiles all 283k lines.
- **Peak memory.** A single `rustc` on this crate has been observed above
  16 GB resident, which is what makes two concurrent workspace builds
  OOM-kill each other (exit 137, no swap configured).
- **Three full relinks.** `sl-client-bevy-viewer`, `-gallery` and `-scenes`
  each link the whole thing.
- **The build cache cannot help.** A `kache` entry is keyed on the exact
  compilation, so the crate under active development misses by construction —
  in practice the cache only hits on dependencies. The single most expensive
  compile in the workspace is therefore also the one no cache tuning will ever
  reach; the only ways to reduce it are to make it cheaper or to make less of
  it recompile per edit. This task is the latter.

The debuginfo / opt-level / mold tuning already landed (see
[Build performance & memory](../../book/src/tools/build-performance.md)) cut the
binary 2.9x and the DWARF 3.5x, but it does not touch the *compile* cost of this
crate — `.text` was 113 MB before and after. Splitting is the next lever and the
largest remaining one.

Measured on a full `cargo build --release -p sl-client-bevy-viewer`
(`--timings`, 24 cores, warm dependency cache):

| | |
| --- | --- |
| Wall clock | 11m 45s |
| Sum of all unit compile times | 73 min (1051 units) |
| Effective parallelism | ~6.2x |
| **`sl-client-bevy-viewer` alone** | **239 s** |

That one unit is **34% of the wall clock**, and because it is the leaf every
binary depends on, it is a near-serial tail: roughly four minutes during which
23 of 24 cores have nothing to do. Peak RSS for the build was 9.9 GB, and this
crate's `rustc` is what sets it.

## Progress

Measured against 406 real commits touching `sl-client-bevy-viewer/src`, the
target shape is 25 crates: feature crates over a four-way `world` split with a
thin `world-api` types crate beneath them. That last piece is what carries the
win — mean lines recompiled per commit goes 280k (today) → 200k (feature crates
over one fat `world`) → 183k (world split four ways) → **125k mean / 78k
median** once `world-api` exists. A 30-crate variant was measured too and buys
1.8% for five more manifests, so 25 is the stopping point.

Full-build wall clock is the lesser prize and should not be oversold: the
dependency wall is ~420-440s of the 705s build and no viewer split touches it,
so the floor is ~8 min rather than the ~4m30 a lines-based projection suggests.

Landed so far — all twenty steps, `ce64b51d` through `f6592d13`:

- **0a** — every `include!` argument now resolves inside its own crate
  (`ce64b51d`), so the viewer's sixteen-configuration `cargo hack` check stops
  re-running on the ~59% of commits that never touch it. Needed a companion
  change in `global-git-hooks` (`83e0b0f`) so the `OUT_DIR` codegen idiom counts
  as local, which fixed `sl-wire` too.
- **0b** — `[workspace.package]` and `[workspace.dependencies]` (`ee65862c`), so
  a new crate's manifest is a few lines rather than forty, and 35 identical
  `clippy.toml` files collapsed to one at the root. That consolidation caught
  three crates calling `std::fs::read_to_string` where the shared rules require
  `fs_err`.
- **1** — `sl-viewer-notifications` (`4fe6653b`): the 21.7k-line catalogue, zero
  outgoing edges. Moved with `pub(crate) use sl_viewer_notifications as
  notifications;` in the viewer's `lib.rs`, so none of the 21 consumer files
  changed. This is the step that calibrated the ms/line estimate for the rest.
- **2** — `sl-viewer-platform` and `sl-viewer-kit` (`1cc5b6a5`). Named `kit`
  rather than the plan's `geom` because the set is not all geometry.
  `tracy_plots` and `net_diagnostics` stayed behind: both import `tracing_tracy`
  directly, so moving them would put a `profile-tracy` feature — and a doubled
  `cargo hack` matrix — on a new crate to relocate 251 lines that compile to
  nothing by default.
- **3** — `sl-viewer-settings` (`a2afa566`): the store, 345 lines, and the first
  step that was not a move. `ViewerSettings::load()` named 28 feature modules,
  which is why it could not be a crate; it is now `load_with(registrars)` with
  the list at the composition root. A dropped registrar is silent, so the
  declared surface (231 settings) is pinned in `tests/settings-golden.txt`,
  generated before the move and still passing after it.
- **6, taken early** — `sl-viewer-testkit` (`36bd6c61`). Planned after the
  widgets, which was wrong: `ui_tab`, `ui_search`, `menu` and `pie_menu` all
  test through it, so it had to come first.
- **5** — `sl-viewer-ui-widgets` (`71fd10d0`), 13 modules, 20.6k lines.
  `UiPointerClaim` moved from `hud_pick` to `ui-core` first, which was the last
  thing tying the widget layer to the world picker.
- **4** — `sl-viewer-ui-core` (`278dcbbe`), the UI vocabulary. The `ELEMENTS`
  registry named 35 feature modules, so it moved to the binary crate — the same
  composition-root argument as the settings registrars — leaving `ui_element`
  holding the vocabulary and the generic spawners. `assets/fonts/` moved with
  `ui_font`, whose nine `include_bytes!` are its only consumer.
- **7** — `sl-viewer-media` and `sl-viewer-spacenav` (`df87241b`). Both sets
  were already closed — no upward references at all, the first extraction
  needing no de-cycling.
- **8** — `sl-viewer-world` as one crate (`9d4ece02`), preceded by six staging
  commits that lifted the shared state clusters out first (`86e2baf5` selection,
  `92ebb51f` edit-tool modes, `24d67413` mutes, `e8f4ccba` friends, `4c577e64`
  groups, `aa9f00e7` presence and map tracking) and one that settled the last
  code edge (`02a52eb5`).
- **9–17** — the feature crates, bottom-up: `audio` (`ef3d4e1e`), `notices`
  (`1c43dff5`), `map` (`01d043e8`), `search` (`db7cb3c1`), `inventory`
  (`d3847a3c`), `pickers` (`9e49b608`), `edit` (`5c895d50`), `places` and
  `asset-editors` (`e53aa597`), `chat` (`3d9d7001`), `people` (`09f2d216`),
  `preferences` (`5d4c48ad`). Two commits exist only to unblock these:
  `5a252282` staged the cross-tier intents below the surfaces that raise them,
  and `879495e1` rehomed three strays holding back thirty thousand lines.
- **19, taken early** — `sl-viewer-world-api` (`7bcd28a2`). Out of plan order
  because step 8's first attempt died with 135 errors without it. It then grew
  incrementally across the world work (`d30787c2`, `dbb53e40`, `010adbc7`,
  `cbc2eb63`, `493c1f74`, `589e01c4`, `192b8c70`), and `0b8552eb` replaced
  ordering-against-system-names with a `WorldPhase` system set.
- **18** — the world split (`20eb11fc` broke the last cycles, `8a8dac95` did the
  split). **Three crates, not the planned four.** See below.
- **19, completed** — two further commits moved what the feature crates were
  reaching into the world for: `ac3f820b` freed chat, audio and map, and
  `f6592d13` split the decoded-texture store from the fetch machinery, freeing
  inventory, places and search.

## Where this departed from the plan

**The world split three ways, not four.** The plan's four-way cut (objects,
avatar, scene, view) measured at 57 cross-group references over five cyclic
pairs. Merging objects and avatar gave 44 references over two cycles, and both
of those broke cleanly. The avatar layer would not separate from the object
layer because in Second Life an avatar *is* an object and an attachment *is* an
object parented to one: the asset managers, the world-space billboard renderer
that serves both name tags and `llSetText`, derender, and rigged-attachment
skinning all genuinely serve both. What that would take is written up in
[build-split-world-avatar-crate](../ideas/build-split-world-avatar-crate.md),
along with the reason it is not obviously worth doing.

`media_prim` sits in `world-view` rather than with the objects it names. That
placement is what makes the graph acyclic; grouped with the objects it closes a
cycle back through the camera.

**Crate count is 26, not 25** (25 `sl-viewer-*` plus the app crate) — the
feature tier decomposed slightly finer than the plan drew (`places`,
`asset-editors`, `chat`, `search` and `notices` as their own crates), while the
world tier came out one coarser.

## Measured outcome

Full `cargo build --release -p sl-client-bevy-viewer` on the same 24-core
machine. **Caveat:** the baseline ran with a warm dependency cache; the "after"
run started with no `target/release` at all, so its wall clock includes the
whole dependency wall and is if anything pessimistic.

| | before | after |
| --- | --- | --- |
| Wall clock | 11m 45s | **8m 56s** |
| Sum of all unit compile times | 73 min (1051 units) | **61.9 min (1075 units)** |
| Largest viewer unit | **239 s** | **31.2 s** |
| Peak RSS (build process tree) | 9.9 GB | **4.24 GB** |

The plan's Risk 3 said to stop splitting if the unit-sum rose more than ~25%.
It **fell 15%**, so the per-crate re-monomorphization and metadata tax was more
than repaid by what no longer recompiles, even with 24 more units.

The largest unit is now the app crate's own library at 31.2 s, and the largest
world crate is `world-objects` at 28.0 s. The bottleneck has left the viewer
entirely: the most expensive units in the workspace are now `openssl-sys`
(106.6 s, a build script), **`sl-wire` (99.7 s)**, `aws-lc-sys` (95.3 s) and
`bevy_pbr` (80.5 s). Any further build-time work belongs there, not here.

### Incremental rebuilds — the goal-2 measurement

Edit one file, then `cargo build --release -p sl-client-bevy-viewer`. Baseline
was 239 s + link for every one of these, whichever file was touched.

| file (commits touching it) | crate | after |
| --- | --- | --- |
| `menu_bar.rs` (31) | app crate | **43.5 s** |
| `avatars.rs` (79) | world-objects | 133.2 s |
| `textures.rs` (33) | world-objects | 133.5 s |
| `animations.rs` (30) | world-objects | 133.6 s |
| `objects.rs` (82) | world-objects | 139.7 s |
| `ui_element.rs` (35) | ui-core | 150.6 s |
| `settings.rs` (33) | settings | 159.5 s |
| *(no-op rebuild, nothing changed)* | — | 12.0 s |
| *(relink only — edit `main.rs`)* | app bin | 4.6 s |

Real, but well short of what a lines-recompiled model predicts, and the shape
is the interesting part: **editing the 353-line `settings` crate costs more
than editing the 44k-line `world-objects`**, because cost is set by what sits
*above* the edit, not by the size of what was edited. Everything depends on
settings; only the app crate depends on `menu_bar`.

So the ~133 s floor is not link time — a full relink of the 113 MB binary is
4.6 s. It is the dependent crates recompiling: `world-objects` (28 s) cascading
into scene, view, five feature crates and the 31 s app crate. Further splitting
of the *world* would not move it; only shortening the chain above an edit would.

### Where the memory actually goes

The plan's Risk 4 (link becomes the next bottleneck) did **not** materialise,
in either time or memory:

| process | peak RSS | multiplies with `-j`? |
| --- | --- | --- |
| `rustc` | 2.60 GB | yes — this is the OOM-relevant figure |
| `mold` | under 0.45 GB | no (linking is serialised at the end) |
| `kache` daemon | 5.11 GB resident | no — one daemon for the whole machine |

The kache daemon is a single long-lived process shared by every compile, and it
grew 0 MB across a full rebuild, so its high-water mark is a fixed resident
cost rather than something a build re-pays or that scales with parallelism. The
number that matters for two concurrent builds OOM-killing each other is the
per-`rustc` peak, and that is what fell: the viewer's `rustc` set the old 9.9 GB
peak, and no viewer unit now exceeds 2.60 GB.

### Measuring this again — four traps

Every one of these produced a wrong number here before being caught:

1. **kache is the `rustc` wrapper.** An edit whose content matches a previous
   build hits the cache and reports a fraction of the true time. `touch` alone
   is useless, and so is re-appending the *same* probe comment — a second
   identical edit reported 14 s against a true 136 s. Make each probe unique.
2. **`cargo build -p <one crate>` re-unifies features.** Building a single
   member standalone rebuilt dependencies in a different configuration and
   reported 330 s for a subset of a 135 s build.
3. **Deleting `target/release/<bin>` does not force a link.** Cargo hardlinks it
   from `target/release/deps/`, so the rebuild just recreates the link: 8 s and
   no linker ran. Edit the binary's own source instead.
4. **`/usr/bin/time -v` reports the maximum over the process tree**, so it names
   a number without naming the process. Read `VmHWM` per process to attribute
   it, and remember a long-lived daemon's high-water mark predates the build.

## Follow-ups this opened

- [build-split-world-avatar-crate](../ideas/build-split-world-avatar-crate.md) —
  what separating the avatar layer from the object layer would take.
- [build-structural-encapsulation-audit](build-structural-encapsulation-audit.md)
  — the 60 component types that are `pub` only to satisfy exported system
  signatures, and the render handles held where state lives.
- [viewer-ecs-idiom-audit](../ideas/viewer-ecs-idiom-audit.md) — the
  call-into-manager pattern this refactor kept running into. The texture-store
  inversion in `f6592d13` is one instance of the fix; ten systems turned out to
  need no manager at all once the reads moved.
- `sl-wire` at 99.7 s is now the workspace's most expensive Rust unit.
