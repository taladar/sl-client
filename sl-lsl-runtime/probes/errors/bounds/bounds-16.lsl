// probe: errors/bounds
// script: Bounds 16
// order: 16
// drive: none
//
// One of the bounds cases: a script of its own per case, since a
// fault stops a script on Second Life. It runs its case when sent
// the number its name ends in as a link message and says what it
// got, or nothing more when the case faults. Link message 1000
// pings it.
string ID = "errors/bounds";

string q(string s)
{
    s = llDumpList2String(llParseStringKeepNulls(s, ["\\"], []), "\\\\");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\""], []), "\\\"");
    s = llDumpList2String(llParseStringKeepNulls(s, ["\n"], []), "\\n");
    return "\"" + s + "\"";
}

emit(string rec, string fields)
{
    llOwnerSay("PROBE " + ID + " rec=" + rec + fields);
}

run()
{
    string s = "abcdef";
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=insertstring-at step=before");
    r = llInsertString(s, 99, "x") + "|" + llInsertString(s, -5, "x")
        + "|" + llInsertString(s, -1, "x");
    emit("case", " script=" + q(llGetScriptName()) + " case=insertstring-at step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 16) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
