# Roadmap

Planning for `sl-client` lives here as **one small markdown file per task**,
sorted into directories **by status**. This replaces the former set of large
monolithic `*_ROADMAP.md` files at the repository root, which were painful to
edit and merge because every status change was an in-place edit to a shared
multi-thousand-line file.

## How it works

- Each task is a single file: `roadmap/<status>/<topic>-<id>-<slug>.md`.
- A task's **status is the directory it lives in**. To change status, `git mv`
  the file into another status directory. That is the whole workflow — no
  checkboxes to flip inside a shared file, so concurrent work rarely collides.
- `INDEX.md` is a **generated** overview (by status × topic). Never hand-edit
  it; run `python3 roadmap/index.py` to regenerate. Because it is generated,
  merge conflicts in it are resolved by regenerating, not by hand.
- `context/` holds the non-task prose extracted from the old roadmaps —
  legends, conventions, "protocol reality", audit methods, key facts. Read the
  relevant `context/<topic>.md` before working a task in that topic.

## Status directories

| Directory | Meaning |
| --- | --- |
| `ideas/` | Rough, not-yet-fleshed-out notes. |
| `ready/` | Fleshed-out tasks ready to be picked up. |
| `blocked/` | Fleshed out, but waiting on **another roadmap task** — see [Blocking & partial order](#blocking--partial-order). Auto-clears when the blocker reaches `done/`. |
| `in-progress/` | Tasks actively being worked on. |
| `bugs/` | Known bugs / defects. |
| `done/` | Completed work (kept for the record). |
| `deferred/` | Parked for now, but expected to be picked up later, for a reason that is **not** another roadmap task (a non-task prerequisite, a pending investigation, a deliberate wait). Use `blocked/` when the blocker *is* a task. |
| `wont-do/` | Decided against — will never be implemented (obsolete, superseded, or out of scope for good). |

## Topics and IDs

The `<id>` in each filename preserves the **native numbering of the source
roadmap** so existing code comment anchors (e.g. `ROADMAP #23`,
`CHAT_ROADMAP B10`) and cross-references stay resolvable.

| Topic | Source | ID form |
| --- | --- | --- |
| `protocol` | `ROADMAP.md` (#1–#65) | `protocol-NN` |
| `viewer` | `VIEWER_ROADMAP.md` | `viewer-pN-M`, `viewer-rNN` (bugs) |
| `idiomatic` | `IDIOMATIC_ROADMAP.md` | `idiomatic-pN-KK` |
| `chat` | `CHAT_ROADMAP.md` | `chat-aN`, `chat-bN` |
| `permission` | `PERMISSION_ROADMAP.md` | `permission-aN`, `permission-bN` |
| `inventory` | `INVENTORY_ROADMAP.md` | `inventory-aN`, `inventory-bN` |
| `missing` | `MISSING_ROADMAP.md` | `missing-<message>` |
| `test` | `TEST_ROADMAP.md` | `test-<case-name>` (== conformance registry name) |
| `api` | `SL_API_ROAD_MAP.md` | `api-gN`, `api-dfN` |
| `repl` | `SL_REPL_ROAD_MAP.md` | `repl-<phase><n>` |
| `aditi` | `KNOWN_ISSUES_ADITI.md` | `aditi-N` |
| `server` | grid/simulator | `server-<subsystem>` |

## Task file format

```markdown
---
id: chat-b10
title: Chat-log persistence guard
topic: chat
status: ready
origin: CHAT_ROADMAP.md — Phase B
points: 3
refs: [chat-a9, inventory-b3]
blocked_by: [inventory-a1]
---

Prose for this task. Cross-references are written as [[chat-a9]] wikilinks and
resolved by the index generator (which errors on a dangling reference).
```

The `status:` field mirrors the directory; the **directory is authoritative**
if they ever disagree, and `index.py --check` flags the mismatch.

The `refs:` field (plus inline `[[id]]` wikilinks in the body) records loose
cross-references. The `blocked_by:` field is stronger — a hard dependency edge —
and is described next.

## Blocking & partial order

`blocked_by:` is a list of task ids that must reach `done/` before this task may
be worked. It turns the flat status buckets into a **partial order**. The field
is **plain dependency metadata that any status may carry** (an `ideas/` note can
already record what it will depend on); the `blocked/` *directory* is separate,
and narrower — see below.

- A blocker is **cleared** only when the task it names is in `done/`; any other
  status leaves it **open**.
- `blocked/` is one outcome of the fleshed-out pipeline
  (`ideas/` → plan → `ready/` | `blocked/` | `in-progress/` | `deferred/`). Put
  a **fleshed-out** task in `blocked/` exactly when its *only* remaining barrier
  is an open blocker; when the last one reaches `done/`, move it to `ready/`
  (or straight to `in-progress/`).
- An `ideas/` note **keeps its `blocked_by` but stays in `ideas/`** — it is not
  ready to work regardless of its dependencies, so it never lives in `blocked/`.
  The same holds for `deferred/`: a task parked for a *non-task* reason (an
  external prerequisite, a pending investigation) may also carry `blocked_by`,
  but its directory reflects the manual park, not the dependency.
- `blocked_by` must stay **acyclic**, and records only real directed
  prerequisites. An apparent cycle usually means a task boundary is drawn wrong
  — separate the concerns rather than papering over it (e.g.
  `viewer-url-linkification` renders text as clickable links, while
  `viewer-slurl-handling` dispatches SLURL actions to their UI targets; they
  read as co-dependent but are actually independent). Keep looser "related to"
  notes in prose, not in `blocked_by`.

`index.py --check` enforces the ordering (all breaches are fatal):

- every `blocked_by` id resolves; no task blocks itself; no dependency cycles;
- a `blocked/` task has at least one open blocker (else: move it to `ready/`);
- no task in `ready/`, `in-progress/`, or `done/` has an open blocker (else:
  move it to `blocked/`) — you cannot start, work, or finish a task ahead of its
  dependency. (`ideas/` and `deferred/` are exempt: they may hold open
  blockers.)

A dependency on a `wont-do/` task is a **warning** (fatal only under `--check`):
it can never clear, so the dependent is parked forever — drop the edge or
reconsider the task. The generated `INDEX.md` annotates each task's line with
its blockers, tagging any that are already `done`.

## Working in parallel (several worktrees)

When more than one agent works this repo at once, each from its own
`git worktree`, they coordinate through `roadmap/coord.sh`. Its state lives in
the shared `.git` directory, so every worktree sees the same picture, and
nothing about it is committed — the durable record stays what it has always
been: the task file's status directory, moved with `git mv`.

```sh
roadmap/coord.sh status                        # who holds what, on which branch
roadmap/coord.sh claim <id> --subsystem <area> # one agent per task
roadmap/coord.sh release
```

**Read `status` before picking a task.** It lists every live agent's claim and
the crates its unmerged commits already touch. Prefer a task in a different
area from what another agent is rewriting — an import conflict is cheap to
merge, two rewrites of one subsystem are not. Claiming a task another agent
holds is refused; overlapping on a subsystem only warns.

Claiming does not move the file: run the printed `git mv` into `in-progress/`
and commit it yourself, as before.

### Heavy commands take a slot

```sh
roadmap/coord.sh heavy -- cargo clippy -p sl-wire
roadmap/coord.sh heavy --exclusive -- cargo build --release -p sl-client-bevy-viewer
```

Builds, tests and **commits** (the pre-commit hook runs cargo-hack's feature
powerset and the full nextest suite, so committing *is* a large build) go
through `heavy`. It bounds how many run at once, waits for real free memory,
and runs each in its own transient systemd scope.

That last part is the one that matters most: `systemd-oomd` kills whole
cgroups, so a build started straight from the agent's terminal takes the agent
and every one of its subprocesses down with it when memory runs out. In its own
scope, the build is the only casualty — it exits 137 and the session survives.

Use `--exclusive` for a full or release build of `sl-client-bevy-viewer`: a
single rustc for that crate has been measured near 16 GiB, and two of them do
not fit. A `PreToolUse` hook in `.claude/settings.json` denies an unwrapped
heavy command and tells you the wrapped form, so this cannot be forgotten;
`ROADMAP_COORD_BYPASS=1` is the escape hatch. Tuning lives in
`roadmap/coord.conf`.

Before adding a worktree, `export CEF_PATH=$HOME/.cache/cef` —
`.cargo/config.toml` pins `CEF_PATH` relative, so a fresh worktree otherwise
re-downloads 1.8 GiB.
Keep per-worktree `target/` directories; a shared one serialises on cargo's own
build lock, putting a quick check behind a full release build.

## Conventions

- Markdown layout is whatever `rumdl fmt` produces (`rumdl.toml` sets
  `MD013 reflow = true`, i.e. reflow to 80 columns). Do not hand-tune wrapping.
- `python3 roadmap/index.py --check` validates the tree: every `[[ref]]`
  resolves, every `status:` matches its directory, no duplicate ids, and the
  `blocked_by` partial order holds (see above). Use it as a gate before
  committing roadmap changes.
- `python3 roadmap/index.py --locate <id>` prints `<status>` and the file's
  path for one id (exit 1 if unknown). `coord.sh` uses it to reject a claim on
  a finished task and to work out the `git mv` to suggest.
