# Context — SL_REPL_ROAD_MAP.md

Non-task preamble carried over from `SL_REPL_ROAD_MAP.md`. Tasks split out of
that file carry the `repl` topic.

An interactive REPL test client (`sl-repl` lib + `sl-repl-tokio` /
`sl-repl-bevy` bins) for shaking the SL client out against a live grid
(aditi), plus the wire-level diagnostics it needs. Work these top-to-bottom;
tick a box only when the step builds, is clippy-clean (restriction lints), and
`cargo test` passes. Add sub-tasks as you discover them. Detailed design lives
in the planning doc this was generated from and is re-derivable from the code.

Scope reminders:

- Commit on the current branch only (never auto-create a feature branch).
- Keep `sl-client-tokio` and `sl-client-bevy` at feature parity (land mirrored
  steps together).
- Never push client-only protocol types into the shared `sl-types` crate.
- Secrets (password, MFA token) must never reach any log, transcript, or HTTP
  body that gets logged.
