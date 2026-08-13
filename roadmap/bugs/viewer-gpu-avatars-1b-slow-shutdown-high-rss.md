---
id: viewer-gpu-avatars-1b-slow-shutdown-high-rss
title: GPU-avatar (1b) session — 10.6 GB RSS + ~2 min 263%-CPU shutdown spin
topic: viewer
status: bugs
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

## Status (2026-08-13): OPEN — two live hypotheses, observe across runs

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

Do not commit 1b until this is understood — a 10.6 GB RSS in ~4 minutes, if it
is hypothesis 1, is a normal-session OOM risk.
