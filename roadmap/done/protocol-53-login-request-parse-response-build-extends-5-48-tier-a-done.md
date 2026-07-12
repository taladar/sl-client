---
id: protocol-53
title: Login request parse / response build (extends #5/#48, Tier A). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**53. Login request parse / response build (extends #5/#48, Tier A). ✅ Done.**
`sl-wire/src/login.rs` had `build_login_request` + `parse_login_response` (the
client direction). Added the server direction. **`parse_login_request`** → a new
`ParsedLoginRequest` (the same fields as `LoginRequest`, but with the
already-hashed `passwd` the server actually receives — never the plaintext — and
the `agree_to_tos`/`read_critical`/`extended_errors` acknowledgement flags
surfaced), reusing the existing `collect_members`/`member_value_node` machinery
plus a new `array_strings` for the `options` list. **`build_login_response`** is
the element-by-element inverse of `parse_login_response`: it emits the
`<methodResponse>` struct (`login` plus, on success, the ids / sim placement /
seed cap and every optional inventory-root/skeleton, buddy-list, quasi-LLSD
`home`+`look_at` with `r`-prefixed reals, access, max-agent-groups, and library
field — written only when present, so the output re-parses to an equal value),
or a failure's `reason`/`message`, or an `mfa_challenge`. The login endpoint is
XML-RPC, so `build_login_response` mirrors `build_login_request`'s XML-RPC
helpers rather than #52's LLSD-XML serializer (#52 is reused by the LLSD-side
producers #59/#61–#64). Plus a small **`LoginServer`** helper (with `Credential`
and `MfaPolicy`) whose `respond(request, credential, success)` maps a parsed
request and supplied account/sim facts to the `LoginResponse` to return:
`Success` when the hashed password matches and any MFA policy is satisfied (by a
matching one-time `token` or an echoed remembered `mfa_hash`); an `MfaChallenge`
(handing out the policy's hash + message) when MFA is required but unmet; or a
`Failure` (reason `"key"`) on a password mismatch. Covered by five
`sl-wire/tests/login.rs` round-trip tests: `parse_login_request` of
`build_login_request` (incl. the hashed password, flags, and options); a full
success, a failure, and an MFA challenge through `build_login_response` →
`parse_login_response`; and the `LoginServer` decision matrix (good/bad
password, MFA challenge, MFA satisfied by token and by remembered hash). *Test:
unit round-trip (no grid).*
