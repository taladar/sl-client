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

Launch the [Tracy](https://github.com/wolfpld/tracy) GUI (or `tracy-capture -o
capture.tracy` for a headless recording) and connect to the running viewer. The
per-zone statistics view answers "which system runs how often, and for how
long"; `bevy_render` emits a `tracy.frame_mark` event every frame so Tracy draws
frame boundaries. (That event is filtered out of the human-readable log so it
does not spam the terminal.)

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

#### Machine-readable export (`scripts/tracy-grab.sh`)

The GUI is graphical and streams a lot of data fast, which is awkward when you
want to grep, diff two runs, or hand the numbers to a tool. `tracy-grab.sh`
captures a bounded window headlessly and exports it to tab-separated tables:

```console
scripts/tracy-grab.sh 10 # capture 10s -> tracy-grab-10s/*.tsv
```

It writes `zones-self.tsv` (self time per zone, sorted — the view that surfaces
which systems burn the frame), `zones-inclusive.tsv`, and `messages.tsv`, and
prints the top self-time zones. It needs the `tracy-capture` and
`tracy-csvexport` utilities (from `$PATH`, or built under `$TRACY_DIR`; the
script header has the `cmake` lines). Because Tracy accepts only **one**
profiler connection, disconnect the GUI before capturing — or keep the GUI, use
its **File -> Save trace**, and run `tracy-csvexport -e <file>` yourself.

**Keep the window short (≤ ~10–15 s).** A full-instrumentation Bevy trace emits
~5 k zones per frame (every system, every stage), so a 30 s capture is ~6 M
zones / ~88 MB. At that size the shared `libTracyServer` **Worker load** does
not terminate in reasonable time — a 30 s trace hung both `tracy-csvexport` (95
min, 99.9 % CPU, no output) **and the Tracy GUI (stuck at 91 %)**. A 10 s trace
(~1.2 M zones) loads and exports in seconds. For the "loading / rezzing"
question a 10 s window right after the region handshake is the heaviest, most
representative slice anyway. For a single zone's per-frame timeline (large, so
always filtered):

```console
tracy-csvexport -u -f composite_minimap trace.tracy
```

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
