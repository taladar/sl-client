---
id: server-lsl-runtime-errors
title: Run-time errors where a resident can see them
topic: server
status: in-progress
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 3
blocked_by: [server-lsl-vm-execution]
refs: [server-world-chat-routing, server-lsl-memory-and-limits,
  viewer-lsl-editor-save-compile]
---

Context: [context/lsl.md](../context/lsl.md).

A script that fails at run time on a real grid does three things, none of
them silent: it shouts the error on **`DEBUG_CHANNEL`**
(`0x7FFFFFFF`, 2147483647) from the object's position, it stops, and the
viewer pops its script-warning window showing object name, owner, region
position and the message. Firestorm's `llfloaterscriptdebug.cpp` is the
consumer; a `ChatFromSimulator` with `ChatType::Debug` is the wire.

Wanted, once the VM can fail ([[server-lsl-vm-execution]]):

- a single error path that formats the reference's message shapes —
  "Math Error", "Stack-Heap Collision", "Script run-time error", with
  the script name and the line the source map yields
  ([[server-lsl-compiler-ir]] carries the span);
- delivery as a `DEBUG_CHANNEL` shout through the region's chat fan-out
  ([[server-world-chat-routing]]), so a script listening on
  `DEBUG_CHANNEL` hears it exactly as on a real grid — the standard way
  content self-reports;
- the script left **stopped** in the state it failed in, so
  `llGetScriptState` and the viewer's Running checkbox agree;
- `llScriptDanger` and `llGetScriptState` answering from the same
  record.

There is a viewer half too, but it is not this task's: check whether the
Bevy viewer renders a `ChatType::Debug` message anywhere, and if not,
raise a separate `viewer-*` task for a script-warning surface rather
than widening this one. The editor's *compile* errors already have a
home ([[viewer-lsl-editor-save-compile]] renders them as a listed
report); run-time errors do not.

Acceptance: a script dividing by zero produces one `DEBUG_CHANNEL`
message naming the script and the line, stops, and reports stopped; the
region keeps ticking; and a second script listening on `DEBUG_CHANNEL`
receives it.

## Progress (2026-09-27)

Done: `llGetScriptState` reads the instance's own run flag — the one a fault
clears — through a `Peers` view the engine lends a running script
(`Host::script_named` resolves the name). The VM already stopped a faulting
script in its state with its globals, and carried the source position in the
`Fault`.

Open: the `DEBUG_CHANNEL` message itself. Scripts listen on that channel and
parse what they hear, so its text, the speaker name, the volume (range) and
whether a missing name in `llGetScriptState` also shouts must be measured on
aditi before they are written. The `llScriptDanger` item above is a slip: it
answers whether a *position* is no-script/damage land, which is parcel data,
not this record; it stays with [[server-lsl-lib-task-inventory]].

Our client can now plant scripts on aditi itself (`RezScript` and
`UpdateScriptTask` work there — [[test-phase-z-deferred-04]] was a harness
bug), so the probe can run without hands; until that is wired up the scripts
go in by hand in Firestorm.
Setup: prim A holds five scripts named exactly `Controller`, `Div int`,
`Mod int`, `Div float` and `Recurse`; prim B, a separate object about 50 m
away (past say range, inside shout range), holds the listener. Touch prim A;
the sequence runs ~30 s and ends with "sequence end". Record all owner-say
chat of both prims and the Script Debug window's text verbatim (with any
object or script-name prefix); if prim B hears nothing at 50 m, repeat at
10 m. All six compile under tailslide.

