---
id: server-lsl-opensim-shared-lists
title: OpenSim's lists are shared objects — a change in place shows through every copy
topic: server
status: ideas
origin: server-lsl-ossl-culture-strings (2026-11-01)
refs: [server-lsl-ossl-culture-strings, server-lsl-lib-money-permissions,
  server-lsl-value-model]
---

Context: [context/lsl.md](../context/lsl.md).

On OpenSim an LSL list is a .NET object (`LSL_Types.list`, a class), and both
engines hand it on by reference: `list b = a;` makes `b` the same object, and
a list passed to a user function is the caller's object in the callee. Two
library functions change the object they are handed — `osListSortInPlace`
sorts it and `llSetPayPrice` pads its button list to five — and the change
then shows through **every** variable holding that object, not only the one
passed: `ossl/strings` (2026-11-01, both engines) sorted `a` and read `b`
sorted too after `b = a`, sorted a copy and read the original sorted, and
sorted a function's parameter and read the caller's variable sorted after
the call. A list built by an expression (`a + []`) is a new object, and
sorting it changes nothing.

The value model copies a list on every assignment, so the compiler writes the
change back only into the variable passed (`Instr::CallBuiltinWriteBack`,
`Instr::PadPayButtons`), and the replay of `ossl/strings` names the three
lines where OpenSim's aliasing shows as a deviation (`copied-copy`,
`copy-sorted-original`, `parameter-after`).

What it would take: a list identity that survives a copy on the OpenSim
flavours — minted when a list is built (a literal, a concatenation, any
library result) and kept by loads, stores, arguments and returns — and the
in-place functions writing their result to every global, every frame's
locals and every operand-stack entry holding that identity. It touches the
value model's `Value::List`, memory accounting (one object counted once, as
OpenSim's heap counts it? measure first) and persistence (identities across
a save). Nothing else in OpenSim's library changes a list in place, so the
two functions above are the whole observable; worth it only if content
relies on it.
