// probe: http/options
// script: Probe
// order: 3
// drive: none
//
// The llHTTPRequest options http/outbound leaves out because the library
// table did not know them then: HTTP_USER_AGENT, HTTP_ACCEPT and
// HTTP_EXTENDED_ERROR. Like http/outbound, the script sends each request to
// its own in-world URL (llRequestURL), to <url>/s<step>, waiting for each
// answer (or six seconds) and 1.2 seconds more before the next — four after
// a refusal or an error, since Second Life stops passing on a prim's
// DEBUG_CHANNEL complaints when they come faster ("Too many errors..."). Its
// http_request handler prints the user-agent the request arrived with and
// answers "ok-<step>" with status 200 — as text/plain unless the step sets
// llSetContentType, or with 404 where the step says so. The http_response
// handler prints the status, the metadata, the body's length and its start,
// further for the extended-error steps, whose body may be a JSON problem
// report.
//
// The steps: the default user agent, one appended with HTTP_USER_AGENT, an
// empty one, a long one and the option twice; Accept for JSON answered with
// text/plain and with JSON, for text/plain, a list of two, */* and an empty
// one; then HTTP_EXTENDED_ERROR on for a request that succeeds, one the URL
// answers 404, and the requests http/outbound saw refused — not a URL, ftp://,
// a port nothing listens on, a host that does not resolve, an unknown option
// (after the flag and before it), a reserved custom header, a body limit past
// the maximum, an unknown method — and an Accept the answer does not meet;
// and the flag off for a URL that is not one. A user agent with a space, which
// the table calls a script error, is tried by the scripts Agent space and
// Agent space extended (see there). A request llHTTPRequest refuses prints
// its NULL_KEY; the runner notes what the grid says about it on DEBUG_CHANNEL.
//
// OpenSim knows none of the three options, so there the measurement is the
// compile error.
string ID = "http/options";

string url;
integer step;
key pending;
integer waiting;

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

begin()
{
    emit("start", " region=" + q(llGetRegionName())
        + " channel=" + q(llGetEnv("sim_channel"))
        + " version=" + q(llGetEnv("sim_version")));
}

// The start of a text, quoted, with `...` where it was cut after `n`
// characters.
string clip(string s, integer n)
{
    if (llStringLength(s) > n)
    {
        return q(llGetSubString(s, 0, n - 1) + "...");
    }
    return q(s);
}

string fill(integer n)
{
    string s = "0123456789";
    while (llStringLength(s) < n)
    {
        s += s;
    }
    return llGetSubString(s, 0, n - 1);
}

// A path without the slashes it starts with: on a grid whose URLs end in
// one, "<url>/s1" arrives as "//s1".
string bare(string path)
{
    while (llGetSubString(path, 0, 0) == "/")
    {
        path = llGetSubString(path, 1, -1);
    }
    return path;
}

