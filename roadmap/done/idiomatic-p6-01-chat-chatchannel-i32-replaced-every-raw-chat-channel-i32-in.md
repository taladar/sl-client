---
id: idiomatic-p6-01
title: chat::ChatChannel(i32) — replaced every raw chat-channel i32 in the ty
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Already in use: `sl_types::{lsl::Vector, lsl::Rotation, money::LindenAmount,
attachment::*}`. Adopt these more, selectively by semantic role:

`chat::ChatChannel(i32)` — replaced every raw chat-channel `i32` in
    the typed layer with `sl_types::chat::ChatChannel`, wrapping at the codec
    boundary only (decode `ChatChannel(raw)`, encode `.0`) so wire bytes are
    byte-identical. NO sl-types change — `ChatChannel` already carries
    `Copy`/`Eq`/`Ord`/`Display`/`FromStr` (consumed via the existing path dep,
    no version bump). Converted fields: `Command::Chat.channel`,
    `Command::ReplyScriptDialog.chat_channel`, `ScriptDialog.chat_channel`
    (`types/script.rs`), `ServerEvent::Chat.channel` (`sim_session.rs`), and
    the matching codec/method params (`Session::say` +
    `send_chat_from_viewer`, `Session::reply_script_dialog` +
    `send_script_dialog_reply`, the `script_dialog` conversion decode, the
    `set_typing` channel-`0` call). The sl-wire *generated* message blocks
    (`ChatFromViewerChatDataBlock.channel`,
    `ScriptDialogDataBlock`/`ScriptDialogReplyDataBlock.chat_channel`) stay
    raw `i32` (the wire representation). Left raw (not chat channels):
    `LoginRequest.channel` (the viewer-version string),
    `ReplyScriptDialog.button_index`, voice-channel fields. Re-exported
    `ChatChannel` through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`
    (parity; the runtimes forward the typed `Command` field verbatim, no
    signature change). REPL `chat`/`reply_script_dialog` parse the raw `i32`
    then wrap. Book `content/chat.md` updated. +1 focused unit test
    (`chat_channel_round_trips_raw_i32`, incl. the negative hidden channels
    and `i32::MIN`/`MAX`); lifecycle + `sim_session` round-trip suites
    updated.
    Build + clippy (`--workspace --all-targets`) + tests + `cargo doc`
    (`-D warnings`) + mdbook green. NO sl-types touched.
