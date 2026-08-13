---
id: viewer-gpu-avatars-1b-slow-shutdown-high-rss
title: GPU-avatar (1b) session — 10.6 GB RSS + ~2 min 263%-CPU shutdown spin
topic: viewer
status: done
origin: Phase 1b Aditi tracy capture (2026-08-13)
refs: [viewer-perf-gpu-avatar-phase1-gpu-fk-palettes]
---

Context: [context/gpu-avatars.md](../context/gpu-avatars.md).

On the Phase 1b Aditi capture (release **`--features profile-tracy`**, GPU
in-place path active, a ~4 min session that included **walking**), quitting via
the menu left the process **spinning at 263 % CPU with ~10.6 GB RSS**,
streaming to `tracy-capture`, for **~2 minutes** before it finally exited
cleanly (the trace did save, 534 MB). A normal Tracy flush on this project is
**near-instant (≤ a couple seconds)**, so both the multi-minute shutdown and
the 10.6 GB RSS are abnormal — the RSS is the key clue.

Only observed with the **1b GPU-avatar path** active; not seen on earlier
captures (e.g. the Phase 0 full-session capture was 596 MB / 15 M zones and
flushed promptly). It is in the **uncommitted 1b code** — resolve or at least
understand before committing 1b.

## Two hypotheses (distinguish first)

1. **Real memory leak / unbounded per-frame allocation in the 1b path** — would
   grow RSS in *normal* (non-tracy) builds too, so it would also threaten long
   normal sessions (eventual OOM). **Serious.** Candidates to audit: the
   per-frame `LocalPose`/frame/instance staging uploads; the ghost / rigid-ghost
   / "GPU"-label entity lifecycle (spawn without matching despawn?); the pooled
   `joint_map`/IBP arenas (`Arc::make_mut` growth without dedup hits?); the
   socket-FK `deformed_world_chain` scratch; the `SkinInstance` table rebuild.
2. **Tracy-zone-volume blow-up (profiling-only)** — the extra GPU-avatar systems
   plus the **walking-avatar propagation re-dirty** (which re-globals the whole
   joint tree every frame — the known Phase-4-pending cost) flood
   transform-propagation zones faster than `tracy-capture` drains, so the Tracy
   client buffers unboundedly in RAM → 10.6 GB → a slow serialize on exit. Would
   *not* reproduce in a non-tracy build.

## Update — Phase 2 aditi run (2026-08-13): alarm largely defused

Second data point (Phase 2 tracy run, disk now 1.1 TB free): **VmRSS 11.8 GB**,
shutdown flush **~45 s** (elapsed 3:22 vs time-span 2:46), clean 627 MB save.
Cross-run comparison is the useful part:

- 1b: **10.6 GB / ~90 s session / ~2 min shutdown** — but that shutdown was on a
  **~100 %-full disk**, so the trace *write* was crawling; the 2 min was largely
  disk I/O, not flush/mem.
- P2: **11.8 GB / ~166 s session (2644 frames) / ~45 s shutdown** on a free
  disk.

**Similar RSS (~10–12 GB) despite ~2× the session length** ⇒ not a linear ~5
MB/frame leak (that would have pushed P2 well past 15 GB); it looks like a
**high-but-bounded footprint that plateaus** (GPU-avatar buffers + Tracy +
textures), and the scary "2 min shutdown" was mostly the full disk. So:
**not a commit blocker, not an OOM risk.** Still worth understanding the 10–12
GB baseline eventually (compare a non-tracy session's RSS; and where the
GPU-avatar allocations sit). Downgraded from "hard blocker" to "investigate
opportunistically." Note: the intra-session RSS *trend* wasn't captured (window
closed before the monitor sampled mid-run) — a future capture should log RSS
periodically.

## Update — Phase 3 aditi runs (2026-08-13): disk/tracy explanations refuted

Two Phase 3 captures on a **free disk**, same binary family (GPU avatars +
GPU pick), bracket the behaviour and kill the earlier "mostly full disk /
tracy flush" downgrade:

