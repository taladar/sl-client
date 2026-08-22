---
id: build-split-viewer-crate
title: Split the viewer crate to regain cross-crate build parallelism
topic: viewer
status: in-progress
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

Landed so far:

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
- **1** — `sl-viewer-notifications`: the 21.7k-line catalogue, zero outgoing
  edges. Moved with `pub(crate) use sl_viewer_notifications as notifications;`
  in the viewer's `lib.rs`, so none of the 21 consumer files changed.
- **2** — `sl-viewer-platform` (7 modules, 3.2k lines: the XDG layout, on-disk
  caches, clipboard, URL linkification) and `sl-viewer-kit` (19 modules, 10.6k
  lines: geometry math, two render materials with their shaders, small models).
  Named `kit` rather than the plan's `geom` because the set is not all
  geometry. `tracy_plots` and `net_diagnostics` stayed behind: both import
  `tracing_tracy` directly, so moving them would put a `profile-tracy` feature
  — and a doubled `cargo hack` matrix — on a new crate to relocate 251 lines
  that compile to nothing by default.
- **3** — `sl-viewer-settings`: the store, 345 lines. The first step that was
  not a move. `ViewerSettings::load()` named 28 feature modules, which is why
  it could not be a crate; it is now `load_with(registrars)` and the list lives
  at the composition root as `REGISTRARS` in the viewer. A dropped registrar is
  silent — the setting simply stops being declared and its saved value reverts
  — so the declared surface (231 settings, their sections, kinds, defaults and
  flags) is pinned in `tests/settings-golden.txt`, generated before the move
  and still passing after it.
- **6, taken early** — `sl-viewer-testkit`: the headless UI layout harness.
  Planned after the widgets, which was wrong: `ui_tab`, `ui_search`, `menu` and
  `pie_menu` all test through it, so it had to come first. `ui_test` split in
  two — the harness moved, the `ELEMENTS` sweeps stayed in the binary and
  re-export it under the old module name. `LayoutTest` gained
  `with_widget_layout` so it stops naming `pie_menu::fit_pie_layout`; a harness
  below the widgets cannot see one.
- **5** — `sl-viewer-ui-widgets`: 13 modules, 20.6k lines. `UiPointerClaim`
  moved from `hud_pick` to `ui-core` first, which was the last thing tying the
  widget layer to the world picker.
- **4** — `sl-viewer-ui-core`: the UI vocabulary (scaffold and logical box
  model, font stack, Fluent lookup, CSS skin, UI sounds), 10 modules, 8.5k
  lines. Needed two prerequisites. The `ELEMENTS` registry named 35 feature
  modules, so it moved to the binary crate as `ui_elements` — the same
  composition-root argument as `REGISTRARS` — leaving `ui_element` holding the
  vocabulary and the six generic spawners. And `assets/fonts/` moved with
  `ui_font`, whose nine `include_bytes!` are its only consumer, keeping those
  includes crate-local; `.gitattributes` repathed with it. The bundle-content
  tests (Polish `few`/`many`, Arabic `zero`/`two`) moved to the viewer, which
  owns the shipped `.ftl` files.
- **7** — `sl-viewer-media` (6 modules, 2.2k lines: the CEF and GStreamer
  backends behind one boundary, the browser widget, the browser-hosted login)
  and `sl-viewer-spacenav` (852 lines). Both sets were already closed — no
  upward references at all, the first extraction needing no de-cycling. The
  `spacenav` feature moves with its crate and the viewer forwards it, so the
  binary no longer declares `evdev` at all; `sl-media` left with the backends.

## Remaining sequence

Steps, not phases: the phases are sections of this plan (the target graph, the
alias technique, the de-cycling mechanisms, the app-crate policy, the visibility
pass) and apply during every step. The queue is the twenty numbered steps below.

`2` platform + geom (28 leaf modules) · `3` settings (registrar inversion —
`ViewerSettings::load()` runs before any `App` exists, so aggregation moves up
into the app crate, not down into plugins) · `4` ui-core · `5` ui-widgets · `6`
testkit · `7` media + spacenav · `8` world as one crate · `9-17` the nine
feature crates · `18` world split four ways · `19` `world-api`.

Steps 18-19 are where goal 2 is actually delivered: `world` is touched in 268 of
406 commits, so stopping before them leaves the mean at 72% of the monolith
instead of 45%.

## Approach

1. Run `cargo build --release --timings -p sl-client-bevy-viewer` and read the
   HTML report in `target/cargo-timings/` to establish the critical path and
   the self-time of this crate against everything else.
2. Map the module dependency graph. The modules are flat and mostly
   floater/UI-shaped (`about_land`, `avatar_profile`, `group_profile`,
   `search`, `edit_params`, ...), which suggests a separable UI cluster, but
   they are all private `mod`s today so real boundaries need measuring, not
   guessing.
3. Propose a split into a few leaf crates and land it incrementally, starting
   with the cluster that has the fewest inbound edges from the rest.

## Constraints

- Every module is currently private; a split means widening visibility across
  crate boundaries, so it wants a deliberate public-API pass rather than a
  mechanical `pub` sweep.
- This is a large, wide-reaching refactor. It must not land while other
  branches carry substantial viewer work — coordinate merges first.
