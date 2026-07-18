---
id: viewer-chat-log-grid-key
title: Key the REPL binaries' chat logs by grid + avatar, not name alone
topic: viewer
status: done
origin: persistence audit while wiring viewer-settings-account-scope-persist (2026-07)
blocked_by: []
refs: [viewer-settings-account-scope-persist]
---

Context: [context/viewer.md](../context/viewer.md).

The chat-log transcripts are keyed by peer / group **display name**
(`<name>.txt`, `<group> (group).txt`) with **no grid or account component** in
the path — the core (`sl-proto`) deliberately delegates the per-account
directory to the host, whose doc says "the host supplies an already-per-account
path". The viewer now does that (via [[viewer-settings-account-scope-persist]] /
`sl-account-dirs`), but the **two REPL binaries still don't**:

- `sl-repl-tokio` and `sl-repl-bevy` pass the bare `--chat-log-dir` CLI argument
  straight through as `agent_chat_log_dir`, with no derivation. So `Alice
  Resident.txt` (or `My Group (group).txt`) from OpenSim, Agni and Aditi all
  collide into the same transcript when one `--chat-log-dir` is reused.

Fix: resolve the per-avatar directory with the same `sl-account-dirs`
`reconcile_account_dir` the viewer uses, so both REPLs write transcripts under
`<chat-log-dir>/<grid>/<name>/` (grid + readable name, UUID rename discovery).

- **tokio REPL** (`sl-repl-tokio`): resolve between `connect()` and `run()`
  (the agent UUID is known post-connect, before any cache is touched) and set it
  via `set_directories`.
- **bevy REPL** (`sl-repl-bevy`): pass an `AccountDirsConfig` to
  `SlClientPlugin` (the field already exists, currently `None`), exactly as the
  viewer does.

`--chat-log-dir` becomes the accounts *base* rather than an already-per-account
path. Also consider the inventory cache dir (`agent_cache_dir`, currently `None`
in both REPLs) — it can adopt the same base.

Low-priority sibling noted in the same audit: the conformance aditi-login
cooldown stamp (`sl-conformance/src/context.rs`) keys by avatar name and is only
grid-safe today via its hardcoded `aditi-last-login/` directory; add a grid
component if a second rate-limited grid is ever introduced.

## Done

Both REPLs now resolve their `--chat-log-dir` through
`sl_account_dirs::reconcile_account_dir`, so transcripts land in
`<chat-log-dir>/<grid>/<name>/` (grid + readable name, UUID rename discovery)
instead of colliding by name:

- **tokio** (`sl-repl-tokio`): resolves between `connect()` and `run()` — the
  agent UUID is known post-connect, before any cache is touched — and sets the
  keyed dir via `set_directories`. Falls back to the base as-is on a filesystem
  error or an unparsable login URI.
- **bevy** (`sl-repl-bevy`): passes an `AccountDirsConfig` (chat-log base =
  `--chat-log-dir`, inventory base `None`) to `SlClientPlugin`, which reconciles
  it inline at login — the same path the viewer uses.

The plugin's `AccountDirsConfig` was reshaped this pass to carry **two separate
accounts roots** (chat vs inventory) rather than one, so the viewer can honour
the XDG split (chat → state, inventory → cache, settings → config) while the
REPLs supply only a chat base. The conformance cooldown-stamp sibling is left
as noted (still grid-safe in practice).

Verified: all four crates build + clippy-clean. Live transcript-path exercise
left to a real grid run.
