# sl-repl

Shared library for an interactive Second Life / OpenSim REPL test client.

`sl-repl` turns a single line of text into a [`sl_proto::Command`] (or a
REPL meta-action) and back, so the `sl-repl-tokio` and `sl-repl-bevy`
binaries can drive a live `sl-proto` session from a console, a script, or a
recorded transcript while staying at feature parity.

This crate is sans-I/O: it owns the command grammar, the line parser, and the
command registry (one entry per `Command` variant). The runtime crates own the
session, the socket, and the terminal.

See `SL_REPL_ROAD_MAP.md` in the workspace root for the phased plan.
