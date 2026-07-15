# sl-rlv

Pure decoder for the Second Life / OpenSim **RLV / RLVa** `@`-command chat
protocol — the language a worn attachment speaks to control the viewer. It is
the RLV counterpart of `sl-prim` and `sl-anim`: **Bevy-free and I/O-free**, with
no session and no grid, so a headless RLV-compliant client can use exactly this,
and it is unit-testable to the letter.

RLV is not a wire protocol. The carrier is ordinary **owner-say chat**
(`CHAT_TYPE_OWNER` on channel `0`) from an object the agent owns — nothing new
on the wire, which is why it works on any grid with no server support. A message
is an RLV command line when it starts with `@`; the viewer swallows it so it
never reaches the chat log. The payload is a **comma-separated list** of
commands, each `behaviour[:option]=param`, lower-cased.

This crate is the **language decoder** only — it turns a chat line into a typed
command stream. *Obeying* the commands (the restriction state and the
enforcement families) is a separate concern that builds on this.

## Usage

- `is_rlv_line(line)` tests the `@` prefix.
- `parse_chat_line(line)` strips the `@`, splits the payload on `,` (dropping
  empty fields), and decodes each field independently, returning a `Result` per
  field so one malformed command does not sink its neighbours.
- `RlvCommand::parse_field(field)` decodes a single `behaviour[:option]=param`
  field (a leading `@` is tolerated).

Each `RlvCommand` carries:

- the raw lower-cased `keyword` (so an unrecognised or newer-than-us behaviour
  round-trips as `RlvBehaviour::Unknown` without losing its spelling),
- the classified `behaviour` (one of the ~175 keywords in `ERlvBehaviour` plus
  the wire synonyms and deprecated aliases),
- `strict` — whether the `_sec` suffix was present on a behaviour that supports
  it (`@recvim_sec=n`),
- the `option` between behaviour and `=` (a UUID, exception, modifier or path,
  left raw for the consumer to interpret), and
- the classified `param` (`RlvParam`):
  - `Add` / `Remove` — `n` / `add` and `y` / `rem` toggle a restriction,
  - `Force` — `force` runs an action now (`@sit:<uuid>=force`),
  - `Reply { channel }` — a number makes it a query answered on that channel
    (`@version=2222`, `@getoutfit=1234`),
  - `Clear { filter }` — `@clear[=<filter>]` drops restrictions.

The param-classification precedence is faithful to the reference down to the
edge cases (`@clear=n` classifies as `Add` while the behaviour stays `Clear`).

The grammar and classification follow Firestorm's `rlvhandler.cpp`,
`rlvhelper.cpp` and `rlvdefines.h` (`ERlvBehaviour`, `ERlvParamType`,
`RLV_CMD_PREFIX`), reimplemented idiomatically rather than copied. Channel-0
owner-say gating is the caller's job — this crate decodes a line it is handed.
