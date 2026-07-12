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
| `in-progress/` | Tasks actively being worked on. |
| `bugs/` | Known bugs / defects. |
| `done/` | Completed work (kept for the record). |
| `deferred/` | Explicitly parked or out-of-scope items. |

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
---

Prose for this task. Cross-references are written as [[chat-a9]] wikilinks and
resolved by the index generator (which errors on a dangling reference).
```

The `status:` field mirrors the directory; the **directory is authoritative**
if they ever disagree, and `index.py --check` flags the mismatch.

## Conventions

- Markdown layout is whatever `rumdl fmt` produces (`rumdl.toml` sets
  `MD013 reflow = true`, i.e. reflow to 80 columns). Do not hand-tune wrapping.
- `python3 roadmap/index.py --check` validates the tree: every `[[ref]]`
  resolves, every `status:` matches its directory, no duplicate ids. Use it as
  a gate before committing roadmap changes.
