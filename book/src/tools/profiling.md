# Profiling the viewer

The Bevy viewer (`sl-client-bevy-viewer`) is wired so that Bevy's built-in
tracing profilers work, even though the viewer installs its **own** `tracing`
subscriber instead of using Bevy's `LogPlugin`.

## Why this needs wiring at all

Login logging happens **before** the Bevy `App` and window exist, so the viewer
installs its subscriber up front (`init_tracing` in `lib.rs`) and disables
Bevy's `LogPlugin` (`.disable::<LogPlugin>()`). Bevy's profilers, however,
attach their tracing layers *through* `LogPlugin` — so with it disabled, a
`trace_chrome` build produced no trace file and Tracy saw nothing.

`init_tracing` therefore re-creates those layers on our subscriber, behind Cargo
features. It also turns on `bevy/trace`, which is what actually wraps every
system, schedule stage and render step in a span for the profiler to see.

## Features

| Feature | What you get |
| --- | --- |
| `profile-tracy` | Stream per-system / per-stage spans to the Tracy UI |
| `profile-tracy-memory` | `profile-tracy` plus per-allocation memory profiling |
| `profile-chrome` | Write a Chrome/Perfetto trace file (no GUI needed) |

All three are off by default and cost nothing in a normal build.

Always profile a **`--release`** build — a debug Bevy runtime is dominated by
its own overhead.

### Tracy (frame timeline, per-system call counts)

```console
cargo run --release -p sl-client-bevy-viewer --features profile-tracy
```

