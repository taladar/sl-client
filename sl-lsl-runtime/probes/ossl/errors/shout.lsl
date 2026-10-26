// probe: ossl/errors
// vary: elapsed
// script: Shout
// prim: A
// order: 6
// drive: none
// library: opensim
//
// Calls osRegexIsMatch, allowed for everyone, with a pattern that is not a
// regular expression: an OSSL error that is shouted on DEBUG_CHANNEL and
// returns, rather than thrown.
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
        if (num == 6)
        {
            emit("call", " script=" + q(llGetScriptName()));
            float t = llGetTime();
            string r;
            r = (string)osRegexIsMatch("a", "(");
            emit("returned", " script=" + q(llGetScriptName())
                + " value=" + q(r)
                + " elapsed=" + (string)(llGetTime() - t));
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
