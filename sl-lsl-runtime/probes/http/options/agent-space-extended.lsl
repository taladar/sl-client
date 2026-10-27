// probe: http/options
// script: Agent space extended
// order: 2
// drive: none
//
// The Probe's neighbour (see there): Agent space's request again with
// HTTP_EXTENDED_ERROR on, so that a refusal may come back as an http_response
// instead of a script error. A script of its own for the same reason, and it
// too prints only the answer to its own request.
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
        emit("try", " label=agent-space-extended");
        pending = llHTTPRequest("http://no-such-host.invalid/",
            [HTTP_EXTENDED_ERROR, TRUE, HTTP_USER_AGENT, "probe agent"], "");
        string request = "set";
        if (pending == NULL_KEY)
        {
            request = "null";
        }
        emit("carried-on", " label=agent-space-extended request=" + request);
    }

    http_response(key id, integer status, list metadata, string body)
    {
        if (id != pending)
        {
            return;
        }
        emit("try-response", " label=agent-space-extended status=" + (string)status
            + " length=" + (string)llStringLength(body)
            + " body=" + q(llGetSubString(body, 0, 299)));
    }
}
