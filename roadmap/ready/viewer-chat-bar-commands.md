---
id: viewer-chat-bar-commands
title: Chat-bar action commands (`cmd …`)
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs: [viewer-chat-channel-and-commands, viewer-chat-input-bar]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm ships a suite of in-chat text commands (default prefix `cmd`,
each individually toggleable) that perform viewer actions from the chat
bar. Our generic `SlashCommands` registry
([[viewer-chat-channel-and-commands]], done) was built exactly for this
but has almost no registrants. Where a command produces output it writes
a viewer-originated `ChatSource::System` line into nearby chat.

Commands to implement (Firestorm setting in parentheses):

- draw distance set (`FSCmdLineDrawDistance`)
- calculator, echo result to chat (`FSCmdLineCalc`)
- dice roll `cmd [n] [faces]` (`FSCmdLineRollDice`)
- position / ground height / height report (`FSCmdLinePos`,
  `FSCmdLineGround`, `FSCmdLineHeight`)
- teleport home / to camera / within-region tp
  (`FSCmdLineTeleportHome`, `FSCmdTeleportToCam`, `FSCmdLineTP2`)
- map-to region, keep-position variant (`FSCmdLineMapTo`)
- copy camera position (`FSCmdLineCopyCam`)
- rez a temp platform at height, size setting (`FSCmdLineRezPlatform`)
- set music / media URL (`FSCmdLineMusic`, `FSCmdLineMedia`)
- offer tp to an avatar (`FSCmdLineOfferTp`)
- key-to-name lookup (`FSCmdLineKeyToName`)
- clear chat (`FSCmdLineClearChat`)

Reference (Firestorm, read-only): `fscommon.cpp` /
`fslslbridge`-adjacent `cmd` handling, the `FSCmdLine*` settings in
`app_settings/settings.xml`.

Builds on: the slash-command registry and chat input bar (both done);
individual commands lean on teleport, map, and media systems as they
land.
