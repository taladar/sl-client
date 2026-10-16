---
id: protocol-classified-query-wrong-flag-space
title: DirClassifiedQuery sends DirFind maturity bits instead of classified-query bits
topic: protocol
status: done
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns, viewer-search-floater]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`Command::DirClassifiedQuery { flags: DirFindFlags }`
(`sl-proto/src/command.rs`) is sent as `flags.bits()`, and the search floater
fills it with
`DirFindFlags::INC_PG / INC_MATURE / INC_ADULT` (bits 24-26,
`sl-viewer-search/src/search.rs` `maturity_flags`). The reference packs
`QueryFlags` for this message with `pack_classified_flags_request`
(`fsfloatersearch.cpp` ~2255) from `llclassifiedflags.h`:
`CLASSIFIED_QUERY_INC_PG = 1 << 2`, `INC_MATURE = 1 << 3`,
`INC_ADULT = 1 << 6` (and an auto-renew bit). `DirFindFlags`' doc claims it is
shared by every `Dir*Query`, which is wrong for this one.

## Effect

The server never sees the maturity bits it reads: classified search filters
wrongly or returns nothing, silently.

## Fix

A separate `ClassifiedQueryFlags` type for this command (and its server-side
decode in `SimSession`), mapped from the same maturity checkboxes. Live-check a
classified search on aditi.

## Done (2026-10-16, in [[gridspec-search-directory]])

`ClassifiedQueryFlags` (`sl-proto/src/types/directory.rs`) is the
classifieds' own bit space, and `Command::DirClassifiedQuery` and
`ServerEvent::DirClassifiedQuery` carry it; `for_ratings` packs the rating
checkboxes as `pack_classified_flags_request` does (General without Moderate
also sets `CLASSIFIED_QUERY_FILTER_MATURE`). The search floater sends it from
its classifieds checkboxes. The live check ran on aditi, but aditi lists no
classifieds: both bit spaces were answered by the same empty reply (status
`FOUNDNONE`), so the fix rests on the reference
(`book/src/gridspec/search.md`, *Classifieds*).
