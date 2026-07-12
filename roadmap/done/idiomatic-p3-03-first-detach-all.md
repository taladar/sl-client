---
id: idiomatic-p3-03
title: first_detach_all:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 3 — Intent enums replacing bool / magic-int params (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`first_detach_all: bool` → `DetachOrder` (`command.rs:1531`,
`sim_session.rs:315`). New public `DetachOrder` enum
(`DetachAllFirst`/`Keep`, `detaches_all_first`/`from_first_detach_all`) in
`sl-proto/src/types/appearance.rs` (next to `AttachmentMode`) replaces the
`first_detach_all: bool` on both `Command::RezAttachments` and
`ServerEvent::RezAttachments` (field renamed `first_detach_all` → `detach`).
`Session::rez_attachments` and the `send_rez_multiple_attachments` codec take
`DetachOrder`; the codec wraps at the boundary (`detach.detaches_all_first()`
on encode, `DetachOrder::from_first_detach_all(..)` on decode) so the
`RezMultipleAttachmentsFromInv` `FirstDetachAll` wire bool is byte-identical.
Re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (both
runtimes updated at parity). REPL gains `parse_detach_order` (accepts
`detach`/`keep` plus the legacy `true`/`false` boolean spelling);
`rez_attachments` usage is now `[detach=detach|keep]`. Book
`content/attachments.md` updated. +1 unit test (mode↔first-detach-all-flag
mapping + round-trip) and the lifecycle + `sim_session` round-trip suites
updated. NO sl-types touched (a client wire-protocol concept).
