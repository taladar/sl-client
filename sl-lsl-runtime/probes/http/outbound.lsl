// probe: http/outbound
// drive: none
// vary: region, position, here, rotation, turn, velocity
//
// What llHTTPRequest sends and what comes back, measured on the script's own
// in-world URL, so that one script is both the client and the server and no
// request leaves the grid. The script asks for a URL (llRequestURL), then
// sends one request per step to <url>/s<step>, waiting for each answer (or six
// seconds) and 1.2 seconds more before the next, which stays under both
// grids' per-prim throttles. Its http_request handler prints how each request
// arrived — method, path, query, the length and start of the body and the
// headers llGetHTTPHeader gives — and answers it: "ok-<step>" with status
// 200, or for the steps that measure the answer, a 3000-character body or the
// status 201, 404 or 500. The http_response handler prints the status, the
// metadata, the body's length and its start. The steps vary the method, the
// mimetype, custom headers (allowed names and reserved ones), the
// "Pragma: no-cache" header, the request and the answer's body lengths, the
// maximum body length, and send to URLs no request can reach: one that is not
// a URL, an ftp:// URL, a port nothing listens on and a host that does not
// resolve. A request llHTTPRequest refuses prints its NULL_KEY; the runner
// notes what the grid says about it on DEBUG_CHANNEL. The owner's name is
// never printed: whether the header holds the owner's legacy name or user
// name is.
//
// On OpenSim the runner's `lsl_settings` must let scripts reach the
// simulator's own address (OutboundDisallowForUserScriptsExcept), which its
// defaults refuse like any other on the simulator's network.
string ID = "http/outbound";

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

