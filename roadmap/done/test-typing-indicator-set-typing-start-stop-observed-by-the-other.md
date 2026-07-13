---
id: test-typing-indicator
title: set_typing start/stop observed by the other
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 2 — Local chat `[both]`
---

Context: [context/test.md](../context/test.md).

`typing-indicator` — `set_typing` start/stop observed by the other. `2av`
(OpenSim now; Aditi deferred → Phase Z). The local-chat typing indicator is a
`ChatFromViewer` with no text and a `StartTyping`/`StopTyping` chat type (the
animation trigger a viewer fires while editing the chat bar); the simulator
broadcasts it to nearby avatars, surfaced as `Event::ChatTyping`. The
secondary (`avatar2`) sends `Command::Typing(true)` then
`Command::Typing(false)`, and the primary — a separate session sharing the
region — observes `typing: true` then `typing: false`, both attributed to the
secondary's agent id. Where `chat-hear-other` proves the simulator relays a
spoken *message*, this proves it relays the typing *signal*. Unlike a spoken
`say` (gated by say/whisper/shout distance), OpenSim delivers typing with no
distance check, so the relay does not depend on how close the avatars logged
in. Green on OpenSim; start RTT ≈ 7 ms, stop RTT ≈ 1 ms on loopback.