Launch the [Tracy](https://github.com/wolfpld/tracy) GUI and connect to the
running viewer, or capture headlessly to a file (below). The per-zone statistics
view answers "which system runs how often, and for how long"; `bevy_render`
emits a `tracy.frame_mark` event every frame so Tracy draws frame boundaries.
(That event is filtered out of the human-readable log so it does not spam the
terminal.)

**On-demand mode.** `profile-tracy` enables `tracing-tracy/ondemand`, so the
Tracy client collects **nothing until a profiler connects** and discards on
disconnect. Without it, Tracy's default buffers every event in memory the whole
time no client is attached — an untethered run grows without bound (~2 M
zones/s), and on this CEF build that heap growth eventually trips Chromium's
periodic memory-dump `CHECK` (`MallocDumpProvider::OnMemoryDump`), aborting the
process with `SIGILL`. On-demand keeps memory flat between captures (verified:
~3.8 GB steady untethered, vs. >10 GB and an abort without it), so the build is
safe to leave running and connect to only when capturing. The trade-off is that
a capture sees only events from the moment you connect — there is no pre-connect
history.

**Version pin.** Tracy checks its **wire protocol version** on handshake and
refuses to connect on any mismatch — this is effectively an *exact*
Tracy-release match, not merely "same major version", because the protocol bumps
between minor releases. The viewer pins `tracing-tracy = 0.11.4` to match Bevy
0.19's `bevy_log`; that resolves to `tracy-client 0.18.4` →
`tracy-client-sys 0.28.0`, which vendors **Tracy 0.13.1** (protocol 76). Install
the **Tracy 0.13.x GUI** (0.13.1 to be safe). If it reports a protocol mismatch,
confirm the resolved versions and the vendored Tracy release:

```console
cargo tree -p sl-client-bevy-viewer --features profile-tracy -i tracy-client-sys
# then read tracy/common/TracyVersion.hpp in that tracy-client-sys checkout,
# or the crate ↔ Tracy mapping at https://github.com/nagisa/rust_tracy_client
```

`profile-tracy-memory` additionally installs a process-wide global allocator
(from Bevy's `bevy_log`) that reports every alloc/free to Tracy, giving
per-allocation lifetimes correlated with frame structure. It carries real
overhead, so keep it opt-in.

#### Capturing a trace to a file — one command, closed by the viewer

The GUI is graphical and streams a lot of data fast, which is awkward when you
want to grep, diff two runs, or hand the numbers to a tool. Capture headlessly
to a file instead. The **only** reliable pattern is to run `tracy-capture` and
the viewer **together**, and end the recording by **closing the viewer** so the
*client* disconnects cleanly — never bound the capture with `-s` (see below):

```console
# Start the recorder, launch the viewer, drive it, then close the viewer window
# (normal Quit -> AppExit) to flush a complete trace. Do NOT pass -s.
tracy-capture -o trace.tracy -f &   # -f overwrites; blocks until the client connects
BEVY_ASSET_ROOT=… SL_VIEWER_ASSETS=… RUST_LOG=info \
  cargo run --release -p sl-client-bevy-viewer --features profile-tracy -- <args>
wait   # tracy-capture serializes the complete trace once the viewer disconnects
```

`tracy-capture -f` records `while (worker.IsConnected())` and serializes only
once the viewer disconnects, which leaves the worker on a consistent boundary
and writes a complete file. A clean `AppExit` (normal window close) flushes the
Tracy client; an abrupt `SIGKILL` does **not** — so close the window, never kill
the process. Keep the window **visible/unoccluded** the whole time or Bevy
throttles rendering. Because Tracy accepts only **one** profiler connection,
disconnect the GUI before capturing (or keep the GUI, use its **File -> Save
trace**, and export from that file).

**Never use `tracy-capture -s <seconds>`.** When the `-s` deadline fires,
`tracy-capture` calls `worker.Disconnect()` and immediately serializes; at a
high zone rate that cuts the network worker off mid-stream and writes a
**truncated** trace. A truncated file is the "capture won't load" failure —
`tracy-csvexport` and older Tracy GUIs spin indefinitely on it (the loader bug
is fixed in current Tracy, which now errors out instead). **Trace length is not
a data-volume limit**: a complete trace of any length loads and exports fine, so
let the viewer run as long as the question needs and end it by closing the
window. `RUST_LOG` must stay at the `info` default — a stricter filter drops the
spans and the capture shows `Zones: 0`.

`tracy-capture` / `tracy-csvexport` come from `$PATH`, or a Tracy checkout
(default `~/devel/3rdparty/tracy`), built with:

```console
cmake -S capture -B capture/build -DCMAKE_BUILD_TYPE=Release
cmake -S csvexport -B csvexport/build -DCMAKE_BUILD_TYPE=Release
cmake --build capture/build --build csvexport/build
```

#### Machine-readable export

Export the completed `trace.tracy` to tab-separated tables with
`tracy-csvexport` (tab separator so commas inside zone names stay intact):

```console
tracy-csvexport -e -s $'\t' trace.tracy >zones-self.tsv   # self time per zone
tracy-csvexport -s $'\t' trace.tracy >zones-inclusive.tsv # inclusive time
tracy-csvexport -m -s $'\t' trace.tracy >messages.tsv     # log messages
# per-instance timeline for ONE zone (huge, so always filtered):
tracy-csvexport -u -f check_dir_light_mesh_visibility trace.tracy
```

**Do not read the sorted self-time list as "which systems burn the frame"** — it
is summed across all threads and badly over-weights parallel work; see the
*Self-time sums across threads mislead* section below.

Two sharp edges on large traces:

- **`tracy-csvexport` segfaults on large traces** in the parallel sort
  (`ppqsort::execution::par` → `process_blocks_branchless`). Build the Tracy
  utilities from a checkout that forces the **sequential** sort — the `taladar`
  fork carries this as commit `d96fc51f` (`ppqsort::execution::par` → `::seq`
  at every call site). Slower, but it completes; the stock parallel build
  crashes on a ~24 M-zone trace.
- **`-t` / `--truncated_mean` takes an *attached* argument** (`-t90`, not
  `-t 90`); the space form is parsed as a bare `-t` plus a stray positional and
  just prints usage. The `max_ns` / `mean_ns` / `std_ns` columns are always
  present without `-t` anyway, and per-zone `max_ns ÷ mean_ns` is the outlier
  metric — it finds the frames where a system runs far above its own average,
  which the aggregate mean hides.

#### Self-time sums across threads mislead — rank by the wall-clock critical path

`tracy-csvexport`'s default (aggregate) output — and `zones-self.tsv`'s sorted
`total_ns` — **sums each zone across every thread**. That makes it a poor way to
pick an optimization target, because Bevy runs each system on a **single**
thread, and only explicit `par_for_each` / `par_iter` bodies spread across the
~11 worker threads:

- A `par_for_each` body with **6.3 ms summed** self-time is only ~**0.6 ms
  wall-clock** — it parallelises ~10×, and one worker finishing early does not
  shorten the frame.
- A single-threaded system's summed self-time **is** its wall-clock.

Ranking by summed self-time conflates the two and hugely inflates parallel work.
Concretely, on an Aditi rez the "frustum culling" that looked like ~17 ms of the
frame was **summed par-iter self-time**: `check_visibility_cpu_culling` is
~**1.4 ms wall-clock**, runs on a *worker* thread overlapping the main thread,
and is **off the critical path**. Optimising it (e.g. a spatial octree) could
save at most ~1 ms — the summed number was a mirage.

**Finding the real critical path.** The frame is **main-thread bound** and
pipelined across two stages:

- The **main thread** runs the top-level `schedule{name=…}` zones —
  First → PreUpdate → Update → PostUpdate → Last — then the extract
  `sub app{name=RenderExtractApp}`, **sequentially**. Identify it as the thread
  that owns those zones (unwrap any of them and read the `thread` column).
- The **render thread** runs `schedule{name=Render}` **concurrently** (Bevy's
  pipelined rendering: it renders frame *N* while the main thread builds *N+1*).

So `frame ≈ max(main-thread schedules, render-thread Render schedule)`, and
neither stage is idle-waiting much when they are balanced. Rank targets by
**per-instance wall-clock on the gating thread**, obtained with `-u`:

```console
# per-instance INCLUSIVE wall-clock + thread for a system, steady-state window
tracy-csvexport -u -f check_dir_light_mesh_visibility trace.tracy |
  awk -F, '$4>40e9 {n++; t+=$5} END{print (t/n)/1e6 " ms/frame"}'
```

A schedule stage is a **barrier**: it waits for its **slowest single system**,
so a system's per-instance inclusive time (`exec_time_ns`, the `-u` column) —
and which `thread` it ran on — is what matters, not the cross-thread sum. A slow
system on a worker that overlaps the main thread may gate nothing. For an A/B
comparison (culling on/off, before/after a change), compare **steady-state frame
time and the main-thread schedule durations**, never summed self-times.

Worked numbers from one no-culling Aditi trace (steady state, per frame): main
thread 31.7 ms = PostUpdate 13.1 plus extract 8.1 plus Update 6.0 plus PreUpdate
1.6 plus First/Last 0.6; render thread's Render schedule 29.9 ms in parallel;
`present` 7 ms lives on the render thread. The levers were extract and the
render-submit path (both scale with **drawn-object count**), not the cull
algorithm.

### Chrome / Perfetto (no-GUI fallback for bug reports)

```console
TRACE_CHROME=trace.json \
  cargo run --release -p sl-client-bevy-viewer --features profile-chrome
```

The trace file is written when the process exits (the viewer holds the tracer's
flush guard for its whole lifetime). Without `TRACE_CHROME` the file lands at
`./trace-<unix-timestamp>.json`. Open it in
[Perfetto](https://ui.perfetto.dev/) or `chrome://tracing`. Spans are named by
their formatted fields, so a system shows as `system: name=<system path>` rather
than a wall of bare `system` entries.

## What this does *not* cover

Only Bevy-instrumented code lives inside these spans. For time spent outside
them (wgpu internals, JPEG2000 decode, rustls, the tokio side), reach for a
sampling profiler — `samply record <binary>` is the Linux first choice, with
`perf` + `hotspot` or `cargo flamegraph` as alternatives. Sampling finds hot
code; Tracy shows frame structure and counts. The two are complementary.