// The start of a long text, quoted, with `...` where it was cut.
string clip(string s)
{
    if (llStringLength(s) > 40)
    {
        return q(llGetSubString(s, 0, 39) + "...");
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

// How a header's value compares with what the script expects of it.
string compared(string value, string expected)
{
    if (value == "")
    {
        return "empty";
    }
    if (value == expected)
    {
        return "match";
    }
    return "other";
}

// How x-script-url compares with the URL the script was given.
string script_url(string value)
{
    if (value == "")
    {
        return "empty";
    }
    if (value == url)
    {
        return "match";
    }
    if (value + "/" == url)
    {
        return "without-slash";
    }
    return "other";
}

string owner_name(string value)
{
    if (value == "")
    {
        return "empty";
    }
    if (value == llKey2Name(llGetOwner()))
    {
        return "legacy";
    }
    if (value == llGetUsername(llGetOwner()))
    {
        return "username";
    }
    return "other";
}

string address(string value)
{
    if (value == "")
    {
        return "empty";
    }
    if (llSubStringIndex(value, "127.") == 0)
    {
        return "loopback";
    }
    return "set";
}

// The host of a URL: what lies between "//" and the next "/".
string host_of(string u)
{
    string rest = llGetSubString(u, llSubStringIndex(u, "//") + 2, -1);
    integer slash = llSubStringIndex(rest, "/");
    if (slash < 0)
    {
        return rest;
    }
    return llGetSubString(rest, 0, slash - 1);
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

string h(key id, string name)
{
    return llGetHTTPHeader(id, name);
}

// Every header a request may carry, as llGetHTTPHeader gives it.
headers(key id)
{
    string pos_header = h(id, "x-secondlife-local-position");
    string rot_header = h(id, "x-secondlife-local-rotation");
    emit("headers", " step=" + (string)step
        + " shard=" + q(h(id, "x-secondlife-shard"))
        + " object_name=" + compared(h(id, "x-secondlife-object-name"), llGetObjectName())
        + " object_key=" + compared(h(id, "x-secondlife-object-key"), (string)llGetKey())
        + " region=" + q(h(id, "x-secondlife-region"))
        + " position=" + q(pos_header)
        + " here=" + q((string)llGetPos())
        + " rotation=" + q(rot_header)
        + " turn=" + q((string)llGetRot())
        + " velocity=" + q(h(id, "x-secondlife-local-velocity"))
        + " owner_name=" + owner_name(h(id, "x-secondlife-owner-name"))
        + " owner_key=" + compared(h(id, "x-secondlife-owner-key"), (string)llGetOwner()));
    emit("transport", " step=" + (string)step
        + " user_agent=" + q(h(id, "user-agent"))
        + " accept=" + q(h(id, "accept"))
        + " accept_charset=" + q(h(id, "accept-charset"))
        + " accept_encoding=" + q(h(id, "accept-encoding"))
        + " cache_control=" + q(h(id, "cache-control"))
        + " connection=" + q(h(id, "connection"))
        + " via=" + q(h(id, "via"))
        + " forwarded_for=" + address(h(id, "x-forwarded-for"))
        + " remote_ip=" + address(h(id, "x-remote-ip"))
        + " host=" + compared(h(id, "host"), host_of(url))
        + " script_url=" + script_url(h(id, "x-script-url")));
}

// Sends the request of this step; returns FALSE when there is none.
integer send()
{
    string to = url + "/s" + (string)step;
    list options = [];
    string body = "";
    string name;
    if (step == 1)
    {
        name = "default";
    }
    else if (step == 2)
    {
        name = "post-json";
        options = [HTTP_METHOD, "POST", HTTP_MIMETYPE, "application/json"];
        body = "{\"a\":1}";
    }
    else if (step == 3)
    {
        name = "post-default-type";
        options = [HTTP_METHOD, "POST"];
        body = "plain";
    }
    else if (step == 4)
    {
        name = "put";
        options = [HTTP_METHOD, "PUT"];
        body = "put-body";
    }
    else if (step == 5)
    {
        name = "delete";
        options = [HTTP_METHOD, "DELETE"];
    }
    else if (step == 6)
    {
        name = "patch";
        options = [HTTP_METHOD, "PATCH"];
        body = "patch-body";
    }
    else if (step == 7)
    {
        name = "head";
        options = [HTTP_METHOD, "HEAD"];
    }
    else if (step == 8)
    {
        name = "options";
        options = [HTTP_METHOD, "OPTIONS"];
    }
    else if (step == 9)
    {
        name = "lower-case";
        options = [HTTP_METHOD, "get"];
    }
    else if (step == 10)
    {
        name = "unknown-method";
        options = [HTTP_METHOD, "FOO"];
    }
    else if (step == 11)
    {
        name = "get-with-body";
        body = "get-body";
    }
    else if (step == 12)
    {
        name = "query";
        to = to + "?a=1&b=two%20words";
    }
    else if (step == 13)
    {
        name = "custom";
        options = [HTTP_CUSTOM_HEADER, "X-Probe", "one", HTTP_CUSTOM_HEADER, "X-Probe-Two", "two"];
    }
    else if (step == 14)
    {
        name = "custom-host";
        options = [HTTP_CUSTOM_HEADER, "Host", "example.com"];
    }
    else if (step == 15)
    {
        name = "custom-shard";
        options = [HTTP_CUSTOM_HEADER, "X-SecondLife-Shard", "Elsewhere"];
    }
    else if (step == 16)
    {
        name = "custom-user-agent";
        options = [HTTP_CUSTOM_HEADER, "User-Agent", "probe-agent"];
    }
    else if (step == 17)
    {
        name = "custom-accept";
        options = [HTTP_CUSTOM_HEADER, "Accept", "application/json"];
    }
    else if (step == 18)
    {
        name = "no-pragma";
        options = [HTTP_PRAGMA_NO_CACHE, FALSE];
    }
    else if (step == 19)
    {
        name = "long-request";
        options = [HTTP_METHOD, "POST"];
        body = fill(3000);
    }
    else if (step == 20)
    {
        name = "long-answer";
    }
    else if (step == 21)
    {
        name = "long-answer-max";
        options = [HTTP_BODY_MAXLENGTH, 16384];
    }
    else if (step == 22)
    {
        name = "long-answer-small";
        options = [HTTP_BODY_MAXLENGTH, 100];
    }
    else if (step == 23)
    {
        name = "max-over";
        options = [HTTP_BODY_MAXLENGTH, 16385];
    }
    else if (step == 24)
    {
        name = "status-201";
    }
    else if (step == 25)
    {
        name = "status-404";
    }
    else if (step == 26)
    {
        name = "status-500";
    }
    else if (step == 27)
    {
        name = "unknown-option";
        options = [99, 1];
    }
    else if (step == 28)
    {
        name = "not-a-url";
        to = "not a url";
    }
    else if (step == 29)
    {
        name = "ftp";
        to = "ftp://127.0.0.1/";
    }
    else if (step == 30)
    {
        name = "refused-port";
        to = "http://127.0.0.1:1/";
    }
    else if (step == 31)
    {
        name = "no-such-host";
        to = "http://no-such-host.invalid/";
    }
    else
    {
        return FALSE;
    }
    pending = llHTTPRequest(to, options, body);
    string request = "set";
    if (pending == NULL_KEY)
    {
        request = "null";
    }
    emit("sent", " step=" + (string)step + " case=" + name + " request=" + request);
    if (pending == NULL_KEY)
    {
        waiting = FALSE;
        llSetTimerEvent(1.2);
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
        string path = h(id, "x-path-info");
        integer at = (integer)llGetSubString(bare(path), 1, -1);
        emit("inbound", " step=" + (string)at
            + " method=" + q(method)
            + " path=" + q(path)
            + " query=" + q(h(id, "x-query-string"))
            + " length=" + (string)llStringLength(body)
            + " body=" + clip(body)
            + " content_type=" + q(h(id, "content-type"))
            + " content_length=" + q(h(id, "content-length"))
            + " pragma=" + q(h(id, "pragma"))
            + " x_probe=" + q(h(id, "x-probe"))
            + " x_probe_two=" + q(h(id, "x-probe-two")));
        if (at == 1 || at == 2)
        {
            headers(id);
        }
        if (at == 14)
        {
            emit("transport", " step=" + (string)at
                + " host=" + compared(h(id, "host"), host_of(url)));
        }
        if (at == 15)
        {
            emit("headers", " step=" + (string)at
                + " shard=" + q(h(id, "x-secondlife-shard")));
        }
        if (at == 16 || at == 17)
        {
            emit("transport", " step=" + (string)at
                + " user_agent=" + q(h(id, "user-agent"))
                + " accept=" + q(h(id, "accept")));
        }
        if (at >= 20 && at <= 23)
        {
            llHTTPResponse(id, 200, fill(3000));
        }
        else if (at == 24)
        {
            llHTTPResponse(id, 201, "created");
        }
        else if (at == 25)
        {
            llHTTPResponse(id, 404, "missing");
        }
        else if (at == 26)
        {
            llHTTPResponse(id, 500, "broken");
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
        emit("response", " step=" + (string)step
            + " status=" + (string)status
            + " metadata=" + q(llList2CSV(metadata))
            + " length=" + (string)llStringLength(body)
            + " body=" + clip(body));
        waiting = FALSE;
        llSetTimerEvent(1.2);
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
