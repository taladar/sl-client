// probe: errors/bounds
// script: Bounds 18
// order: 18
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
    emit("case", " script=" + q(llGetScriptName()) + " case=ord-char-out-of-range step=before");
    r = (string)llOrd(s, 99) + "|" + (string)llOrd(s, -99) + "|" + q(llChar(-1))
        + "|" + q(llChar(1114112)) + "|" + q(llChar(0));
    emit("case", " script=" + q(llGetScriptName()) + " case=ord-char-out-of-range step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 18) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
