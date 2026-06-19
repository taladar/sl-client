# The REPL test client (`sl-repl`)

Most of this book describes the protocol and the library crates that implement
it. This chapter describes a *tool* built on top of them: an interactive
**REPL test client** for driving a live grid by hand, scripting it, and
capturing every byte that goes wrong for later analysis.

It exists to shake the client out against a real grid (the local
[OpenSim](../introduction.md) test region, or Second Life's `aditi` beta grid)
without writing a throwaway program each time: you log in as a configured
avatar, type [commands](../content/index.md) at a prompt, watch the
[events](../comms/sessions.md) and [diagnostics](#wire-diagnostics) come back
symbolized for clean reading, and replay the whole thing afterwards.

## The three crates

The tool is split so the two runtime backends stay at feature parity:

- **`sl-repl`** — the sans-I/O shared library. It owns the command grammar, the
  line parser, the command registry (one build entry per `Command` variant), the
  placeholder context, the symbolizing formatters, the read-only smoke battery,
  the script recorder, and the TOML credentials + MFA handling. It does no
  networking of its own (the one slice of I/O is running the MFA command).
- **`sl-repl-tokio`** — the thin binary that wires `sl-repl` to a real
  [`sl-client-tokio`](../architecture.md) session and socket.
- **`sl-repl-bevy`** — the ECS sibling that wires the same library to a
  [`sl-client-bevy`](../architecture.md) `SlClientPlugin` session. It is
  feature-for-feature identical to `sl-repl-tokio`; only the runtime underneath
  differs.

## Running

Both binaries take the same options. With no subcommand they open a REPL
session; the `generate-manpage` and `generate-shell-completion` subcommands emit
packaging artifacts and exit.

```sh
sl-repl-tokio --credentials creds.toml --avatar alice --grid aditi
sl-repl-bevy --credentials creds.toml --avatar alice --grid aditi
```

Key options:

- `--credentials <path>` — the TOML [credentials file](#credentials-toml--mfa).
- `--avatar <name>` — which avatar in the file to log in as (defaults to the
  file's `default_avatar`, or the sole avatar).
- `--grid <name>` / `--login-uri <url>` — the grid to log in to. `--login-uri`
  wins, then `--grid` (`agni`/`aditi`/`localhost`), then the avatar's own
  `login_uri`/`grid`, then the local OpenSim default.
- `--start <last|home|uri:Region&x&y&z>`, `--channel`, `--version` — login
  parameters.
- `--smoke` — fire the read-only [smoke battery](#smoke-mode) once the region
  handshake lands, then continue interactively.
- `--script <path>` — replay a `.repl` script instead of reading interactively,
  honouring its `sleep` directives. A non-terminal stdin is replayed the same
  way.
- `--log-file <path>` — the always-on trace log (default `sl-repl-tokio.log` /
  `sl-repl-bevy.log`).
- `--script-out <path>` — record the interactive session to a replayable
  `.repl` transcript.

## The command grammar

Every non-meta line names a grid `Command` followed by its arguments. There is
one registry entry per `Command` variant, named with the registry spelling
(`im`, not `InstantMessage`), so the command vocabulary is exactly the set of
things the client can send.

- **Positional and keyword tokens** — arguments are either positional or
  `key=value`. Strings that contain spaces are double-quoted.
- **Vectors and rotations** — `<x,y,z>` for an `LLVector3`, `<x,y,z,s>` for a
  quaternion.
- **Byte blobs** — written as hex.
- **Lists and records** — comma-separated, with `:`-separated fields inside each
  record.
- **Enums** — every enum argument accepts a lowercase name (underscores
  optional) *or* its numeric wire code.

All but one of the `Command` variants can be expressed as a line. The exception
is `send` (an arbitrary boxed message), which cannot be written as text and
instead points you at the specific typed command for what you want to do.

### Placeholders

Volatile session ids would make every transcript different and every command
tedious to type, so the REPL resolves `$placeholder` tokens to literal values at
**dispatch time**, against the live session context:

| Placeholder | Stands for |
|-------------|------------|
| `$self`     | the agent's own id (once login completes) |
| `$session`  | the session id |
| `$circuit`  | the circuit code |
| `$region`   | the current region's handle |
| `$parcel`   | the region-local id of the most recently seen parcel |
| `$lastobj`  | the id of the most recently seen object |
| `$cap:Name` | the URL of capability `Name` from the seed map |
| `$var`      | a user variable set with `set` |

The same context runs in reverse as a **symbolizer**: when an event or command
is printed, any literal that matches a binding is rewritten back to its
`$placeholder`. That is what makes two runs of the same script diff cleanly —
the volatile ids are gone from the output. Each time a binding changes, an
`info`-level line records the new value.

### Meta commands

A handful of lines act on the REPL itself rather than the grid, recognised by
their leading token:

- `# comment` — a comment; preserved verbatim in a recorded transcript.
- `sleep <seconds>` — a pause, used to pace script replay.
- `set <name> <value>` — bind a `$name` user variable (the value is the rest of
  the line, surrounding quotes stripped).
- `unset <name>` — remove a user variable.
- `vars` — list the currently bound user variables.

## Smoke mode

`--smoke` fires a fixed battery of **read-only** requests once the region
handshake completes — a safe end-to-end check that login, the circuit, the CAPS
seed, and the decoders all work against a given grid. It touches nothing in the
world, so it is safe to run on any account. After the battery the session stays
open for interactive use.

## Wire diagnostics

The REPL turns on the [`Diagnostic`](../comms/sessions.md) surface (see
[CAPS](../comms/caps.md) and [Messages](../comms/messages.md)) so that anything
the client would otherwise silently drop is surfaced and logged:

- a message that fails to decode (`DecodeFailed`) is printed as a **marked
  hexdump** that brackets the byte at the failure offset;
- a message with no handler (`UnhandledMessage`), an unknown event-queue event
  (`UnknownCapsEvent` / `CapsDecodeFailed`), and a reliable packet that never
  got its reply (`ExpectedReplyMissing`) are each logged literally.

Diagnostics are rendered *without* symbolization — they are about raw bytes, so
they are shown raw.

## Logging and recording

Two logging layers are **always on**:

- a **file** layer at `trace` level capturing the full record — symbolized
  events and dispatched commands, the placeholder binding lines, the literal
  diagnostics (including the decode-failure hexdumps), and the underlying wire
  trace;
- a **terminal** layer at `info` level (override with `RUST_LOG`) whose writer
  wraps rustyline's external printer, so log output never corrupts the prompt.

`--script-out` additionally records the interactive session to a replayable
`.repl` transcript: each typed line, `sleep` deltas between them, placeholders
preserved verbatim, and any parse failure kept as a `# ERROR` comment so the
transcript still round-trips.

**Secrets never leak.** The password and any acquired MFA token are never
logged, and the XML-RPC login body is never body-logged.

## Credentials TOML & MFA

Credentials live in a TOML file describing one or more avatars:

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

When the grid answers [login](../content/login.md) with a multi-factor
challenge, the avatar's `mfa_command` is run to obtain a one-time token. TOTP
tokens are valid only within a wall-clock-aligned 30-second window, so
acquisition **waits out the tail** of the current window when fewer than
`mfa_window_guard_secs` of it remain — otherwise a token generated at second 29
would expire mid-flight and the re-submitted login would be rejected. An avatar
with no `mfa_command` cannot answer an MFA challenge.

The two runtimes reach the same outcome by different routes. `sl-repl-tokio`
re-submits the login inline. `sl-repl-bevy` cannot — the plugin's Startup login
is one-shot — so the binary re-runs the Bevy app once per challenge with the
token folded in; the persistent line editor and the log survive the restart.

---

> **In this codebase**
>
> - The shared library is the `sl-repl` crate: `parse.rs` (`parse_line` →
>   `ReplAction`), `args.rs` (the tokenizer + typed accessors), `meta.rs`
>   (`MetaCommand`), `registry.rs` (one build entry per `Command`), `context.rs`
>   (`ReplContext` / `SessionContext`, placeholder resolution + symbolizer),
>   `format.rs` (`format_event` / `format_command` / `format_diagnostic` /
>   `hexdump`), `smoke.rs` (`smoke_battery`), `record.rs` (`ScriptRecorder`),
>   and `auth.rs` (`Credentials`, the redacting `Secret` newtype,
>   `acquire_mfa_token` with the window-aligned wait).
> - The binaries are `sl-repl-tokio` and `sl-repl-bevy`. Each seeds
>   `$self`/`$session`/`$circuit`/`$cap:*` from identity facts the `Event`
>   stream does not carry: the tokio `Client` exposes `session_id` /
>   `circuit_code` / `seed_capability` accessors and a `set_caps_reporter`
>   cap-map stream; the Bevy plugin emits an additive `SlIdentity` event and an
>   `SlCapabilities` event for parity.
> - The diagnostics surface (`set_diagnostics`, `poll_diagnostic`, the
>   `Diagnostic` enum) lives in `sl-proto`; the runtimes stream it as a
>   `Diagnostic` mpsc channel (tokio) and an `SlDiagnostic` event (Bevy). See
>   [Sessions](../comms/sessions.md).
