---
id: viewer-profiling
title: Viewer profiling story
topic: viewer
status: ideas
origin: user request (2026-07-22), during the Vintage-parity coverage audit
refs: [viewer-statistics-floater, viewer-debug-consoles, viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

Profiling for the viewer — where frame time goes, which systems fire how
often, and what allocates. Researched 2026-07-22; the pieces below are the
plan-of-record when this is picked up.

## Frame / CPU time — Tracy (primary)

Bevy already wraps **every system, schedule stage and render step** in
tracing spans; `--features bevy/trace_tracy` streams them to the Tracy UI
live: frame timeline, per-zone statistics (mean-time-per-call, histograms),
capture diffing. Zone statistics directly answer "**which systems run how
often**" — call counts per frame ride along for free. `tracy-capture -o`
records headlessly (fits the screenshot-harness workflow). Gotchas: the
`tracy-client` crate version must match the Tracy GUI
(`cargo tree --features bevy/trace_tracy | grep tracy` against the
rust_tracy_client support table), and profile `--release` (we already do).
Deliverable: `profile-tracy` / `profile-tracy-memory` viewer features
forwarding to Bevy's, plus a book chapter documenting the version pin. Our
own hot paths (tessellation, texture decode, bake compositing) get
one-line `info_span!` zones. `bevy/trace_chrome` → Perfetto stays the
no-GUI fallback for bug reports.

## Sampling profilers (uninstrumented code)

For time outside Bevy spans (wgpu internals, JPEG2000 decode, rustls, the
tokio side): **samply** (Firefox Profiler UI, `samply record <binary>`,
1000 Hz, no code changes) is the Linux first choice; perf + hotspot /
`cargo flamegraph` as alternatives (`-C force-frame-pointers=y`,
`debug = true` in the release profile for stacks). Complementary to
Tracy: sampling finds hot code, Tracy shows frame structure and counts.

## GPU time

Bevy's **`RenderDiagnosticsPlugin`** records per-render-pass GPU + CPU
elapsed and pipeline statistics into the standard `Diagnostics` store —
wire those rows into [[viewer-statistics-floater]] /
[[viewer-debug-consoles]]. Tracy shows GPU spans on its `RenderQueue` row
(mind dynamic-clock variance; read statistics, not single frames). Vendor
tools (Nsight, Radeon GPU Profiler) for shader-level cost; RenderDoc for
frame *content*, not timing.

## Memory / allocations

- **Tracy memory mode** (`bevy/trace_tracy_memory`): every alloc/free
  with call stack, usage plot, active-allocation view, per-span
  allocation events, and alloc↔free pairing = **per-allocation
  lifetimes**, correlated with frame structure ("this system allocates
  4 KB every frame"). Higher overhead; opt-in feature.
- **bytehound** (Linux `LD_PRELOAD`, no rebuild): deepest offline
  analysis — allocation timeline, leak detection, first-class
  **temporary-allocations / lifetime filtering**, scriptable console.
  Large captures on long sessions.
- **heaptrack**: lighter GUI alternative (peak / leaks / temporary
  counts); can OOM on allocation-heavy programs.
- **dhat-rs**: in-process global allocator, cross-platform; counts,
  sizes, **lifetimes**, and — the fit for us — **heap regression tests**
  in plain `cargo test` ("tessellating this prim performs ≤ N
  allocations"), ideal for the pure crates (sl-prim, sl-mesh,
  sl-texture).
- **jemalloc heap profiling** (tikv-jemallocator + jeprof): low-overhead
  statistical sampling for "why does a 6-hour session grow" hunts.
- **Alloc-free enforcement**: `assert_no_alloc`-style guard on realtime
  threads — the [[viewer-audio-backend]] mixer callback should never
  allocate; add the guard when that lands.

## Likely scope when promoted to ready

1. The Tracy feature pair + docs (version pin, tracy-capture recipe).
2. samply/perf notes for uninstrumented paths.
3. RenderDiagnosticsPlugin rows in the statistics floater.
4. dhat heap-regression tests in the geometry/decode crates.
5. assert-no-alloc on the audio thread (with the audio backend).

No bespoke in-viewer profiler UI (the reference's Fast Timers): Tracy +
the statistics floater cover it; revisit only if a user-facing need
appears. The reference for comparison: `llfasttimerview`
(`floater_fast_timers.xml`).
