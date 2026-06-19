# sl-repl

Shared library for an interactive Second Life / OpenSim REPL test client.

`sl-repl` turns a single line of text into a [`sl_proto::Command`] (or a
REPL meta-action) and back, so the `sl-repl-tokio` and `sl-repl-bevy`
binaries can drive a live `sl-proto` session from a console, a script, or a
recorded transcript while staying at feature parity.

This crate is largely sans-I/O: it owns the command grammar, the line parser,
and the command registry (one entry per `Command` variant). The runtime crates
own the session, the socket, and the terminal. The one exception is the `auth`
module, which loads TOML credentials, redacts secrets, and acquires
wall-clock-aligned MFA (TOTP) tokens by running a configured shell command — a
small, synchronous slice of I/O the binaries invoke once before login.

See `SL_REPL_ROAD_MAP.md` in the workspace root for the phased plan.
