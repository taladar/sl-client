// probe: ossl/errors
// vary: elapsed
// script: Listed
// prim: A
// order: 2
// drive: none
// library: opensim
//
// Calls osGetAgents, threat level None but named on an Allow_ line for the
// estate owner and managers only, which the probe's owner is not: the
// denial by the Allow_ list, beneath the configured threat level.
// Prints `call` before and `returned` after, so a call that throws shows
// as a `call` alone; answers the leader's ping (link message 100) with
// `answer`, which a script the error stopped does not.
string ID = "ossl/errors";

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

default
{
    link_message(integer sender, integer num, string msg, key id)
    {
        if (num == 2)
        {
            emit("call", " script=" + q(llGetScriptName()));
            float t = llGetTime();
            string r;
            r = llList2CSV(osGetAgents());
            emit("returned", " script=" + q(llGetScriptName())
                + " value=" + q(r)
                + " elapsed=" + (string)(llGetTime() - t));
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