- **P3 run 1:** 4252 frames, **time span 3:04.6, elapsed 3:05.1** — shutdown
  flush **~0.5 s** on a **615 MB** trace. Instant.
- **P3 run 2:** 1410 frames, **time span 52.94 s, elapsed 7:48** — the user
  hit **Quit** ~53 s in, then the process took most of **~7 minutes** to
  exit, on a **free disk** with a **smaller 421 MB** trace.

So within one session type, a **larger** trace flushed in 0.5 s while a
**smaller** one took ~7 min — the delay **cannot** be the tracy serialize,
and the disk was not full. This is an **app-side shutdown hang** on the
Quit → process-exit path, and it is **intermittent** (0.5 s vs 7 min with no
code change between the two beyond the pick crop-cull + a menu-gate flip,
neither shutdown-related). Cross-run: 1b ~2 min (full disk), P2 ~45 s, P3r1
~0.5 s, P3r2 ~7 min. Hypothesis 2 (tracy buffering) is effectively **ruled
out**; hypothesis 1 (app-side teardown not joining/dropping — tokio, a GPU
buffer unmap, an entity-despawn loop, the GPU-avatar buffer/entity lifecycle)
is the live one.

**Next reproduction is the cheap discriminator:** if it hangs on Quit again,
`gdb -p <pid>` → `thread apply all bt` (or `samply`) on the still-running
process reveals where the wall-clock goes — no recompile needed. The
"starting flush" RSS shutdown marker (below) is still worth adding so every
capture self-reports, but the live backtrace is the decisive one and needs
catching the hang in the act.

## Update — no-tracy control run (2026-08-13): points back at tracy

A **plain release build with tracy compiled out** (Phase 4 increment-1 check,
`SL_VIEWER_GPU_AVATARS_READBACK=1`, normally driven, quit via the menu),
instrumented with a 2 s CPU%/RSS sampler across its whole life:

- **RSS plateaued at ~5.15 GB** (648 MB → 5.1 GB, then flat for ~40 samples) —
  **no climb**, so no app-side leak over the session.
- **Shutdown ~3 s, clean**: app rendered until 17:37:44, Quit → teardown began
  17:37:45.5 (readback channel closed), process **EXITED 17:37:48**. No spin.

Against the tracy runs (RSS **10–12 GB**, shutdown 0.5 s … **7 min** spinning),
this is the clean discriminator the earlier cross-trace-size argument only
gestured at. Two conclusions:

- The extra **~5–7 GB** in tracy sessions is **tracy's own buffer**, not app
  memory (no-tracy plateaus at ~5 GB).
- With tracy **out**, a normally-driven/normally-quit run **does not hang** —
  so tracy (its buffer flush / teardown interacting with our exit) is
  implicated in the multi-minute spin, not a shipping-viewer teardown bug.

**Reframing:** this is very likely a **profiling-build-only** shutdown cost,
not a problem for released builds. Still worth the definitive backtrace if it
recurs (`gdb -p <pid>` on the next tracy hang), but **downgraded** — it does
not gate the GPU-avatar work or affect users. The ~5 GB no-tracy baseline
(GPU-avatar buffers + textures) is expected, not a leak.

## RESOLVED (2026-08-13): it is the tracy-client worker, not our code

Caught live and backtraced. During a Phase-4 tracy run the viewer window
closed but the process kept spinning — **193 % CPU, 9.5 GB RSS, 132 s+** after
close. `gdb -p <pid> -batch -ex "thread apply all bt"` (saved:
`scratchpad/shutdown-hang-backtrace.txt`) shows:

- **The one hot thread (98 % CPU) is `Tracy Profiler` (the tracy-client worker
  thread)**, stack: `tracy::Profiler::Worker` (TracyProfiler.cpp:2210) →
  `tracy::Socket::HasData` (TracySocket.cpp:432) → `poll(timeout=0)`. A
  non-blocking poll in a tight loop = the busy-spin.
- **Every one of our own threads is idle** — the IO Task Pools and the
  sl-async/tokio workers are all parked in `futex_wait` / `condvar::wait`.