// Sends the request of this step; returns FALSE when there is none.
integer send()
{
    string to = url + "/s" + (string)step;
    list options = [];
    string name;
    if (step == 1)
    {
        name = "agent-default";
    }
    else if (step == 2)
    {
        name = "agent";
        options = [HTTP_USER_AGENT, "probe-agent/1.0"];
    }
    else if (step == 3)
    {
        name = "agent-empty";
        options = [HTTP_USER_AGENT, ""];
    }
    else if (step == 4)
    {
        name = "agent-long";
        options = [HTTP_USER_AGENT, "probe/" + fill(300)];
    }
    else if (step == 5)
    {
        name = "agent-twice";
        options = [HTTP_USER_AGENT, "first/1", HTTP_USER_AGENT, "second/2"];
    }
    else if (step == 6)
    {
        name = "accept-json-served-text";
        options = [HTTP_ACCEPT, "application/json"];
    }
    else if (step == 7)
    {
        name = "accept-json-served-json";
        options = [HTTP_ACCEPT, "application/json"];
    }
    else if (step == 8)
    {
        name = "accept-text";
        options = [HTTP_ACCEPT, "text/plain"];
    }
    else if (step == 9)
    {
        name = "accept-list";
        options = [HTTP_ACCEPT, "application/json, text/plain"];
    }
    else if (step == 10)
    {
        name = "accept-any";
        options = [HTTP_ACCEPT, "*/*"];
    }
    else if (step == 11)
    {
        name = "accept-empty";
        options = [HTTP_ACCEPT, ""];
    }
    else if (step == 12)
    {
        name = "extended-ok";
        options = [HTTP_EXTENDED_ERROR, TRUE];
    }
    else if (step == 13)
    {
        name = "extended-404";
        options = [HTTP_EXTENDED_ERROR, TRUE];
    }
    else if (step == 14)
    {
        name = "extended-not-a-url";
        options = [HTTP_EXTENDED_ERROR, TRUE];
        to = "not a url";
    }
    else if (step == 15)
    {
        name = "extended-ftp";
        options = [HTTP_EXTENDED_ERROR, TRUE];
        to = "ftp://127.0.0.1/";
    }
    else if (step == 16)
    {
        name = "extended-refused-port";
        options = [HTTP_EXTENDED_ERROR, TRUE];
        to = "http://127.0.0.1:1/";
    }
    else if (step == 17)
    {
        name = "extended-no-such-host";
        options = [HTTP_EXTENDED_ERROR, TRUE];
        to = "http://no-such-host.invalid/";
    }
    else if (step == 18)
    {
        name = "extended-unknown-option";
        options = [HTTP_EXTENDED_ERROR, TRUE, 99, 1];
    }
    else if (step == 19)
    {
        name = "extended-after-unknown-option";
        options = [99, 1, HTTP_EXTENDED_ERROR, TRUE];
    }
    else if (step == 20)
    {
        name = "extended-reserved-header";
        options = [HTTP_EXTENDED_ERROR, TRUE, HTTP_CUSTOM_HEADER, "Host", "example.com"];
    }
    else if (step == 21)
    {
        name = "extended-max-over";
        options = [HTTP_EXTENDED_ERROR, TRUE, HTTP_BODY_MAXLENGTH, 16385];
    }
    else if (step == 22)
    {
        name = "extended-unknown-method";
        options = [HTTP_EXTENDED_ERROR, TRUE, HTTP_METHOD, "FOO"];
    }
    else if (step == 23)
    {
        name = "extended-accept-unmet";
        options = [HTTP_EXTENDED_ERROR, TRUE, HTTP_ACCEPT, "application/json"];
    }
    else if (step == 24)
    {
        name = "extended-off-not-a-url";
        options = [HTTP_EXTENDED_ERROR, FALSE];
        to = "not a url";
    }
    else
    {
        return FALSE;
    }
    pending = llHTTPRequest(to, options, "");
    string request = "set";
    if (pending == NULL_KEY)
    {
        request = "null";
    }
    emit("sent", " step=" + (string)step + " case=" + name + " request=" + request);
    if (pending == NULL_KEY)
    {
        waiting = FALSE;
        llSetTimerEvent(4.0);
    }
    else
    {
        waiting = TRUE;
        llSetTimerEvent(6.0);
    }
    return TRUE;
}

next()
{
    step = step + 1;
    if (!send())
    {
        llSetTimerEvent(0.0);
        llReleaseURL(url);
        emit("end", "");
    }
}

default
{
    state_entry()
    {
        begin();
        llRequestURL();
        llSetTimerEvent(20.0);
    }

    http_request(key id, string method, string body)
    {
        if (method == URL_REQUEST_GRANTED)
        {
            url = body;
            emit("url", " granted=1");
            llSetTimerEvent(0.0);
            next();
            return;
        }
        if (method == URL_REQUEST_DENIED)
        {
            emit("url", " granted=0 reason=" + q(body));
            emit("end", "");
            llSetTimerEvent(0.0);
            return;
        }
        integer at = (integer)llGetSubString(bare(llGetHTTPHeader(id, "x-path-info")), 1, -1);
        emit("inbound", " step=" + (string)at
            + " method=" + q(method)
            + " user_agent=" + clip(llGetHTTPHeader(id, "user-agent"), 120)
            + " accept=" + q(llGetHTTPHeader(id, "accept")));
        if (at == 7)
        {
            llSetContentType(id, CONTENT_TYPE_JSON);
            llHTTPResponse(id, 200, "{\"ok\":" + (string)at + "}");
        }
        else if (at == 13)
        {
            llHTTPResponse(id, 404, "missing");
        }
        else
        {
            llHTTPResponse(id, 200, "ok-" + (string)at);
        }
    }

    http_response(key id, integer status, list metadata, string body)
    {
        if (id != pending)
        {
            emit("stray", " status=" + (string)status);
            return;
        }
        integer keep = 40;
        if (step >= 12)
        {
            keep = 500;
        }
        emit("response", " step=" + (string)step
            + " status=" + (string)status
            + " metadata=" + q(llList2CSV(metadata))
            + " length=" + (string)llStringLength(body)
            + " body=" + clip(body, keep));
        waiting = FALSE;
        if (step >= 14)
        {
            llSetTimerEvent(4.0);
        }
        else
        {
            llSetTimerEvent(1.2);
        }
    }

    timer()
    {
        llSetTimerEvent(0.0);
        if (url == "")
        {
            emit("url", " granted=none");
            emit("end", "");
            return;
        }
        if (waiting)
        {
            emit("no-response", " step=" + (string)step);
            waiting = FALSE;
        }
        next();
    }
}
