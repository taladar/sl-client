---
id: idiomatic-p2-03
title: LoginRequest.start (sl-wire/src/login.rs):
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 2 — Constructor invariants (low invasiveness, caller-facing)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`LoginRequest.start` (`sl-wire/src/login.rs`): a `String` constrained to
`"last" | "home" | "uri:Region&x&y&z"`. Introduced a public `StartLocation`
enum (`Last`/`Home`/`Region { region: String, position: [f32; 3] }`) with a
parse-don't-validate `FromStr` (rejecting out-of-grammar values with a public
`StartLocationParseError` — `Unrecognized`/`MalformedUri`), a
`to_wire_string()` inverse, and a `StartLocation::region` constructor. The
`uri:` parser splits the three trailing `&`-coordinates off the right so a
legal region name survives, and the renderer formats floats exactly as
Firestorm's `construct_start_string` does (`128.0` → `128`), so wire bytes are
byte-identical. `LoginRequest.start` is now `StartLocation`; `LoginRequest`,
`LoginParams`, and `ParsedLoginRequest` drop their `Eq` derive (the float
position breaks `Eq`, matching `LoginSuccess`/`HomeLocation`). The server-side
`ParsedLoginRequest.start` is `Result<StartLocation, String>` — parsed into a
typed location when the (untrusted) client value matches the grammar,
otherwise the raw string is preserved verbatim (`Err`), so nothing is lost and
a malformed `start` can't masquerade as a valid location. Re-exported through
`sl-proto`/`sl-client-tokio`/`sl-client-bevy`; REPL/survey CLIs parse the
`--start` arg straight into `StartLocation` via clap, examples `.parse()?`
the `SL_START` env, and the survey relog builds `StartLocation::region`. +4
unit tests (three wire forms, wire round-trip, `&`-in-region-name,
out-of-grammar rejection). **Phase 2 COMPLETE.**
