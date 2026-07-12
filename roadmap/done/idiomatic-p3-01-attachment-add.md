---
id: idiomatic-p3-01
title: attachment add:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 3 — Intent enums replacing bool / magic-int params (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Replace ambiguous `bool`s and magic ints with named enums.

attachment `add: bool` → `AttachmentMode { Add, Replace }`
(`command.rs:1492`, `sim_session.rs:291`). Did the maximal version: a new
public `AttachmentMode` enum in `sl-proto/src/types/appearance.rs`
(`Add`/`Replace`, `is_add`/`from_add_flag`) replaces the `add: bool` flag on
**every** attachment carrier — `Command::AttachObject`,
`ServerEvent::AttachObject`, and `RezAttachment` (the field renamed `add` →
`mode`) — plus the `AttachmentPoint` helpers (`with_add(bool)` →
`with_mode(AttachmentMode)`, `split_code` now returns
`(AttachmentPoint, AttachmentMode)`). `Session::attach_object` /
`send_object_attach` take `AttachmentMode`; codec wraps at the boundary
(`with_mode`/`split_code`) so the wire byte (`ATTACHMENT_ADD` `0x80`) is
byte-identical. Re-exported through `sl-proto`/`sl-client-tokio`/
`sl-client-bevy`; both runtimes updated at parity. REPL gains a
`parse_attachment_mode` (accepts `add`/`replace` plus the legacy
`true`/`false` boolean spelling); `attach_object`/`rez_attachment` take
`mode=add|replace`, `rez_attachments` records take `[:add|replace]`. Book
`content/attachments.md` updated. +2 unit tests (mode↔add-flag mapping,
`with_mode`/`split_code` bit-identical round-trip) and the lifecycle +
`sim_session` round-trip suites updated. NO sl-types touched (a client
wire-protocol concept).
