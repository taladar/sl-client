// probe: http/options
// script: Agent space
// order: 1
// drive: none
//
// The Probe's neighbour (see there): as soon as it is saved it sends a request
// with an HTTP_USER_AGENT holding a space, which the library table says is a
// script error, then prints that it carried on and what comes back. It is a
// script of its own because a script error stops the script that raised it.
// An http_response reaches every script in the prim, so it prints only the
// answer to its own request.
string ID = "http/options";

key pending;

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
    state_entry()
    {
        emit("try", " label=agent-space");
        pending = llHTTPRequest("http://no-such-host.invalid/",
            [HTTP_USER_AGENT, "probe agent"], "");
        string request = "set";
        if (pending == NULL_KEY)
        {
            request = "null";
        }
        emit("carried-on", " label=agent-space request=" + request);
    }

    http_response(key id, integer status, list metadata, string body)
    {
        if (id != pending)
        {
            return;
        }
        emit("try-response", " label=agent-space status=" + (string)status
            + " length=" + (string)llStringLength(body)
            + " body=" + q(llGetSubString(body, 0, 299)));
    }
}
