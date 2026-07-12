---
id: inventory-a4
title: Design the disk-cache file layout & the ClientDirectories struct
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A4. Design the disk-cache file layout & the `ClientDirectories`
    struct.** File naming (`<agent-uuid>.inv.llsd.gz` for agent, a distinct
    `<agent-uuid>.lib.inv.llsd.gz` for library) placed **directly** in the
    caller-supplied directory (no derived subdir); atomic write (temp file +
    gzip + rename); the version-header check on load (mismatch ⇒ ignore the
    file). Define `ClientDirectories` (three `Option<PathBuf>` fields —
    `agent_cache_dir`, `agent_chat_log_dir`, and a reserved
    `shared_cache_dir`) in `sl-proto` (next to `ChatLogConfig`), passed once
    at construction, and the **chat-log retrofit**: `ChatLog::new` takes
    `agent_chat_log_dir` verbatim and drops the `clean_file_name(own_name)`
    subdir join. Specify what changes in the existing chat-log tests.

## Disk-cache layout & directories reference (from A4)

Files `<agent-uuid>.inv.llsd.gz` (agent) and `<agent-uuid>.lib.inv.llsd.gz`
(library) are written **directly** under the caller's `agent_cache_dir` (no
derived subdir). **Crash-safe atomic write** (a save interrupted mid-write must
never corrupt or lose the existing cache): write the complete gzip to a
distinctly-named temp file **in the same directory** (e.g.
`<agent-uuid>.inv.llsd.gz.<pid>.tmp`, so it shares the target's filesystem and a
concurrent save cannot clobber it), `flush` + `fsync` it, then atomically
`rename` it over the target — POSIX `rename(2)` is atomic, so any reader or a
crash sees either the intact old file or the intact new one, never a truncated
blend; on Windows the runtime shell uses the replace-style rename. On error the
temp file is removed and the old cache is left untouched. The library cache
(`.lib.inv.llsd.gz`) is written the same way.
Load: gunzip, read the 4-byte BE version, treat the file as cold (ignore) unless
it equals `5`, else `parse_llsd_binary` the remainder. `ClientDirectories` lives
in `sl-proto` next to `ChatLogConfig` with three `Option<PathBuf>` fields —
`agent_cache_dir`, `agent_chat_log_dir`, `shared_cache_dir` (reserved) — each
`None` disabling that feature; it is passed once at each runtime's construction.
Chat-log retrofit: `ChatLog::new` takes `agent_chat_log_dir` verbatim (drop the
`.join(clean_file_name(own_name))`); the now-redundant `ChatLogConfig.log_dir`
is removed. The chat-log tests that assert the `Me Resident` subdir change to
assert files directly under the supplied dir.

**Surface verified against the code (anchors for B9/B10).** `ChatLogConfig` is
`sl-proto/src/chat_log.rs:168-202`; the field to remove is
`log_dir: Option<std::path::PathBuf>` (`:175`, defaulted `None` at `:208`), and
`clean_file_name` is `:246`. The directory is **read in exactly two places** —
the byte-identical `ChatLog::new(config, own_name, own_id)` shells at
`sl-client-tokio/src/chat_log.rs:157` **and** the bevy copy at the same `:157` —
both `config.log_dir`-or-`chat_logs/` then `.join(clean_file_name(…))`
(`:158-162`). So the retrofit drops **both** the `chat_logs/` default **and**
the `clean_file_name` join, taking `agent_chat_log_dir` verbatim (its `None`
disabling the feature — there is no longer a built-in default dir; the `enabled`
set still gates as before). `log_dir` is **set in exactly one place**:
`sl-repl/src/chat_log_args.rs:75` (`log_dir: self.chat_log_dir.clone()`) from
the `--chat-log-dir` CLI arg (`:35`); after removal `ChatLogArgs::to_config`
drops that line and the dir flows via `ClientDirectories.agent_chat_log_dir`
instead. Constructor threading sites: tokio holds
`chat_log_config: ChatLogConfig` (`sl-client-tokio/src/lib.rs:175`, set via
`set_chat_log_config` `:279`) and calls `ChatLog::new` in `run()` (`:314-318`);
bevy's plugin field is `chat_log_config` (`sl-client-bevy/src/lib.rs:142`),
calling `ChatLog::new` in `advance_login()` (`:404-408`); the REPL wires it with
`client.set_chat_log_config(args.chat_log.to_config())`
(`sl-repl-tokio/src/bin/sl-repl-tokio.rs:559`). `ClientDirectories` does **not**
exist yet (grep-confirmed); it is threaded in alongside these sites at parity.
The `Me Resident` subdir is asserted in **both** runtimes (the two chat_log.rs
are identical): tokio at `:634-645` / `:682-704` / `:741-758`
(`dir.join("Me Resident").join(...)`) and the mirrored bevy copy, each seeded by
a helper that passes `log_dir: Some(dir)` (`:619`) — B9 retypes the helper to
the verbatim dir and drops the `Me Resident` join from every assertion in
**both** crates. B10 anchors: there is **no** `flate2`/gzip dependency anywhere
yet, **no** `tokio::fs` usage, and **no** atomic temp+rename pattern — the chat
writer appends synchronously via `fs_err` + `OpenOptions`
(`sl-client-tokio/src/chat_log.rs:41-49`), so the crash-safe gzip write is
wholly new code B10 adds.
