// probe: errors/bounds
// script: Bounds 15
// order: 15
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
    emit("case", " script=" + q(llGetScriptName()) + " case=substring-ranges step=before");
    r = llGetSubString(s, 99, 100) + "|" + llGetSubString(s, -99, 99)
        + "|" + llGetSubString(s, 4, 1) + "|" + llGetSubString(s, -1, -3)
        + "|" + llDeleteSubString(s, 99, 100) + "|" + llDeleteSubString(s, 4, 1);
    emit("case", " script=" + q(llGetScriptName()) + " case=substring-ranges step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 15) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
