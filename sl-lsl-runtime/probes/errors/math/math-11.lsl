// probe: errors/math
// script: Math 11
// order: 11
// drive: none
//
// One of the faulters: a script of its own per case, since a fault stops
// a script on Second Life. It runs its case when sent the number its name
// ends in as a link message, the zero it divides by read from the
// message's text ("0") so that no compiler folds the expression away,
// and says what it got, or nothing more when the case faults. Link
// message 1000 pings it.
string ID = "errors/math";

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

run(string zero)
{
    float zf = (float)zero;
    string r;
    emit("case", " script=" + q(llGetScriptName()) + " case=log-zero step=before");
    r = (string)llLog(zf) + " " + (string)llLog10(zf - 1.0);
    emit("case", " script=" + q(llGetScriptName()) + " case=log-zero step=after result=" + q(r));
}

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 11) run(msg);
        else if (num == 1000) emit("answer", " script=" + q(llGetScriptName()));
    }
}
