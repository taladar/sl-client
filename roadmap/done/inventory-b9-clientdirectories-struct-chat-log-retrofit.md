---
id: inventory-b9
title: ClientDirectories struct + chat-log retrofit
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B9. `ClientDirectories` struct + chat-log retrofit (from A4)

- [x] Add `ClientDirectories` (three `Option<PathBuf>` fields —
      `agent_cache_dir`, `agent_chat_log_dir`, `shared_cache_dir`)
      in `sl-proto` (next to `ChatLogConfig`, `chat_log.rs`); thread it through
      the constructor sites at parity — tokio gains a `directories` field +
      `set_directories` beside `chat_log_config` / `set_chat_log_config`, bevy's
      plugin + `SlConfig` gain a `directories` field beside `chat_log_config`,
      and both REPL binaries build the struct from `--chat-log-dir` (a new
      `ChatLogArgs::chat_log_dir()` accessor) and wire it through.
- [x] Retrofit **both** `ChatLog::new` shells (the byte-identical tokio +
      bevy `chat_log.rs`) to take `agent_chat_log_dir` **verbatim** as a param,
      dropping **both** the `chat_logs/` default and the
      `.join(clean_file_name(own_name))`; `base_dir` is now `Option<PathBuf>`
      (`None` makes `any_enabled()` false + short-circuits every write). The dir
      flows from `ClientDirectories.agent_chat_log_dir` at the call sites (tokio
      `run()`, bevy `advance_login()`). Removed the now-redundant
      `ChatLogConfig.log_dir` field and the line setting it in
      `ChatLogArgs::to_config`.
- [x] Updated the `Me Resident` subdir tests in **both** runtimes (the two
  chat_log.rs are identical): the `im_config` helper drops its dir arg, every
  `ChatLog::new` call passes the dir verbatim, and every assertion targets files
  directly under the supplied dir (no `Me Resident` join).
