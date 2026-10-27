// probe: errors/bounds
// script: Bounds 4
// order: 4
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

string d(list x)
{
    return "[" + llList2CSV(x) + "]";
}

run()
{
    list l = ["a", "b", "c", "d", "e", "f"];
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=list2list-ranges step=before");
    r = d(llList2List(l, 99, 100)) + d(llList2List(l, 4, 1))
        + d(llList2List(l, -99, 99)) + d(llList2List(l, 1, -99))
        + d(llList2List(l, -1, -2));
    emit("case", " script=" + q(llGetScriptName()) + " case=list2list-ranges step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 4) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
