# sl-repl-bevy

An interactive, Bevy-driven Second Life / OpenSim REPL test client — the ECS
sibling of [`sl-repl-tokio`](../sl-repl-tokio). It logs in as an avatar from a
TOML credentials file, drives a live [`sl-client-bevy`](../sl-client-bevy)
session through `SlClientPlugin`, and lets you type grid commands at a prompt —
every command, event, and wire diagnostic is recorded to an always-on trace log
so a session can be inspected or replayed afterwards.

It is built on the [`sl-repl`](../sl-repl) shared library: `sl-repl` supplies
the command grammar/parser, the placeholder context, the symbolizing formatters,
the read-only smoke battery, the script recorder, and the TOML credentials + MFA
handling; this crate is the thin Bevy binary that wires those to the plugin's
ECS event streams. It is feature-for-feature identical to `sl-repl-tokio`; the
only difference is the runtime underneath.

## Running

```sh
sl-repl-bevy --credentials creds.toml --avatar alice --grid aditi
```

With no subcommand the binary opens a REPL session. The two subcommands
`generate-manpage` and `generate-shell-completion` emit packaging artifacts and
exit.

Key options (identical to `sl-repl-tokio`):

- `--credentials <path>` — the TOML credentials file (see below).
- `--avatar <name>` — which avatar in the file to log in as (defaults to the
  file's `default_avatar`, or the sole avatar).
- `--grid <name>` / `--login-uri <url>` — the grid to log in to; `--login-uri`
  wins, then `--grid` (`agni`/`aditi`/`localhost`), then the avatar's own
  `login_uri`/`grid`, then the local OpenSim default.
- `--start <last|home|uri:Region&x&y&z>`, `--channel`, `--version` — login
  parameters.
- `--smoke` — fire the read-only [smoke battery](../sl-repl) once the region
  handshake lands (a safe end-to-end check of login, the circuit, the CAPS seed,
  and the decoders).
- `--script <path>` — replay a `.repl` script instead of reading interactively
  (honouring its `sleep` directives). A non-terminal stdin is replayed the same
  way.
- `--log-file <path>` — the always-on trace log (default `sl-repl-bevy.log`).
- `--script-out <path>` — record the interactive session to a replayable
  `.repl` transcript.

## Logging

Two layers are always on:

- a **file** layer at `trace` level capturing the full record — symbolized
  events and dispatched commands, placeholder binding lines, literal diagnostics
  (including marked hexdumps of decode failures), and the underlying wire trace;
- a **terminal** layer at `info` level (override with `RUST_LOG`) whose writer
  wraps rustyline's external printer so log output never corrupts the prompt.

Secrets — the password and any acquired MFA token — are never logged, and the
XML-RPC login body is never body-logged.

## Credentials

```toml
default_avatar = "alice"

[avatars.alice]
first = "Alice"
last = "Resident"
password = "hunter2"
grid = "aditi"
mfa_command = "oathtool --totp -b ABCDEF234567"
mfa_window_guard_secs = 5
```

When the grid issues a multi-factor challenge, the avatar's `mfa_command` is run
to obtain a one-time token, and the Bevy app is re-run with the token folded
into the login request. The plugin's Startup login is one-shot, so MFA is
handled by the binary restarting the app once per challenge — the persistent
line editor and log survive the restart. Acquisition waits out the tail of the
current wall-clock-aligned 30-second TOTP window when fewer than
`mfa_window_guard_secs` of it remain, so the submitted token survives the login
round-trip. An avatar with no `mfa_command` cannot answer an MFA challenge.
