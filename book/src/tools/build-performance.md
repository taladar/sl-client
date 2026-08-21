# Build performance & memory

This workspace is large: `sl-client-bevy-viewer` alone is ~283k lines across
239 files, and it links against a patched fork of all 65 Bevy crates. Builds
are correspondingly expensive, and two heavy `rustc` processes running at once
can exhaust this machine's memory. This page records the settings that keep
that in check and, more importantly, *why* they are what they are — so nobody
"restores" `debug = 2` without knowing what it costs.

## The debuginfo setting

The single most expensive knob is the debuginfo level. Measured on release
builds of the viewer binary, before (`debug = 2`) and after
(`debug = "line-tables-only"`):

| Section | `debug = 2` | `line-tables-only` |
| --- | --- | --- |
| `.debug_info` | 932 MB | 282 MB |
| `.debug_str` | 863 MB | 286 MB |
| `.debug_loc` | 757 MB | — |
| `.debug_ranges` | 167 MB | 113 MB |
| `.debug_line` | 124 MB | 112 MB |
| **DWARF total** | **2.86 GB** | **818 MB** |
| `.text` (actual code) | 113 MB | 113 MB |
| **Binary total** | **3.17 GB** | **1.10 GB** |

The "before" binary was **90% debug information**. Dropping to line tables
removes `.debug_loc` entirely and cuts `.debug_info` by two thirds, for a
**2.9x smaller binary** and **3.5x less DWARF** — while `.text` is, as it must
be, unchanged.

The two figures are from different worktrees and so not from a byte-identical
revision; treat them as the right order of effect rather than a controlled A/B.

This is multiplied several times over in practice: the workspace builds three
such binaries (`sl-client-bevy-viewer`, `-gallery`, `-scenes`), and the ggh
hooks build the workspace into five *further* target directories
(`deny_warnings`, `doc_check`, `nextest_check`, `hack_check`, `clippy`).
Target directories of 100–150 GB were the normal result before this change.

The workspace therefore sets:

```toml
[profile.release]
debug = "line-tables-only"

[profile.dev]
debug = "line-tables-only"
```

### Why this does not hurt profiling

Profiling on this project happens on release builds, with Tracy and `perf`,
plus `gdb -p <pid> -ex "thread apply all bt"` for hung threads. None of those
need full DWARF:

- **Symbol names** come from `.symtab`, which is unaffected.
- **File / line attribution and inline frames** come from `.debug_line`, which
  `line-tables-only` keeps in full.
- **`perf --call-graph dwarf` stack unwinding** walks `.eh_frame`, which is
  emitted regardless of the debuginfo setting.

What `debug = 2` adds on top is `.debug_info` / `.debug_str` / `.debug_loc` —
the variable and type descriptions that only a debugger *inspecting local
variables* reads. That is 2.7 GB of the 2.86 GB above.

### When you really do need locals

Use the dedicated profile:

```console
cargo build --profile release-debug -p sl-client-bevy-viewer
```

It is `release` plus `debug = 2`, and it builds into `target/release-debug/`,
so it never invalidates the normal release artifacts.

### The alternative that was considered and rejected

`split-debuginfo` (stable on this target; `packed` writes a `.dwp` beside the
binary, `unpacked` leaves `.dwo` files in `target/`) keeps full DWARF while
taking it out of the linked binary, which makes the link cheap. It was rejected
as the default because `perf` has never handled `.dwo`/`.dwp` well, and trading
away profiler source attribution is the wrong direction for this project. If
full DWARF ever becomes a routine need, `debug = 2` plus
`split-debuginfo = "packed"` is the combination to reach for — prefer `packed`,
since `unpacked` scatters `.dwo` files that a `cargo clean` silently
invalidates.

## Optimization levels

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

This is Bevy's standard fast-build recommendation, with one deviation. Bevy
suggests leaving your own crates at `opt-level = 0` on the assumption that the
engine dominates frame time. That does not hold here — this workspace carries a
lot of performance-sensitive code of its own (the pose driver, culling, UI
layout, the parry3d raycast index) and is unusable unoptimized. `opt-level = 1`
captures most of the runtime win for a fraction of `opt-level = 3`'s compile
cost, and leaves debug assertions and overflow checks on.

Dependencies compile once and are cached, so paying `opt-level = 3` on the
dependency wall costs almost nothing per iteration.

**Measured effect:** a dev build of the viewer against OpenSim went from **~1
FPS** to **12–30 FPS**. Release on the same grid is usually nearer 60. So a dev
build is now genuinely usable for functional and visual checks, and iteration no
longer has to go through a release build.

