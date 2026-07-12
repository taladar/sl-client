---
id: idiomatic-p7-07
title: New WireError::{InvalidUuid, MalformedField} + strict parse_uuid_field
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

New `WireError::{InvalidUuid, MalformedField}` + strict
    `parse_uuid_field`/`parse_optional_uuid_field`/`parse_u32_field` helpers.
    Hardened the user-cited masking sites: `estate_info_from_params` (→
    `Result<Option<EstateInfo>, WireError>`; a malformed owner/id/flags/sun/
    parent/covenant-timestamp / a non-empty invalid covenant id rejects the
    whole `EstateInfo`; `EstateInfo`+`EstateCovenant` `covenant_id` became
    `Option<Uuid>` — the B half); `parse_mute_list`/`parse_mute_line` (→
    fallible; a bad UUID/flags line is a hard error, blank still `Ok(None)`);
    `parse_uuid_string` (EventInfoReply `Creator`); `inventory_offer_bucket`
    (rejects an out-of-byte- range asset code instead of writing `0`). +2 unit
    tests. (commit "Phase 7 C part 1")
