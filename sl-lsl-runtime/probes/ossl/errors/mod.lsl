// probe: ossl/errors
// vary: elapsed
// script: Mod
// prim: A
// order: 8
// drive: none
// library: opensim
//
// Calls modSendCommand, from OSSL's sibling MOD family (AllowMODFunctions),
// for a module no region has: what it answers when nothing handles it.
// The answer is a request key, so it is reported as one.
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
        if (num == 8)
        {
            emit("call", " script=" + q(llGetScriptName()));
            float t = llGetTime();
            string r;
            r = (string)modSendCommand("sl-probe-none", "noop", "");
            emit("returned", " script=" + q(llGetScriptName())
                + " value=" + q(r)
                + " elapsed=" + (string)(llGetTime() - t));
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
