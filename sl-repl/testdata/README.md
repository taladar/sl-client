# sl-repl test inputs

Reusable inputs for exercising `sl-repl-tokio` / `sl-repl-bevy` against a live
grid (the local `opensim.service` test grid, or Second Life aditi).

## Files

- `credentials.example.toml` — credential-file template. Copy it to
  `credentials.toml` (gitignored) and fill in a real password, or pass
  `--credentials <path>`.
- `interactive-probe.repl` — a replayable session script: read-only requests
  plus a chat and a self-IM that exercise the `$self` placeholder. Replay with
  `--script`.
- `record-interactive.py` — a pty driver that drives an interactive session and
  records a transcript, for exercising `--script-out` (which only records in
  TTY mode).

## Local OpenSim verification

Start the grid and build the binaries:

```sh
systemctl --user start opensim.service
cargo build -p sl-repl-tokio -p sl-repl-bevy
```

Make a credentials file with the local test avatar's password (the password is
in the repo's gitignored `.env` as `SL_PASSWORD`):

```sh
cp sl-repl/testdata/credentials.example.toml credentials.toml
# edit credentials.toml: set password = "<SL_PASSWORD from .env>"
```

Then, for each runtime binary (`sl-repl-tokio`, `sl-repl-bevy`):

```sh
BIN=target/debug/sl-repl-tokio

# Smoke battery + a couple of interactive commands (piped stdin replays with
# real sleep pacing; EOF triggers a clean logout).
printf 'sleep 12\nchat "hello"\nim $self "note"\nsleep 3\n' |
  "${BIN}" --credentials credentials.toml --smoke --log-file run.log

# Replay the probe script.
"${BIN}" --credentials credentials.toml \
  --script sl-repl/testdata/interactive-probe.repl --log-file replay.log

# Record an interactive transcript, then replay it (re-logs in).
DELAY_LOGIN=14 python3 sl-repl/testdata/record-interactive.py \
  "${BIN}" --credentials credentials.toml --log-file rec.log --script-out rec.repl
"${BIN}" --credentials credentials.toml --script rec.repl --log-file replay2.log
```

What to check in the trace logs (`--log-file`):

- **Smoke** decodes region/money/economy/parcel/avatar/wearables replies.
- **Placeholders** bind (`binding $self/$session/$circuit/$parcel/$cap:*`) and
  symbolize in rendered events/commands.
- **Diagnostics** surface as `UnhandledMessage` / `UnknownCapsEvent` lines.
- **Record/replay** preserves `$self` verbatim in the `.repl` and resolves it at
  dispatch on replay; `sleep` deltas pace the replay.
- **No secrets**: `grep -ri <password> *.log *.repl` returns nothing.
- **Clean logout**: a `logged_out` line, no `Disconnected`.
- **Two-run diff**: two smoke runs differ only in genuinely volatile server
  data (random terrain/wind, the appearance `serial`, per-session neighbor
  seed-cap UUIDs, the avatar's per-session `local_id`) — not in symbolized ids.

The forced-bad-input `DecodeFailed` (with a marked hexdump) path can't be driven
over a live socket; it is covered by tests instead — `sl-proto`'s
`unknown_message_id_surfaces_decode_failed_with_offset` (a `Session` producing
the diagnostic from a malformed datagram) and `sl-repl`'s
`decode_failed_renders_header_and_marked_hexdump` (the formatter's marked
hexdump rendering of it).