The corollary matters just as much: **12–30 FPS is not a performance
measurement**. Anything about frame cost — Tracy or `perf` captures, frame-time
budgets, judging whether something is fast enough — still needs `--release`.

Note that the *first* dev build after this change costs about as much as a
release build (~11.5 min here), since the whole dependency wall recompiles at
`opt-level = 3`. That is one-time; incremental rebuilds of the workspace's own
crates afterwards are much cheaper than the release equivalent, which is the
entire point.

## The linker

```toml
# .cargo/config.toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-Clink-arg=-fuse-ld=mold"]
rustdocflags = ["-Clink-arg=-fuse-ld=mold"]
```

Worth knowing: this is an **lld → mold** upgrade, not the usual GNU-ld → lld
one. This toolchain's `cc` already defaults to `-fuse-ld=lld`, so the standard
"configure a fast linker" advice was already half-applied before anyone
configured anything. `rustdocflags` is set too because doc tests link, and are a
known slow spot ([bevyengine/bevy#12207]).

[bevyengine/bevy#12207]: https://github.com/bevyengine/bevy/issues/12207

## Nightly flags: deliberately not used

Bevy's `config_fast_builds.toml` template also recommends `-Zshare-generics=y`,
`-Zthreads=0` and `-Zno-embed-metadata`. These are real wins, especially
`share-generics` across 65 Bevy crates, but they require nightly, and the ggh
hooks and the workspace's clippy configuration run on stable. The workspace
stays on stable; revisit if these stabilize.

## Do not run two heavy builds at once

A single `rustc` compiling the viewer crate has been observed above 16 GB
resident. Two at once exceed this machine's memory and get OOM-killed (exit
137), taking both builds down. There is no swap configured, so the kernel has no
slack to fall back on.

In practice:

- Run `cargo build`, `cargo test` and `cargo clippy` for this workspace **one
  at a time**.
- Remember that parallel worktrees share the machine, and that the ggh hooks
  themselves build the workspace — a commit in another worktree counts as a
  heavy build.
- A build that looks stuck may simply be queued behind another worktree's link
  step.

Before starting a heavy build, check whether anything is already compiling:

```console
pgrep -a rustc
ps -o pid,rss,etime,args -p "$(pgrep -d, rustc)"
```

## The build cache

Builds go through `kache` as `RUSTC_WRAPPER` (configured globally in
`~/.cargo/config.toml`, not in this repo), with a local store capped in
`~/.config/kache/config.toml`. Useful commands:

```console
kache stats          # hit rate, store size against the cap
kache gc             # LRU-evict back under the cap
kache why-miss CRATE # why did this crate not come from the cache
```

**Do not read the hit rate as a health metric.** On a normal day of development
it is structurally capped, and a low number is not evidence of a
misconfiguration:

- A cache entry is keyed on the exact compilation. The workspace's own crates
  are *new code on almost every build* — that is what development is — so they
  miss by construction. Realistically kache can only hit on **dependencies**,
  plus the occasional case of a fresh worktree building a revision another
  worktree already built.
- It is additionally near-zero right after a purge or a `target/` wipe.

The uncomfortable consequence is that the **most expensive compile in the
workspace — the viewer crate — is the one that can never be cached**, because it
is the crate that changed. No cache tuning reaches it. That is precisely why the
settings above (which cut the intrinsic cost of every compile) and the crate
split tracked in the roadmap (which cuts how much has to be recompiled per edit)
are the levers that actually matter here.

The debuginfo settings do help the cache indirectly: artifacts around a third of
their former size mean the same store holds correspondingly more entries before
eviction starts.

## Measuring a change

```console
# per-crate breakdown, HTML report in target/cargo-timings/
cargo build --release --timings -p sl-client-bevy-viewer

# peak RSS of the build
/usr/bin/time -v cargo build --release -p sl-client-bevy-viewer

# DWARF share of the resulting binary
objdump -h target/release/sl-client-bevy-viewer |
  awk '$2 ~ /^\.debug/ {s+=strtonum("0x"$3)} END {print s/1048576, "MB"}'
```

The `--timings` report is also the right starting point for the largest
remaining lever, which is structural rather than configuration: the viewer is
one crate of 283k lines with 228 private modules, so there is no cross-crate
parallelism and any one-line edit recompiles all of it. Splitting it is tracked
separately in the roadmap.
