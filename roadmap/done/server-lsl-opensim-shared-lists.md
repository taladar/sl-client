---
id: server-lsl-opensim-shared-lists
title: OpenSim's lists are shared objects — a change in place shows through every copy
topic: server
status: done
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

## Done (2026-11-01)

The two functions were not the whole observable. An audit of every list
parameter in `LSL_Api.cs`, `OSSL_Api.cs`, `LS_Api.cs` and `MOD_Api.cs`, and of
both engines' code generators, found more, and the new probe `vm/list-identity`
(plain LSL, run on aditi and both OpenSim engines) measured all of it:

- **Changes in place**, besides `osListSortInPlace` and `llSetPayPrice`:
  `llDialog` adds `"Ok"` to an empty button list, `llJsonSetValue` adds `0` to
  empty specifiers, and `llJsonGetValue` and `llJsonValueType` do so for JSON
  that starts with neither `[` nor `{`. And **XEngine compiles `list + string`,
  `+ integer`, `+ float` and `+ key` to an operator that appends to its left
  operand** (`LSL_Types.list.ExtendAndAdd`): `list c = a + 2;` changes `a`
  itself, with no alias needed. YEngine builds a new list.
- **The argument handed back**: `llList2List` when both indices are still
  negative once the length is added and the start is past the end
  (`GetSublist`'s `return this`), `llListInsertList` into an empty list at an
  index of 0 or less, `llListReplaceList` from 0 to the end or past it.
- **What shares**: an assignment, a parameter, a return value, a `(list)`
  cast, a chained assignment and a global set from a local or the reverse.
- **Second Life** shares nothing, and keeps a key a key in `list + key`.

`persistence/lists` (rerez and restart, all three grids) measured the saved
form: YEngine keeps two globals and a sleeping handler's local sharing
through a take and a restart (its migration writes a list once and refers to
it after); XEngine's restart parts every list global (`ScriptBase.GetVars`
copies each) and a take resets it.

Built as `vm::objects`: on the OpenSim flavours each list held on the operand
stack (`Operand::Shared`), in a frame's locals and in the globals carries an
identity, minted when a variable is loaded or a list duplicated and handed on
by stores, arguments, returns and `(list)` casts. A change in place is written
to every holder of the identity, and charges no memory (YEngine is not told of
one either). The library reports changes through `ScriptCtx` / `PureCtx`
(`write_back(argument, list)`, applied even when the call then fails, and
`return_argument(argument)`). `Instr::CallBuiltinWriteBack` and
`Instr::PadPayButtons` are gone: the compiler's write-back reached only the
variable passed, and padded a child prim's list, which OpenSim does not touch.
`SavedScript` keeps the identities (format version 3), and
`SavedScript::separate_lists` gives XEngine's shutdown its copies.

Held by `vm::tests::identity`: `vm/list-identity` replayed line for line under
all three flavours, and the persistence rows through `Engine::take`,
`rez_saved`, `shut_down` and `start_up`. The three `ossl/strings` deviations
and `data/json`'s `shared-list` deviation agree now and are gone.

On the way: an object rezzed back from inventory just before a region restart
did not come back with the region on OpenSim (`back missing=A`), so the probe
restarts before it takes; and XEngine's compile of the probe took 33–39 s,
past the `lsl-probes` runner's 30 s step, which now waits up to 90 s for a
compile (noted in `test-conformance-script-compile-xengine-cold-start`).
