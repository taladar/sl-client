---
id: repl-e2
title: Local OpenSim verification (both runtimes)
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase E — docs & live verification
---

Context: [context/repl.md](../context/repl.md).

**E2. Local OpenSim verification (both runtimes).** Verified both
`sl-repl-tokio` and `sl-repl-bevy` against local `opensim.service`: smoke
battery
decoded (region/money/economy/parcel/avatar/wearables); interactive `chat` +
`im $self` dispatched with `$self` resolved at dispatch and symbolized in the
echoes; record/replay (`record-interactive.py` pty driver →
`--script-out`; `$self` preserved verbatim in the `.repl`; replay re-logs in
with a fresh `$circuit`/`$session` and honours `sleep` pacing) on both
runtimes; diagnostics surfaced (`UnhandledMessage`, `UnknownCapsEvent`);
secret-leak grep of every log + transcript = **zero** matches; two-run
symbolized-log clean-diff (differences only in genuinely volatile server data
— random terrain/wind, the appearance `serial`, per-session neighbor seed-cap
UUIDs, the avatar's per-session `local_id` — never in symbolized ids). The
forced-bad-input `DecodeFailed`-with-marked-hexdump path can't be driven over
a live socket (no way to spoof the sim's source port), so it is covered by
tests: `sl-proto`'s `unknown_message_id_surfaces_decode_failed_with_offset` (a
`Session` produces the diagnostic from a malformed datagram) plus a NEW
end-to-end `sl-repl` test `decode_failed_renders_header_and_marked_hexdump`
(the formatter renders the header + bracketed `[aa]` hexdump for the exact
bytes a live `Session` captures). Closing the latter's coverage gap required
re-exporting `MessageId` from `sl-proto` (a `Diagnostic::DecodeFailed` public
field whose type a consumer previously couldn't name). Reusable inputs saved
under `sl-repl/testdata/` (`credentials.example.toml`,
`interactive-probe.repl`, `record-interactive.py`, `README.md`);
`credentials.toml` + the default log
files added to `.gitignore`. Known cosmetic (pre-existing, out of scope): the
C3 token-scanner over-matches bare integer runs (`dilation: $parcel.0` for
`1.0`).