Also open once the text is known: a `Host::say` (the book's sketched shape)
through which the engine shouts the fault, and a test where a second script's
`listen` handler receives it through a mock chat fan-out — the real fan-out is
[[server-world-chat-routing]], and a script on the fake grid needs
[[server-fake-grid-script-engine-wiring]].

`controller.lsl`:

```lsl
// Prim A. Touch to run the sequence: each faulter in turn, then the
// states, then a restart of "Div int" to see where it carries on.
list NAMES = ["Div int", "Mod int", "Div float", "Recurse"];
integer step;

report()
{
    integer i;
    for (i = 0; i < llGetListLength(NAMES); ++i)
    {
        string n = llList2String(NAMES, i);
        llOwnerSay("state of " + n + " = " + (string)llGetScriptState(n));
    }
    llOwnerSay("state of Controller (self) = " + (string)llGetScriptState(llGetScriptName()));
    llOwnerSay("state of Nope (missing) = " + (string)llGetScriptState("Nope"));
}

default
{
    touch_start(integer count)
    {
        step = 0;
        llOwnerSay("sequence start");
        llSetTimerEvent(3.0);
    }

    timer()
    {
        ++step;
        if (step == 1) llMessageLinked(LINK_THIS, 1, "0", "");      // Div int
        else if (step == 2) llMessageLinked(LINK_THIS, 3, "0", ""); // Mod int
        else if (step == 3) llMessageLinked(LINK_THIS, 4, "0", ""); // Div float
        else if (step == 4) llMessageLinked(LINK_THIS, 5, "0", ""); // Recurse
        else if (step == 6) report();
        else if (step == 7)
        {
            llOwnerSay("pinging the stopped Div int (expect no answer)");
            llMessageLinked(LINK_THIS, 100, "", "");
        }
        else if (step == 8)
        {
            llOwnerSay("restarting Div int");
            llSetScriptState("Div int", TRUE);
        }
        else if (step == 9) llMessageLinked(LINK_THIS, 100, "", "");
        else if (step == 10)
        {
            report();
            llSetTimerEvent(0.0);
            llOwnerSay("sequence end");
        }
    }
}
```

`div_int.lsl`:

```lsl
// Prim A, named "Div int".
integer g = 1;

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 1)
        {
            g = 42;
            state armed;
        }
        else if (num == 100) llOwnerSay("Div int answers: state default, g=" + (string)g);
    }
}

state armed
{
    state_entry()
    {
        llMessageLinked(LINK_THIS, 2, "0", "");
    }

    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2)
        {
            llOwnerSay("Div int: dividing");
            llOwnerSay("Div int: result " + (string)(1 / (integer)msg));
            llOwnerSay("Div int: after the division (should not be reached)");
        }
        else if (num == 100) llOwnerSay("Div int answers: state armed, g=" + (string)g);
    }
}
```

`mod_int.lsl`:

```lsl
// Prim A, named "Mod int".
default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 3) llOwnerSay("Mod int: result " + (string)(7 % (integer)msg));
    }
}
```

`div_float.lsl`:

```lsl
// Prim A, named "Div float".
default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 4) llOwnerSay("Div float: result " + (string)(1.0 / (float)msg));
    }
}
```

`recurse.lsl`:

```lsl
// Prim A, named "Recurse".
integer deep(integer depth)
{
    return deep(depth + 1) + 1;
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 5) llOwnerSay("Recurse: result " + (string)deep(0));
    }
}
```

`listener.lsl`:

```lsl
// Prim B, a separate object ~50 m from prim A.
default
{
    state_entry()
    {
        llListen(DEBUG_CHANNEL, "", NULL_KEY, "");
        llOwnerSay("listening on DEBUG_CHANNEL");
    }

    listen(integer channel, string name, key id, string message)
    {
        vector at = llList2Vector(llGetObjectDetails(id, [OBJECT_POS]), 0);
        list lines = llParseStringKeepNulls(message, ["\n"], []);
        string escaped = llDumpList2String(lines, "\\n");
        llOwnerSay("heard name=[" + name + "] id=" + (string)id
            + " dist=" + (string)llVecDist(at, llGetPos())
            + " len=" + (string)llStringLength(message)
            + " msg=[" + escaped + "]");
    }
}
```
