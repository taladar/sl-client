---
id: build-split-viewer-crate
title: Split the viewer crate to regain cross-crate build parallelism
topic: viewer
status: ready
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