So the ~9.5 GB is **tracy's buffer** and the multi-minute spin is **tracy's own
client draining that buffer to `tracy-capture`** after `App::run()` already
returned. It is entirely inside the `tracy-client` library — **none of our
code**. Combined with the earlier no-tracy control (clean ~3 s shutdown, ~5 GB
plateau), this is conclusive:

- **Shipping (non-tracy) builds are unaffected** — no such thread, no such
  buffer, clean shutdown.
- **Profiling (`--features profile-tracy`) builds** pay a shutdown flush
  proportional to the buffered trace, and tracy's worker busy-polls while it
  drains — worse when the trace is large / the capturer is slow / disk is full.

**Resolution:** external tracy-client behaviour, profiling-only, **no viewer
fix warranted**. Not a leak, not an OOM risk, not a shipping concern. If it
ever becomes annoying during profiling, the only levers are tracy-side (smaller
captures, faster capturer sink); do not spend viewer effort on it. Closing.

Decision: **do not rabbit-hole now.** Chasing this immediately means a
non-tracy recompile + inconclusive A/B (an hour+); instead 1b is committed and
we **watch RSS + shutdown time across the next few tracy runs** through the
coming phases, then resolve it at the end. If RSS keeps climbing run-over-run
it is a real leak to fix; if this was a one-off it was noise.

Two hypotheses, both still live:

1. **Real leak / unbounded per-frame allocation in the 1b path** (app memory).
   Argues for it: past **60 fps** captures flushed instantly (a *higher* zone
   rate than this 23 fps run), and the zone **count** here is normal
   (**13.45 M**, baseline 15 M), so the 10.6 GB does not look like Tracy zone
   data. If real, ~5 MB/frame (10.6 GB / ~2208 frames) would OOM a long normal
   session. Candidate sites in the audit list below.
2. **Tracy buffering / shutdown-flush artifact** — not fully ruled out; some
   interaction of this build's Tracy client with the 1b systems could still be
   buffering oddly. The capturer's *elapsed* 5:10 vs *time-span* 1:34 (~3.5 min
   of shutdown) is consistent with a big flush.

The cheap discriminator is just **the RSS trend across future tracy captures**
(and whether a non-tracy session's RSS grows) — no dedicated investigation
until the phases are further along.

**Instrumentation to add next (so future runs self-diagnose):** a
**"starting flush" shutdown marker** — right when `App::run()` returns (app
loop exited, Tracy layer still alive), log `VmRSS` + a timestamp
(`/proc/self/status`), e.g. `shutdown: app loop exited, VmRSS=X MB — flushing`.
That records the RSS at shutdown directly in every capture's log — the key
number. The **"finished flush"** side is not cleanly loggable from inside (the
flush is in `tracy-client`'s low-level `Drop`, after the subscriber is torn
down), but `tracy-capture` already prints `Saving trace... done!` when the
flush completes, and the viewer process does not exit until it finishes — so
the bracket is viewer-`flushing`→capturer-`done!` (gap = flush duration).

## Investigation plan

- **Distinguish (1) vs (2) empirically:** run a **plain release (no tracy)**
  for a few minutes with an avatar animating + walking and watch RSS. Climbs to
  GB → hypothesis 1 (real leak, fix before commit). Stays flat → hypothesis 2
  (tracy buffer; the fix is reducing the walking re-dirty zone flood, largely
  Phase 4, and/or accepting slower profile-build shutdown).
- **Backtrace the spin:** reproduce and `gdb -p <pid>` → `thread apply all bt`
  (or `samply`) on the spinning shutdown to see where the 2.6 cores go (Tracy
  client serialize? a teardown loop? tokio not joining? a GPU-buffer unmap?).
- If a leak: bisect by toggling the 1b sub-paths (`=cpu` forces the legacy path
  = control; `=ghost` vs `=real`) to localise which added system leaks.

(Superseded by the Phase 2 update above: 1b and Phase 2 are committed; the
cross-run comparison shows a bounded ~10–12 GB plateau, not an unbounded leak,
and the "~2 min shutdown" was mostly the full disk. Downgraded to
investigate-opportunistically — the plan below stays valid for that.)
