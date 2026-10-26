// probe: ossl/errors
// vary: elapsed
// script: LightShare
// prim: A
// order: 7
// drive: none
// library: opensim
//
// Calls lsGetWindlightScene, from OSSL's sibling LightShare family
// (AllowLightShareFunctions), for the water colour.
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
        if (num == 7)
        {
            emit("call", " script=" + q(llGetScriptName()));
            float t = llGetTime();
            string r;
            r = llList2CSV(lsGetWindlightScene([WL_WATER_COLOR]));
            emit("returned", " script=" + q(llGetScriptName())
                + " value=" + q(r)
                + " elapsed=" + (string)(llGetTime() - t));
        }
        else if (num == 100)
            emit("answer", " script=" + q(llGetScriptName()));
    }
}
