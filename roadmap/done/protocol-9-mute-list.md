---
id: protocol-9
title: Mute list
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**9. Mute list · 2 pts. ✅ Done.** A small moderation helper that fetches and
edits the mute/block list. Implemented: `Session::request_mute_list`
(`MuteListRequest`, zero CRC), `mute` (`UpdateMuteListEntry`) and `unmute`
(`RemoveMuteListEntry`), with `MuteType` (by-name/agent/object/group/external)
and a `MuteFlags` exception bitfield. The **fetch** is the real thing: the sim
replies with `UseCachedMuteList` (→ `Event::MuteListUnchanged`), a
`GenericMessage` `emptymutelist` (→ `Event::MuteList([])`), or a
`MuteListUpdate` naming a file the client then
**downloads over the legacy `Xfer` file-transfer path** (`RequestXfer` →
`SendXferPacket`/`ConfirmXferPacket`, stripping packet-0's 4-byte length prefix
and detecting the `0x80000000` last-packet flag), parsing the
`<type> <uuid> <name>|<flags>` lines into `MuteEntry` values
(`Event::MuteList`). The `Xfer` machinery (session state for in-flight
transfers) is reusable for #19's legacy asset path. Wired as
`Command::{RequestMuteList, Mute, Unmute}` through both runtimes. Verified live
against the local OpenSim (MuteList module + SQLite MuteListService enabled):
muting an agent then fetching returned the parsed entry over Xfer, and unmuting
then fetching returned an empty list. *Test: local OpenSim with `[Messaging]
MuteListModule = MuteListModule` and a `MuteListService` (SQLite) configured.*
