---
id: repl-a2
title: Diagnostic type + decode/CAPS surfacing
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase A — core diagnostics (sl-wire / sl-proto), no REPL yet
---

Context: [context/repl.md](../context/repl.md).

**A2. `Diagnostic` type + decode/CAPS surfacing.** New
`sl-proto/src/types/diagnostic.rs` enum
(`DecodeFailed{id,name,error,raw,failed_offset}`, `UnhandledMessage`,
`UnknownCapsEvent`, `CapsDecodeFailed`, `ExpectedReplyMissing`) — **separate
from `Event`** (a match on `Event` must never see diagnostics). Add
`set_diagnostics(bool)` (default off), a diagnostic `VecDeque`, and
`poll_diagnostic()`. At the silent sites (`handle_datagram` drop `:725`,
`dispatch` catch-all arms `:~775`, `handle_caps_event`
unknown/`from_llsd`-None `:~279`,`:389`) emit the matching diagnostic (capture
raw bytes + `failed_offset` from `Reader::position()`) and add `tracing`
(`trace!` per
inbound message, `warn!` on failures). Gate raw-byte capture on the flag. Unit
test: malformed/short datagram + unknown id → `DecodeFailed` with offset; flag
off → nothing emitted, no clone.
