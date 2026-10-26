// probe: ossl/errors
// vary: elapsed
// script: Threat
// prim: A
// order: 1
// drive: none
// library: opensim
//
// Calls osTerrainSetHeight, which has no Allow_ line in the grid's [OSSL]
// configuration and is threat level High, over the configured VeryLow: the
// denial by threat level. It would set the ground at 128,128 to the height
// it already has, were it allowed.
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
        if (num == 1)
        {
            emit("call", " script=" + q(llGetScriptName()));
            float t = llGetTime();
            string r;
            r = (string)osTerrainSetHeight(128, 128, osGetTerrainHeight(128, 128));
            emit("returned", " script=" + q(llGetScriptName())
                + " value=" + q(r)
                + " elapsed=" + (string)(llGetTime() - t));
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
