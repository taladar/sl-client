---
id: server-world-chat-routing
title: The fake grid hears local chat and drops it
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
refs: [server-lsl-lib-comms, test-fake-grid-lsl-offline-cases]
---

Context: [context/lsl.md](../context/lsl.md).

`SimSession` decodes a client `ChatFromViewer` into
`ServerEvent::Chat { message, channel, chat_type }`, and `sl-fake-grid`
handles no such event — the grep for `ServerEvent::` across the crate
lists 60-odd variants and `Chat` is not one of them. So an avatar on the
fake grid can say something and nothing happens: no echo to itself, no
delivery to the other avatar standing next to it, and nowhere for a
script's `llListen` to hear it.

This is a prerequisite for half the library's observable behaviour
(`llSay`, `llListen`, `llDialog`'s reply channel, `llOwnerSay`,
`DEBUG_CHANNEL`) and it is worth doing on its own merits: three
conformance cases — `chat-self-echo`, `chat-hear-other`,
`chat-whisper-shout-range` — are live-grid-only today purely for want of
it.

Wanted, in `sl-fake-grid`:

- **Channel 0 routing by range**, at the reference's distances:
  whisper 10 m, say 20 m, shout 100 m, measured from the speaker to each
  listener in the region. The speaker hears itself (the viewer relies on
  the echo, not on local display). `ChatType::Normal` / `Whisper` /
  `Shout` / `OwnerSay` / `RegionSay` each get their rule, and
  `RegionSay`'s rule is "the whole region, no distance".
- **Non-zero channels go to scripts only.** No avatar hears channel 7 or
  the negative channel a `llDialog` reply comes back on; only a
  registered listen does. That distinction is the whole point of the
  hidden channel, and a grid that echoed every channel to every viewer
  would look fine in a one-avatar test and break every dialog.
- **A listen registry** keyed by (script instance, channel, name, key,
  message filter), which is what `llListen` populates and
  `llListenRemove` / `llListenControl` mutate. It lives with the region,
  not with a session, because the script does. Scripts do not exist yet
  — land the registry with a trivial in-crate consumer and let
  [[server-lsl-lib-comms]] fill it.
- **Chat *from* the region**: a single "say this, from this source, at
  this position, on this channel" entry point that fans out to the
  sessions in range as `ChatFromSimulator` and to the listen registry,
  so an object's `llSay` and an avatar's typed line take the same path.
  `SimSession::send_chat_from_simulator` already exists; what is missing
  is the fan-out above it.
- **Distance is the avatar's real position**, which the fake grid does
  not track yet ([[server-world-agent-movement]]) — until it does, the
  arrival position is the honest stand-in, and the range test should be
  written against a position getter so it becomes correct for free.

Acceptance: `chat-self-echo`, `chat-hear-other` and
`chat-whisper-shout-range` move into `fake::OFFLINE_CASES` and pass; a
unit test shows a whisper at 15 m unheard and a shout at 15 m heard; and
a message on channel 7 reaches no session.
