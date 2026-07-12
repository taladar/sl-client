---
id: protocol-1
title: Local chat
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**1. Local chat — `ChatFromViewer` (send), `ChatFromSimulator` (receive) · 3
pts. ✅ Done.** Smallest step to a genuinely interactive client: a text-only
viewer or chat bot. Implemented: `Session::say` (whisper/normal/shout, any
channel) and `Session::set_typing`, with `Event::ChatReceived` (speaker, ids,
type, audibility, region-local position, text) and a distinct
`Event::ChatTyping` for typing start/stop. Wired as
`Command::Chat`/`Command::Typing` through both the tokio and bevy runtimes;
verified live against the local OpenSim (full send→rebroadcast→receive
round-trip). *Test: local OpenSim.*
