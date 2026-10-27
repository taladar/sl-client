// probe: errors/bounds
// script: Bounds 13
// order: 13
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
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=statistics-odd step=before");
    r = (string)llListStatistics(LIST_STAT_MEAN, []) + "|"
        + (string)llListStatistics(99, [1, 2]) + "|"
        + (string)llListStatistics(LIST_STAT_MEDIAN, ["a", "b"]) + "|"
        + (string)llListStatistics(LIST_STAT_RANGE, []);
    emit("case", " script=" + q(llGetScriptName()) + " case=statistics-odd step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 13) run();
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
